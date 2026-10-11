//! One HTTP client, one throttle, one retry rule for every service.
//!
//! ListenBrainz asks for no more than a request per second and rate-limits with
//! `429`; Qobuz and Last.fm rate-limit independently. Spacing every outbound
//! request through one throttle is what keeps the arm inside that guidance in
//! all four code paths that make requests.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Response, StatusCode};
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::config::Config;

/// A client that spaces requests and backs off from `429`.
pub struct Http {
    /// The underlying client.
    pub client: reqwest::Client,
    /// The endpoints and timings this run was configured with.
    pub cfg: Config,
    /// When the last ListenBrainz request was sent.
    last_listenbrainz: Mutex<Option<Instant>>,
    /// When the last request to any other service was sent.
    last_other: Mutex<Option<Instant>>,
}

impl Http {
    /// Build the client for one run.
    pub fn new(cfg: Config) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .user_agent(concat!("octx-jam/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("could not build an HTTP client")?;
        Ok(Self {
            client,
            cfg,
            last_listenbrainz: Mutex::new(None),
            last_other: Mutex::new(None),
        })
    }

    /// Send a ListenBrainz request, spaced and retried.
    pub async fn send<F>(&self, build: F) -> Result<Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        self.send_spaced(true, build).await
    }

    /// Send a request to a service that states no rate guidance.
    ///
    /// The one-request-per-second rule is ListenBrainz's. Applying it to a
    /// catalog lookup would spend a second per track and make a 25-track run
    /// take half a minute for no one's benefit.
    pub async fn send_other<F>(&self, build: F) -> Result<Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        self.send_spaced(false, build).await
    }

    /// Send a request, spaced and retried.
    ///
    /// The builder is a closure so a retry can construct a fresh request: a
    /// `RequestBuilder` cannot be reused.
    async fn send_spaced<F>(&self, listenbrainz: bool, build: F) -> Result<Response>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut attempt = 0u32;
        loop {
            self.space(listenbrainz).await;
            let response = build().send().await.map_err(|error| {
                anyhow::anyhow!(
                    "the request could not be sent ({})",
                    redact(&error.to_string())
                )
            })?;

            if response.status() == StatusCode::TOO_MANY_REQUESTS && attempt < self.cfg.max_retries
            {
                let delay = self.cfg.backoff * 2u32.pow(attempt);
                eprintln!(
                    "jam: rate limited by {} — retrying in {:.1}s",
                    response.url().host_str().unwrap_or("the service"),
                    delay.as_secs_f64()
                );
                attempt += 1;
                tokio::time::sleep(delay).await;
                continue;
            }
            return Ok(response);
        }
    }

    /// Wait until a request may be sent, then record that it was.
    async fn space(&self, listenbrainz: bool) {
        let interval = if listenbrainz {
            self.cfg.min_interval
        } else {
            self.cfg.other_interval
        };
        let last = if listenbrainz {
            &self.last_listenbrainz
        } else {
            &self.last_other
        };

        let mut last = last.lock().await;
        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < interval {
                tokio::time::sleep(interval - elapsed).await;
            }
        }
        *last = Some(Instant::now());
    }
}

/// Strip every URL's query string from a message.
///
/// `reqwest` quotes the URL it failed on, and Qobuz carries the account token
/// as a query parameter, so an error message is a credential leak unless the
/// query is removed. The host and path survive, which is what makes the
/// message useful.
#[must_use]
pub fn redact(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(start) = rest.find("http") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        match tail.find('?') {
            Some(query) => {
                // Keep the URL up to the query, then skip the query itself.
                let end_of_query = tail[query..]
                    .find(|c: char| c.is_whitespace() || c == ')')
                    .map(|offset| query + offset)
                    .unwrap_or(tail.len());
                out.push_str(&tail[..query]);
                rest = &tail[end_of_query..];
            }
            None => {
                out.push_str(tail);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The spacing the arm enforces between ListenBrainz requests.
#[must_use]
pub fn default_min_interval() -> Duration {
    Config::default().min_interval
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_spacing_is_the_guidance_listenbrainz_gives() {
        assert_eq!(default_min_interval(), Duration::from_secs(1));
    }

    #[test]
    fn a_failed_request_never_reports_its_query_string() {
        let message = "error sending request for url (https://www.qobuz.com/api.json/0.2/playlist/get?playlist_id=1&user_auth_token=super-secret): connection refused";
        let redacted = redact(message);
        assert!(!redacted.contains("super-secret"), "{redacted}");
        assert!(redacted.contains("playlist/get"), "{redacted}");
        assert!(redacted.contains("connection refused"), "{redacted}");
    }
}
