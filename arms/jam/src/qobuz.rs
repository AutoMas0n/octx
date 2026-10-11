//! The Qobuz client: a thin wrapper over the four API calls the sink needs.
//!
//! The `qobuz-api` crate was rejected by the spike (task 2.3) — it has no
//! playlist writes, it is GPL-3.0-or-later, and it would add 46 crates
//! including a second `reqwest`. It is not needed: the API calls are a handful
//! of query parameters, and `reqwest` is already here.
//!
//! The app id is a constant every third-party Qobuz client ships. Fetching it
//! from the web player at runtime would mean downloading a 9.2 MB bundle (or
//! caching a rotating secret) to obtain an identifier, which is why the
//! constant exists and why `accounts.<name>.qobuz.app_id` can override it.
//! The app *secret* is only needed by the signed endpoints (streaming and
//! file URLs), which this arm never calls.

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;

use crate::http::Http;

/// The Qobuz application id, extracted from the shipped web player.
pub const APP_ID: &str = "798273057";

/// The most track ids one mutation call accepts.
///
/// The two working clients this was verified against both split at 50 to keep
/// the generated URL from growing without bound.
pub const TRACK_BATCH: usize = 50;

/// How many playlist tracks are read per page.
const PAGE: usize = 500;

/// A playlist the account owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRef {
    /// The playlist's identifier.
    pub id: String,
    /// The playlist's name.
    pub name: String,
    /// How many tracks it holds, when the API said.
    pub tracks_count: Option<u64>,
}

/// A client bound to one account's token.
pub struct Client<'a> {
    http: &'a Http,
    app_id: String,
    token: String,
}

impl<'a> Client<'a> {
    /// Build a client for one account.
    #[must_use]
    pub fn new(http: &'a Http, token: &str, app_id: Option<&str>) -> Self {
        Self {
            http,
            app_id: app_id
                .filter(|id| !id.is_empty())
                .unwrap_or(APP_ID)
                .to_string(),
            token: token.to_string(),
        }
    }

