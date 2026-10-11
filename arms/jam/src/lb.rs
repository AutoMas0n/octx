//! The ListenBrainz client: recommendations, radio, normalization, listens,
//! and submission.
//!
//! Every endpoint is isolated here, and every response shape has a fixture
//! test, so an upstream change fails loudly instead of silently producing a
//! thin document.

use anyhow::{Context, Result, anyhow, bail};
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::document::{Entry, Listen};
use crate::http::Http;

/// Recordings asked for when the caller names no count.
pub const DEFAULT_COUNT: usize = 25;
/// The most the recommendation endpoint will return in one response.
pub const MAX_COUNT: usize = 1000;
/// The most recording MBIDs the player endpoint accepts in one request.
pub const PLAYER_CHUNK: usize = 50;
/// Listens fetched from ListenBrainz when the caller names no limit.
pub const DEFAULT_LISTEN_LIMIT: usize = 200;
/// The most listens the listen endpoint will return in one response.
pub const MAX_LISTEN_LIMIT: usize = 1000;
/// The most listens submitted in one `import` request.
pub const SUBMIT_BATCH: usize = 1000;

/// One recording the model recommends.
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    /// MusicBrainz recording id.
    pub recording_mbid: String,
    /// Recommendation strength.
    pub score: Option<f64>,
    /// When the user last listened to it.
    pub latest_listened_at: Option<i64>,
}

/// What the recommendation endpoint had to say.
#[derive(Debug, Clone, PartialEq)]
pub enum Recs {
    /// The model exists and holds these recordings.
    Ready {
        /// The recommended recordings.
        recommendations: Vec<Recommendation>,
        /// The model's identity.
        model_id: Option<String>,
        /// When the model was trained.
        last_updated: Option<i64>,
        /// How many recordings the model holds.
        total_mbid_count: Option<u64>,
    },
    /// No model exists for the user. `message` is ListenBrainz's own wording
    /// when it sent one.
    NoModel {
        /// The service's wording, when the response carried a body.
        message: Option<String>,
    },
    /// ListenBrainz does not know that user.
    UnknownUser,
}

impl Recs {
    /// Whether a `recs` run could proceed.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Recs::Ready { .. })
    }
}

/// Fetch a user's collaborative-filtering recording recommendations.
///
/// The endpoint is publicly readable, so no token is sent: the credential
/// names the user, it does not authorize the read.
pub async fn recs(http: &Http, user: &str, count: Option<usize>) -> Result<Recs> {
    let requested = count.unwrap_or(DEFAULT_COUNT).min(MAX_COUNT);
    let url = http.cfg.lb(&format!(
        "/1/cf/recommendation/user/{user}/recording?count={requested}"
    ));

    let response = http.send(|| http.client.get(&url)).await?;
    let status = response.status();

    if status.as_u16() == 204 {
        return Ok(Recs::NoModel { message: None });
    }
    if status.as_u16() == 404 {
        return Ok(Recs::UnknownUser);
    }
    let body = read_body(response, "the recommendation endpoint").await?;
    if !status.is_success() {
        bail!(
            "the ListenBrainz recommendation endpoint answered {status}: {}",
            snippet(&body)
        );
    }

    let envelope: RecsEnvelope = serde_json::from_str(&body).with_context(|| {
        format!(
            "the ListenBrainz recommendation response was not the expected shape: {}",
            snippet(&body)
        )
    })?;

    Ok(Recs::Ready {
        recommendations: envelope
            .payload
            .mbids
            .into_iter()
            .map(|item| Recommendation {
                recording_mbid: item.recording_mbid,
                score: item.score,
                latest_listened_at: item.latest_listened_at,
            })
            .collect(),
        model_id: envelope.payload.model_id,
        last_updated: envelope.payload.last_updated,
        total_mbid_count: envelope.payload.total_mbid_count,
    })
}

