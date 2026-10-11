//! The Last.fm listen source.
//!
//! Qobuz publishes plays from its own servers, so Last.fm is the documented
//! route to a Qobuz history. `user.getRecentTracks` needs only the
//! application's API key — unsigned, no user token — which is why the key is
//! shared across accounts while the user name selects whose scrobbles are read.
//!
//! The mapper deliberately requests no `extended` parameter: adding it
//! silently re-shapes `artist` and `album` from `{"#text": …}` to `{"name": …}`.
//! Both shapes are accepted here anyway, so the day someone turns it on is not
//! the day the arm breaks.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::document::{Attribution, Listen};
use crate::http::Http;
use crate::scrobble::identity;

/// The most scrobbles the API returns in one page.
pub const PAGE_SIZE: usize = 200;
/// Scrobbles fetched when the caller names no limit.
pub const DEFAULT_LIMIT: usize = 200;
/// A guard against paging forever on an upstream that always answers.
const MAX_PAGES: usize = 20;

/// What a Last.fm fetch produced.
pub struct Fetched {
    /// The listens, newest first, as Last.fm returned them.
    pub listens: Vec<Listen>,
    /// The account the scrobbles belong to.
    pub user: String,
    /// The credit the API terms require on any copy of this data.
    pub attribution: Attribution,
}

/// Fetch an account's recent scrobbles.
///
/// `user` is the Last.fm account to read; `identity_user` is what listen
/// identities are derived from, so that the same invocations keep deriving the
/// same identities.
pub async fn recent_tracks(
    http: &Http,
    api_key: &str,
    user: &str,
    limit: usize,
    identity_user: &str,
) -> Result<Fetched> {
    let mut listens: Vec<Listen> = Vec::new();
    let per_page = limit.clamp(1, PAGE_SIZE);
    let mut page = 1usize;

    loop {
        let url = format!(
            "{}?method=user.getrecenttracks&user={}&api_key={}&format=json&limit={}&page={}",
            http.cfg.lastfm_api,
            urlencode(user),
            urlencode(api_key),
            per_page,
            page
        );
        let response = http.send_other(|| http.client.get(&url)).await?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("could not read the response from Last.fm")?;

        let parsed: RecentTracksResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "the Last.fm response was not the expected shape: {}",
                snippet(&body)
            )
        })?;

        // Last.fm reports its own errors in a 200-shaped body with `error`.
        if let Some(code) = parsed.error {
            bail!(
                "Last.fm refused the request (error {code}): {}",
                parsed.message.unwrap_or_else(|| "no message".to_string())
            );
        }
        if !status.is_success() {
            bail!("Last.fm answered {status}: {}", snippet(&body));
        }

        let recent = parsed
            .recenttracks
            .ok_or_else(|| anyhow::anyhow!("the Last.fm response carried no recent tracks"))?;

        if recent.track.is_empty() {
            break;
        }
        let last_page = recent
            .attr
            .as_ref()
            .and_then(|attr| attr.total_pages.as_deref())
            .and_then(|pages| pages.parse::<usize>().ok())
            .unwrap_or(page);

        for track in recent.track {
            if let Some(listen) = track.into_listen(identity_user) {
                listens.push(listen);
                if listens.len() >= limit {
                    break;
                }
            }
        }

        if listens.len() >= limit || page >= last_page || page >= MAX_PAGES {
            break;
        }
        page += 1;
    }

    listens.truncate(limit);
    Ok(Fetched {
        listens,
        user: user.to_string(),
        attribution: Attribution::lastfm(user),
    })
}

/// Percent-encode a query value.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A couple of lines of an upstream body, for an error message.
fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let mut out: String = trimmed.chars().take(200).collect();
    if trimmed.chars().count() > 200 {
        out.push('…');
    }
    out
}