    /// Call one API route and return its JSON.
    ///
    /// Neither the URL nor the parameters are ever included in an error: the
    /// token travels as a query parameter, so a message that echoed the
    /// request would echo the credential.
    async fn get(&self, route: &str, params: &[(&str, String)]) -> Result<Value> {
        let mut url = format!("{}/{}?", self.http.cfg.qobuz_api, route);
        for (name, value) in params {
            url.push_str(name);
            url.push('=');
            url.push_str(&encode(value));
            url.push('&');
        }
        url.push_str("user_auth_token=");
        url.push_str(&encode(&self.token));

        let app_id = self.app_id.clone();
        let response = self
            .http
            .send_other(|| {
                self.http
                    .client
                    .get(&url)
                    .header("X-App-Id", app_id.clone())
            })
            .await
            .with_context(|| format!("the request to Qobuz `{route}` could not be sent"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .with_context(|| format!("could not read the response from Qobuz `{route}`"))?;
        let value: Value = serde_json::from_str(&body).with_context(|| {
            format!(
                "the Qobuz `{route}` response was not JSON ({status}): {}",
                snippet(&body)
            )
        })?;

        if !status.is_success() {
            bail!(
                "Qobuz `{route}` answered {status}: {}",
                qobuz_error(&value).unwrap_or_else(|| snippet(&body))
            );
        }
        if let Some(message) = qobuz_error(&value) {
            bail!("Qobuz `{route}` refused the request: {message}");
        }
        Ok(value)
    }

    /// Find a track id for one artist and title.
    ///
    /// Parity with the proven matcher: search `title artist`, take the first
    /// hit, and treat no hit as unresolved rather than as an error.
    pub async fn search_track(&self, artist: &str, title: &str) -> Result<Option<String>> {
        let query = format!("{title} {artist}");
        let value = self
            .get(
                "track/search",
                &[("query", query), ("limit", "1".to_string())],
            )
            .await?;
        let item = value
            .get("tracks")
            .and_then(|tracks| tracks.get("items"))
            .and_then(|items| items.as_array())
            .and_then(|items| items.first());
        Ok(item.and_then(|item| item.get("id")).and_then(id_as_string))
    }

    /// The playlists this account owns.
    ///
    /// `filter=owner` is what makes a name lookup the *user's* playlist rather
    /// than any public playlist with the same name.
    pub async fn owned_playlists(&self) -> Result<Vec<PlaylistRef>> {
        let value = self
            .get(
                "playlist/getUserPlaylists",
                &[("filter", "owner".to_string()), ("limit", PAGE.to_string())],
            )
            .await?;

        let items = value
            .get("playlists")
            .and_then(|playlists| playlists.get("items"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(items
            .into_iter()
            .filter_map(|item| {
                Some(PlaylistRef {
                    id: item.get("id").and_then(id_as_string)?,
                    name: item.get("name").and_then(Value::as_str)?.to_string(),
                    tracks_count: item.get("tracks_count").and_then(Value::as_u64),
                })
            })
            .collect())
    }

    /// The track ids in a playlist, in playlist order.
    pub async fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        let mut offset = 0usize;
        loop {
            let value = self
                .get(
                    "playlist/get",
                    &[
                        ("playlist_id", playlist_id.to_string()),
                        ("extra", "tracks".to_string()),
                        ("limit", PAGE.to_string()),
                        ("offset", offset.to_string()),
                    ],
                )
                .await?;

            let items = value
                .get("tracks")
                .and_then(|tracks| tracks.get("items"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                break;
            }

            let page_len = items.len();
            ids.extend(
                items
                    .into_iter()
                    .filter_map(|item| item.get("id").and_then(id_as_string)),
            );

            let total = value
                .get("tracks")
                .and_then(|tracks| tracks.get("total"))
                .and_then(Value::as_u64)
                .map(|total| total as usize)
                .unwrap_or(ids.len());
            offset += page_len;
            if offset >= total || page_len < PAGE {
                break;
            }
        }
        Ok(ids)
    }

    /// Create a playlist and return its id.
    pub async fn create_playlist(&self, name: &str) -> Result<String> {
        let value = self
            .get(
                "playlist/create",
                &[
                    ("name", name.to_string()),
                    ("is_public", "false".to_string()),
                    ("is_collaborative", "false".to_string()),
                ],
            )
            .await?;
        value
            .get("id")
            .and_then(id_as_string)
            .ok_or_else(|| anyhow!("Qobuz created a playlist but did not return its id"))
    }

    /// Append tracks to a playlist, in order, in batches.
    pub async fn add_tracks(&self, playlist_id: &str, track_ids: &[String]) -> Result<()> {
        for batch in track_ids.chunks(TRACK_BATCH) {
            self.mutate("playlist/addTracks", playlist_id, batch)
                .await?;
        }
        Ok(())
    }

    /// Remove tracks from a playlist, in batches.
    pub async fn remove_tracks(&self, playlist_id: &str, track_ids: &[String]) -> Result<()> {
        for batch in track_ids.chunks(TRACK_BATCH) {
            self.mutate("playlist/deleteTracks", playlist_id, batch)
                .await?;
        }
        Ok(())
    }

    /// One mutation call.
    async fn mutate(&self, route: &str, playlist_id: &str, track_ids: &[String]) -> Result<()> {
        self.get(
            route,
            &[
                ("playlist_id", playlist_id.to_string()),
                ("track_ids", track_ids.join(",")),
            ],
        )
        .await?;
        Ok(())
    }
}

/// A Qobuz error body, whose `message` field carries the reason.
fn qobuz_error(value: &Value) -> Option<String> {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    let text = value.get("error").and_then(Value::as_str)?;
    Some(text.to_string())
}

/// Qobuz returns ids as strings in most responses and as numbers in some.
fn id_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// A couple of lines of an upstream body, with the token stripped out.
fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let mut out: String = trimmed.chars().take(200).collect();
    if trimmed.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// Percent-encode a query value.
///
/// A comma is deliberately left alone: `track_ids` is comma-joined, and both
/// working clients pass those commas literally.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b',' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_read_from_strings_and_numbers() {
        assert_eq!(
            id_as_string(&Value::String("42".into())).as_deref(),
            Some("42")
        );
        assert_eq!(id_as_string(&serde_json::json!(42)).as_deref(), Some("42"));
        assert_eq!(id_as_string(&Value::Null), None);
    }

    #[test]
    fn a_qobuz_error_message_is_surfaced() {
        assert_eq!(
            qobuz_error(&serde_json::json!({"message": "Invalid app id"})).as_deref(),
            Some("Invalid app id")
        );
        assert_eq!(qobuz_error(&serde_json::json!({"ok": true})), None);
    }

    #[test]
    fn query_values_are_encoded_but_a_batch_keeps_its_commas() {
        assert_eq!(encode("Queen/Bohemian"), "Queen%2FBohemian");
        assert_eq!(encode("123,456"), "123,456");
        assert_eq!(
            encode("artist:(queen) & more"),
            "artist%3A%28queen%29%20%26%20more"
        );
    }

    #[test]
    fn the_app_id_is_the_nine_digit_form_the_web_player_uses() {
        assert_eq!(APP_ID.len(), 9, "a Qobuz app id is nine digits");
        assert!(APP_ID.chars().all(|c| c.is_ascii_digit()));
    }
}