/// Turn a token into its user name.
pub async fn validate_token(http: &Http, token: &str) -> Result<String> {
    let url = http.cfg.lb("/1/validate-token");
    let header = token_header(token)?;
    let response = http
        .send(|| {
            http.client
                .get(&url)
                .header("Authorization", header.clone())
        })
        .await?;
    let status = response.status();
    let body = read_body(response, "the token endpoint").await?;
    if !status.is_success() {
        bail!(
            "ListenBrainz rejected the credential ({status}): {}",
            snippet(&body)
        );
    }
    let parsed: ValidateToken = serde_json::from_str(&body).with_context(|| {
        format!(
            "the token response was not the expected shape: {}",
            snippet(&body)
        )
    })?;
    parsed
        .user_name
        .ok_or_else(|| anyhow!("ListenBrainz did not name a user for this credential"))
}

/// Expand recording MBIDs into entries with title and artist credit.
///
/// Recordings are looked up in chunks, because the player endpoint accepts at
/// most [`PLAYER_CHUNK`] of them per request.
pub async fn expand(http: &Http, mbids: &[String]) -> Result<Vec<Entry>> {
    let mut entries = Vec::with_capacity(mbids.len());
    for chunk in mbids.chunks(PLAYER_CHUNK) {
        let ids = chunk.join(",");
        let url = format!("{}?recording_mbids={}", http.cfg.lb_player, urlencode(&ids));
        let response = http.send(|| http.client.post(&url)).await?;
        let status = response.status();
        let body = read_body(response, "the player endpoint").await?;
        if !status.is_success() {
            bail!(
                "the ListenBrainz player endpoint answered {status}: {}",
                snippet(&body)
            );
        }
        let envelope: PlayerEnvelope = serde_json::from_str(&body).with_context(|| {
            format!(
                "the player response was not the expected shape: {}",
                snippet(&body)
            )
        })?;
        entries.extend(envelope.playlist.playlist.into_entries());
    }
    Ok(entries)
}

/// Fetch an LB Radio playlist for a prompt.
///
/// LB Radio requires a user token and enforces a stricter per-token limit than
/// the global one; the shared throttle and `429` backoff cover both.
pub async fn radio(http: &Http, token: &str, prompt: &str, mode: &str) -> Result<Vec<Entry>> {
    let url = http.cfg.lb(&format!(
        "/1/explore/lb-radio?prompt={}&mode={}",
        urlencode(prompt),
        urlencode(mode)
    ));
    let header = token_header(token)?;
    let response = http
        .send(|| {
            http.client
                .get(&url)
                .header("Authorization", header.clone())
        })
        .await?;
    let status = response.status();
    let body = read_body(response, "the radio endpoint").await?;
    if status.as_u16() == 204 {
        bail!("LB Radio had nothing to offer for that prompt and mode");
    }
    if !status.is_success() {
        bail!(
            "the LB Radio endpoint answered {status}: {}",
            snippet(&body)
        );
    }
    let envelope: RadioEnvelope = serde_json::from_str(&body).with_context(|| {
        format!(
            "the radio response was not the expected shape: {}",
            snippet(&body)
        )
    })?;
    Ok(envelope.payload.jspf.playlist.into_entries())
}