#[derive(Deserialize)]
struct RecentTracksResponse {
    #[serde(default)]
    recenttracks: Option<RecentTracks>,
    #[serde(default)]
    error: Option<i64>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct RecentTracks {
    #[serde(default)]
    track: Vec<Scrobble>,
    #[serde(default, rename = "@attr")]
    attr: Option<PageAttr>,
}

#[derive(Deserialize)]
struct PageAttr {
    /// Last.fm spells it `totalPages`.
    #[serde(default, rename = "totalPages", alias = "total_pages")]
    total_pages: Option<String>,
}

#[derive(Deserialize)]
struct Scrobble {
    #[serde(default)]
    artist: Option<Text>,
    #[serde(default)]
    album: Option<Text>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    date: Option<Date>,
}

impl Scrobble {
    /// Map one scrobble to a listen.
    ///
    /// A scrobble with no timestamp is the track that is playing right now,
    /// which is not a listen and is dropped.
    fn into_listen(self, identity_user: &str) -> Option<Listen> {
        let listened_at = self.date?.uts?.parse::<i64>().ok()?;
        let artist = self.artist.and_then(Text::into_string).unwrap_or_default();
        let title = self.name.unwrap_or_default();

        Some(Listen {
            listened_at,
            recording_msid: identity(identity_user, listened_at, &artist, &title),
            artist,
            title,
            album: self.album.and_then(Text::into_string),
            duration_ms: None,
        })
    }
}

#[derive(Deserialize)]
struct Date {
    #[serde(default)]
    uts: Option<String>,
}

/// A Last.fm text field: an object keyed `#text`, or a bare string.
#[derive(Deserialize)]
#[serde(untagged)]
enum Text {
    Plain(String),
    Object {
        #[serde(default, rename = "#text")]
        text: Option<String>,
    },
}

impl Text {
    fn into_string(self) -> Option<String> {
        match self {
            Text::Plain(value) => Some(value),
            Text::Object { text } => text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrobble(body: &str) -> Vec<Listen> {
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        parsed
            .recenttracks
            .expect("recenttracks")
            .track
            .into_iter()
            .filter_map(|track| track.into_listen("automason"))
            .collect()
    }

    #[test]
    fn scrobbles_map_to_listens() {
        // Pinned against the live response: `#text` fields and a string `uts`.
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"#text": "Princess Goes", "mbid": ""},
                    "album": {"#text": "THANKS FOR COMING"},
                    "name": "Eat an Eraser",
                    "date": {"uts": "1760013000", "#text": "11 Oct 2026, 11:30"}
                }],
                "@attr": {"page": "1", "perPage": "200", "totalPages": "1", "total": "9"}
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens.len(), 1);
        assert_eq!(listens[0].listened_at, 1_760_013_000);
        assert_eq!(listens[0].artist, "Princess Goes");
        assert_eq!(listens[0].title, "Eat an Eraser");
        assert_eq!(listens[0].album.as_deref(), Some("THANKS FOR COMING"));
    }

    #[test]
    fn the_now_playing_track_is_not_a_listen() {
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"#text": "Queen"},
                    "album": {"#text": "A Night at the Opera"},
                    "name": "Bohemian Rhapsody",
                    "@attr": {"nowplaying": "true"}
                }, {
                    "artist": {"#text": "Queen"},
                    "album": {"#text": "Sheer Heart Attack"},
                    "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }]
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens.len(), 1);
        assert_eq!(listens[0].title, "Misfire");
    }

    #[test]
    fn the_extended_shape_is_tolerated() {
        // `extended=1` swaps `#text` for `name`; the mapper does not ask for it
        // but must not break if it ever arrives.
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"name": "Queen"},
                    "album": {"name": "Sheer Heart Attack"},
                    "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }]
            }
        }"##;
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        let tracks = parsed.recenttracks.expect("recenttracks").track;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name.as_deref(), Some("Misfire"));
    }

    #[test]
    fn the_page_count_is_read_from_the_attr_block() {
        // Pinned: Last.fm spells it `totalPages`, and getting this wrong makes
        // the fetch stop after one page without saying so.
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"#text": "Queen"}, "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }],
                "@attr": {"page": "1", "perPage": "200", "totalPages": "5", "total": "9"}
            }
        }"##;
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        assert_eq!(
            parsed
                .recenttracks
                .expect("recenttracks")
                .attr
                .expect("attr")
                .total_pages
                .as_deref(),
            Some("5")
        );
    }

    #[test]
    fn an_error_body_is_reported() {
        let body = r##"{"error":6,"message":"User not found"}"##;
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        assert_eq!(parsed.error, Some(6));
        assert_eq!(parsed.message.as_deref(), Some("User not found"));
    }

    #[test]
    fn a_bare_string_artist_is_accepted() {
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": "Queen",
                    "album": "Sheer Heart Attack",
                    "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }]
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens[0].artist, "Queen");
        assert_eq!(listens[0].album.as_deref(), Some("Sheer Heart Attack"));
    }

    #[test]
    fn an_unparseable_timestamp_is_dropped_rather_than_guessed() {
        let body = r##"{
            "recenttracks": {"track": [{
                "artist": {"#text": "Queen"}, "name": "Misfire", "date": {"uts": "soon"}
            }]}
        }"##;
        assert!(scrobble(body).is_empty());
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }
}
