//! The two documents the arm speaks in.
//!
//! Every source reduces to one of these and every sink consumes one, which is
//! what keeps a sink from knowing which source produced its input. Both are
//! serialize-only: the arm produces documents, it does not read them back.

use serde::Serialize;

/// Version of the document shapes. Bumped when a consumer must know.
pub const DOCUMENT_VERSION: u32 = 1;

/// The Last.fm API terms. Attribution travels in the data because the data is
/// what gets copied.
pub const LASTFM_TERMS_URL: &str = "https://www.last.fm/api/tos";

/// Where a playlist document came from, so a consumer can tell which
/// recommendation set it is looking at.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Provenance {
    /// `recs` or `radio`.
    pub source: String,
    /// The ListenBrainz user the recommendations were computed for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// The collaborative-filtering model's identity, when the source named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// When the model was trained, as a UNIX timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<i64>,
    /// How many recordings the model holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_mbid_count: Option<u64>,
    /// The LB Radio prompt, for that source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The LB Radio mode, for that source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// One playlist entry: what a sink needs to find the recording, plus the
/// fields a later matching pass would want (carried now so that adding it
/// needs no re-plumbing).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    /// MusicBrainz recording id.
    pub recording_mbid: String,
    /// Track title.
    pub title: String,
    /// Artist credit, as one string.
    pub artist_credit: String,
    /// Release title, when the source supplied one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Track length in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Recommendation strength, when the source ranked entries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// When the user last listened to it, as a UNIX timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_listened_at: Option<i64>,
}

/// The normalized playlist document.
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistDocument {
    /// Shape version.
    pub version: u32,
    /// Always `playlist`.
    pub kind: &'static str,
    /// Which source produced this document.
    pub source: Provenance,
    /// The entries, in the source's order.
    pub entries: Vec<Entry>,
}

impl PlaylistDocument {
    /// Wrap entries with their provenance.
    #[must_use]
    pub fn new(source: Provenance, entries: Vec<Entry>) -> Self {
        Self {
            version: DOCUMENT_VERSION,
            kind: "playlist",
            source,
            entries,
        }
    }
}

/// One listen, normalized.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Listen {
    /// When the track was played, as a UNIX timestamp.
    pub listened_at: i64,
    /// Track artist.
    pub artist: String,
    /// Track title.
    pub title: String,
    /// Release title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    /// Track length in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// The listen's stable identity, supplied to ListenBrainz inside
    /// `track_metadata.additional_info`.
    pub recording_msid: String,
}

/// The credit Last.fm's terms require on any copy of its data that is made
/// public. It travels inside the document because the document is the copy.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Attribution {
    /// Always `Last.fm`.
    pub source: &'static str,
    /// The Last.fm account the data belongs to.
    pub user: String,
    /// The account's Last.fm page.
    pub account_url: String,
    /// Last.fm's API terms.
    pub terms_url: &'static str,
    /// What the terms require of anyone passing this data on.
    pub notice: &'static str,
}

impl Attribution {
    /// The credit for a Last.fm account.
    #[must_use]
    pub fn lastfm(user: &str) -> Self {
        Self {
            source: "Last.fm",
            user: user.to_string(),
            account_url: format!("https://www.last.fm/user/{user}"),
            terms_url: LASTFM_TERMS_URL,
            notice: "Listen data from the Last.fm API, used under its terms: \
                     non-commercial use only, no sub-licensing, and this credit \
                     and link must be kept on any copy that is made public.",
        }
    }

    /// The one-line form printed on stderr.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "listen data from Last.fm ({}) — {} — used under {}",
            self.account_url, self.notice, self.terms_url
        )
    }
}

/// The normalized listen document.
#[derive(Debug, Clone, Serialize)]
pub struct ListenDocument {
    /// Shape version.
    pub version: u32,
    /// Always `listens`.
    pub kind: &'static str,
    /// Where the listens came from.
    pub source: String,
    /// The user the listens are attributed to, when the source said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// The credit the source's terms require, when it requires one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<Attribution>,
    /// The listens, in the order the source gave them.
    pub listens: Vec<Listen>,
}

impl ListenDocument {
    /// Wrap listens with their provenance.
    #[must_use]
    pub fn new(source: &str, user: Option<String>, listens: Vec<Listen>) -> Self {
        Self {
            version: DOCUMENT_VERSION,
            kind: "listens",
            source: source.to_string(),
            user,
            attribution: None,
            listens,
        }
    }

    /// Add the attribution the source's terms require.
    #[must_use]
    pub fn with_attribution(mut self, attribution: Attribution) -> Self {
        self.attribution = Some(attribution);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_document_serializes_entries_in_order() {
        let doc = PlaylistDocument::new(
            Provenance {
                source: "recs".to_string(),
                user: Some("automason".to_string()),
                model_id: Some("model-1".to_string()),
                last_updated: Some(1_760_000_000),
                total_mbid_count: Some(42),
                ..Provenance::default()
            },
            vec![
                Entry {
                    recording_mbid: "a".to_string(),
                    title: "first".to_string(),
                    artist_credit: "one".to_string(),
                    album: None,
                    duration_ms: Some(1000),
                    score: Some(0.5),
                    latest_listened_at: None,
                },
                Entry {
                    recording_mbid: "b".to_string(),
                    title: "second".to_string(),
                    artist_credit: "two".to_string(),
                    album: Some("release".to_string()),
                    duration_ms: None,
                    score: None,
                    latest_listened_at: Some(1_700_000_000),
                },
            ],
        );

        let value: serde_json::Value = serde_json::to_value(&doc).expect("serialize");
        assert_eq!(value["kind"], "playlist");
        assert_eq!(value["source"]["model_id"], "model-1");
        assert_eq!(value["entries"][0]["title"], "first");
        assert_eq!(value["entries"][1]["title"], "second");
        // Absent optional fields stay absent rather than becoming null.
        assert!(value["entries"][0].get("album").is_none());
    }

    #[test]
    fn listen_document_carries_its_credit() {
        let doc = ListenDocument::new(
            "lastfm",
            Some("jesse1819".to_string()),
            vec![Listen {
                listened_at: 1_760_000_000,
                artist: "Queen".to_string(),
                title: "Misfire".to_string(),
                album: Some("Sheer Heart Attack".to_string()),
                duration_ms: None,
                recording_msid: "id".to_string(),
            }],
        )
        .with_attribution(Attribution::lastfm("jesse1819"));

        let value: serde_json::Value = serde_json::to_value(&doc).expect("serialize");
        assert_eq!(value["attribution"]["source"], "Last.fm");
        assert_eq!(
            value["attribution"]["account_url"],
            "https://www.last.fm/user/jesse1819"
        );
        assert_eq!(value["attribution"]["terms_url"], LASTFM_TERMS_URL);
        assert!(value["attribution"]["notice"].as_str().is_some());
        assert_eq!(value["listens"][0]["recording_msid"], "id");
    }
}