/// Read a user's listens back from ListenBrainz.
///
/// Publicly readable, so no token is sent, and each listen keeps the identity
/// ListenBrainz returned rather than deriving a new one.
pub async fn user_listens(http: &Http, user: &str, limit: Option<usize>) -> Result<Vec<Listen>> {
    let count = limit.unwrap_or(DEFAULT_LISTEN_LIMIT).min(MAX_LISTEN_LIMIT);
    let url = http
        .cfg
        .lb(&format!("/1/user/{user}/listens?count={count}"));
    let response = http.send(|| http.client.get(&url)).await?;
    let status = response.status();
    let body = read_body(response, "the listens endpoint").await?;
    if !status.is_success() {
        bail!(
            "the ListenBrainz listens endpoint answered {status}: {}",
            snippet(&body)
        );
    }
    let envelope: ListensEnvelope = serde_json::from_str(&body).with_context(|| {
        format!(
            "the listens response was not the expected shape: {}",
            snippet(&body)
        )
    })?;

    Ok(envelope
        .payload
        .listens
        .into_iter()
        .map(|listen| Listen {
            listened_at: listen.listened_at,
            artist: listen.track_metadata.artist_name,
            title: listen.track_metadata.track_name,
            album: listen.track_metadata.release_name,
            duration_ms: None,
            // Preserved: re-delivering a user's own listens must not invent
            // parallel identities for plays ListenBrainz already holds.
            recording_msid: listen
                .recording_msid
                .unwrap_or_else(|| crate::scrobble::identity("", listen.listened_at, "", "")),
        })
        .collect())
}

/// Submit listens.
///
/// `import` is the only listen type that accepts more than one listen, so a
/// single listen goes as `single` and everything else is batched.
pub async fn submit(http: &Http, token: &str, listens: &[Listen]) -> Result<()> {
    if listens.is_empty() {
        return Ok(());
    }
    let header = token_header(token)?;

    for batch in listens.chunks(SUBMIT_BATCH) {
        let listen_type = if batch.len() == 1 { "single" } else { "import" };
        let payload: Vec<SubmitListen> = batch
            .iter()
            .map(|listen| SubmitListen {
                listened_at: listen.listened_at,
                track_metadata: SubmitTrackMetadata {
                    artist_name: listen.artist.clone(),
                    track_name: listen.title.clone(),
                    release_name: listen.album.clone(),
                    additional_info: AdditionalInfo {
                        recording_msid: listen.recording_msid.clone(),
                        duration_ms: listen.duration_ms,
                    },
                },
            })
            .collect();
        let body = serde_json::to_string(&SubmitBody {
            listen_type,
            payload,
        })
        .context("could not serialize the listens")?;

        let url = http.cfg.lb("/1/submit-listens");
        let response = http
            .send(|| {
                http.client
                    .post(&url)
                    .header("Authorization", header.clone())
                    .header("Content-Type", "application/json")
                    .body(body.clone())
            })
            .await?;
        let status = response.status();
        let text = read_body(response, "the submit endpoint").await?;
        if !status.is_success() {
            bail!("{}", submit_failure(status.as_u16(), &text));
        }
    }
    Ok(())
}

/// Describe a refused submission, distinguishing a token that cannot submit
/// from one ListenBrainz does not know.
fn submit_failure(status: u16, body: &str) -> String {
    let lower = body.to_lowercase();
    let permission = ["scope", "permission", "submit-listens", "not allowed"]
        .iter()
        .any(|needle| lower.contains(needle));
    if permission {
        return format!(
            "the ListenBrainz token may not submit listens ({status}): {}\n  \
             it needs the `listenbrainz:submit-listens` permission — create a token that \
             grants it and store it as this account's `listenbrainz.token` (`octx creds add jam`)",
            snippet(body)
        );
    }
    format!(
        "ListenBrainz refused the submission ({status}): {}",
        snippet(body)
    )
}

/// A `Token <token>` header, marked sensitive so it cannot be logged.
fn token_header(token: &str) -> Result<HeaderValue> {
    let mut value = HeaderValue::from_str(&format!("Token {token}"))
        .map_err(|_| anyhow!("the stored ListenBrainz credential is not a valid header value"))?;
    value.set_sensitive(true);
    Ok(value)
}

/// Read a response body, even when the status is a failure.
async fn read_body(response: reqwest::Response, what: &str) -> Result<String> {
    response
        .text()
        .await
        .with_context(|| format!("could not read the response from {what}"))
}

/// At most a couple of lines of an upstream body, for an error message.
fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let mut out: String = trimmed.chars().take(200).collect();
    if trimmed.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// Percent-encode the characters that matter in a query value.
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

#[derive(Deserialize)]
struct ValidateToken {
    #[serde(default)]
    user_name: Option<String>,
}

#[derive(Deserialize)]
struct RecsEnvelope {
    payload: RecsBody,
}

#[derive(Deserialize)]
struct RecsBody {
    #[serde(default)]
    mbids: Vec<RecsItem>,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    last_updated: Option<i64>,
    #[serde(default)]
    total_mbid_count: Option<u64>,
}

#[derive(Deserialize)]
struct RecsItem {
    recording_mbid: String,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    latest_listened_at: Option<i64>,
}

#[derive(Deserialize)]
struct ListensEnvelope {
    payload: ListensBody,
}

#[derive(Deserialize)]
struct ListensBody {
    #[serde(default)]
    listens: Vec<LbListen>,
}

#[derive(Deserialize)]
struct LbListen {
    listened_at: i64,
    #[serde(default)]
    recording_msid: Option<String>,
    track_metadata: LbTrackMetadata,
}

#[derive(Deserialize)]
struct LbTrackMetadata {
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    release_name: Option<String>,
}

/// The player endpoint wraps its JSPF twice: `{"playlist": {"playlist": …}}`.
#[derive(Deserialize)]
struct PlayerEnvelope {
    playlist: PlayerInner,
}

#[derive(Deserialize)]
struct PlayerInner {
    playlist: Jspf,
}

/// LB Radio wraps the same JSPF under `payload.jspf`, which itself carries the
/// `playlist` key — so `PlayerInner` does the second unwrap for both sources
/// and there is one mapper, not two code paths.
#[derive(Deserialize)]
struct RadioEnvelope {
    payload: RadioPayload,
}

#[derive(Deserialize)]
struct RadioPayload {
    jspf: PlayerInner,
}

#[derive(Deserialize)]
struct Jspf {
    #[serde(default)]
    track: Vec<JspfTrack>,
}

impl Jspf {
    fn into_entries(self) -> Vec<Entry> {
        self.track
            .into_iter()
            .filter_map(JspfTrack::into_entry)
            .collect()
    }
}

#[derive(Deserialize)]
struct JspfTrack {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    creator: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    duration: Option<Value>,
    #[serde(default)]
    identifier: Option<Identifiers>,
}

impl JspfTrack {
    /// Map one JSPF track onto an entry.
    ///
    /// A track with no recording MBID cannot be resolved to anything, so it is
    /// dropped rather than carried as an entry the sinks must special-case.
    fn into_entry(self) -> Option<Entry> {
        let recording_mbid = self.identifier?.recording_mbid()?;
        Some(Entry {
            recording_mbid,
            title: self.title.unwrap_or_default(),
            artist_credit: self.creator.unwrap_or_default(),
            album: self.album.filter(|album| !album.is_empty()),
            duration_ms: self.duration.as_ref().and_then(duration_ms),
            score: None,
            latest_listened_at: None,
        })
    }
}

/// JSPF allows one identifier or a list of them.
#[derive(Deserialize)]
#[serde(untagged)]
enum Identifiers {
    Many(Vec<String>),
    One(String),
}

impl Identifiers {
    /// The MusicBrainz recording id among the identifiers, when there is one.
    fn recording_mbid(&self) -> Option<String> {
        let values: &[String] = match self {
            Identifiers::Many(values) => values,
            Identifiers::One(value) => std::slice::from_ref(value),
        };
        values.iter().find_map(|value| {
            value
                .split_once("musicbrainz.org/recording/")
                .map(|(_, mbid)| mbid.trim_end_matches('/').to_string())
                .or_else(|| {
                    // A bare UUID is tolerated; nothing else is an MBID.
                    (!value.contains('/') && value.len() == 36).then(|| value.clone())
                })
        })
    }
}

/// JSPF carries the duration as a string of milliseconds.
fn duration_ms(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

#[derive(Serialize)]
struct SubmitBody<'a> {
    listen_type: &'a str,
    payload: Vec<SubmitListen>,
}

#[derive(Serialize)]
struct SubmitListen {
    listened_at: i64,
    track_metadata: SubmitTrackMetadata,
}

#[derive(Serialize)]
struct SubmitTrackMetadata {
    artist_name: String,
    track_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_name: Option<String>,
    additional_info: AdditionalInfo,
}

#[derive(Serialize)]
struct AdditionalInfo {
    recording_msid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_response_shape_maps_to_entries() {
        // Pinned against the live response: the double wrap, the string
        // duration, and the identifier list.
        let body = r#"{
            "playlist": {
                "playlist": {
                    "title": "Playlist from recordings",
                    "track": [{
                        "title": "Bohemian Rhapsody",
                        "creator": "Queen",
                        "album": "A Night at the Opera",
                        "duration": "356000",
                        "identifier": ["https://musicbrainz.org/recording/df3543df-dd02-4415-a92b-15ee3e3da451"],
                        "extension": {"https://musicbrainz.org/doc/jspf#track": {"added_at": "2026-10-11T18:26:02Z"}}
                    }]
                }
            }
        }"#;
        let envelope: PlayerEnvelope = serde_json::from_str(body).expect("parse");
        let entries = envelope.playlist.playlist.into_entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Bohemian Rhapsody");
        assert_eq!(entries[0].artist_credit, "Queen");
        assert_eq!(entries[0].album.as_deref(), Some("A Night at the Opera"));
        assert_eq!(entries[0].duration_ms, Some(356_000));
        assert_eq!(
            entries[0].recording_mbid,
            "df3543df-dd02-4415-a92b-15ee3e3da451"
        );
    }

    #[test]
    fn radio_response_shape_maps_through_the_same_mapper() {
        // Pinned against the live response: `payload.jspf` carries the same
        // `playlist` key as the player endpoint.
        let body = r#"{
            "payload": {
                "feedback": "Using seed artist Queen",
                "jspf": {
                    "playlist": {
                        "track": [{
                            "title": "Misfire",
                            "creator": "Queen",
                            "album": "Sheer Heart Attack",
                            "duration": "114000",
                            "identifier": ["https://musicbrainz.org/recording/1123e365-7b81-4a1f-b876-877c4bc0ca1e"]
                        }, {
                            "title": "no identifier at all",
                            "creator": "Nobody"
                        }]
                    }
                }
            }
        }"#;
        let envelope: RadioEnvelope = serde_json::from_str(body).expect("parse");
        let entries = envelope.payload.jspf.playlist.into_entries();
        assert_eq!(entries.len(), 1, "an entry without an MBID is dropped");
        assert_eq!(entries[0].title, "Misfire");
        assert_eq!(entries[0].duration_ms, Some(114_000));
    }

    #[test]
    fn a_single_identifier_string_is_accepted() {
        let identifiers =
            Identifiers::One("https://musicbrainz.org/recording/abc-123/".to_string());
        assert_eq!(identifiers.recording_mbid().as_deref(), Some("abc-123"));
    }

    #[test]
    fn a_bare_uuid_identifier_is_accepted() {
        let identifiers = Identifiers::Many(vec![
            "https://example.org/other".to_string(),
            "df3543df-dd02-4415-a92b-15ee3e3da451".to_string(),
        ]);
        assert_eq!(
            identifiers.recording_mbid().as_deref(),
            Some("df3543df-dd02-4415-a92b-15ee3e3da451")
        );
    }

    #[test]
    fn recommendation_response_shape_maps_to_items() {
        let body = r#"{
            "payload": {
                "count": 2,
                "last_updated": 1760000000,
                "model_id": "0fe09b1a",
                "total_mbid_count": 100,
                "mbids": [
                    {"recording_mbid": "a", "score": 0.5, "latest_listened_at": 1750000000},
                    {"recording_mbid": "b", "score": null, "latest_listened_at": null}
                ]
            }
        }"#;
        let envelope: RecsEnvelope = serde_json::from_str(body).expect("parse");
        assert_eq!(envelope.payload.mbids.len(), 2);
        assert_eq!(envelope.payload.model_id.as_deref(), Some("0fe09b1a"));
        assert_eq!(envelope.payload.last_updated, Some(1_760_000_000));
        assert_eq!(envelope.payload.total_mbid_count, Some(100));
        assert_eq!(envelope.payload.mbids[1].score, None);
    }

    #[test]
    fn listens_response_shape_maps_to_listens() {
        let body = r#"{
            "payload": {
                "count": 1,
                "latest_listen_ts": 1760000000,
                "listens": [{
                    "listened_at": 1759999999,
                    "recording_msid": "upstream-identity",
                    "track_metadata": {
                        "artist_name": "Drake, Rihanna",
                        "track_name": "Take Care",
                        "release_name": "Take Care",
                        "additional_info": {"recording_mbid": "x"}
                    }
                }]
            }
        }"#;
        let envelope: ListensEnvelope = serde_json::from_str(body).expect("parse");
        assert_eq!(envelope.payload.listens.len(), 1);
        assert_eq!(
            envelope.payload.listens[0].recording_msid.as_deref(),
            Some("upstream-identity")
        );
    }

    #[test]
    fn submit_body_puts_identity_where_listenbrainz_reads_it() {
        let listens = [Listen {
            listened_at: 1_760_000_000,
            artist: "Queen".to_string(),
            title: "Misfire".to_string(),
            album: Some("Sheer Heart Attack".to_string()),
            duration_ms: Some(114_000),
            recording_msid: "identity".to_string(),
        }];
        let payload: Vec<SubmitListen> = listens
            .iter()
            .map(|listen| SubmitListen {
                listened_at: listen.listened_at,
                track_metadata: SubmitTrackMetadata {
                    artist_name: listen.artist.clone(),
                    track_name: listen.title.clone(),
                    release_name: listen.album.clone(),
                    additional_info: AdditionalInfo {
                        recording_msid: listen.recording_msid.clone(),
                        duration_ms: listen.duration_ms,
                    },
                },
            })
            .collect();
        let body: Value = serde_json::to_value(SubmitBody {
            listen_type: "single",
            payload,
        })
        .expect("serialize");

        assert_eq!(body["payload"][0]["listened_at"], 1_760_000_000u64);
        assert_eq!(
            body["payload"][0]["track_metadata"]["additional_info"]["recording_msid"],
            "identity"
        );
        // Only the two keys ListenBrainz accepts at the top level of a listen.
        let keys: Vec<&str> = body["payload"][0]
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.len(), 2, "{keys:?}");
    }

    #[test]
    fn scope_failures_are_distinguished_from_unknown_credentials() {
        let scope = submit_failure(401, r#"{"error":"Token has no permission"}"#);
        assert!(scope.contains("may not submit"), "{scope}");
        assert!(scope.contains("submit-listens"), "{scope}");

        let unknown = submit_failure(401, r#"{"error":"No such token"}"#);
        assert!(unknown.contains("refused the submission"), "{unknown}");
        assert!(!unknown.contains("may not submit"), "{unknown}");
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(
            urlencode("artist:(queen) & more"),
            "artist%3A%28queen%29%20%26%20more"
        );
    }

    #[test]
    fn counts_are_clamped_to_the_endpoint_maximum() {
        assert_eq!(25usize.min(MAX_COUNT), 25);
        assert_eq!(5000usize.min(MAX_COUNT), 1000);
        assert_eq!(5000usize.min(MAX_LISTEN_LIMIT), 1000);
    }
}
