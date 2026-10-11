//! Reading listens from NDJSON, and giving each listen a stable identity.
//!
//! Identity is what makes the write side safe to schedule: ListenBrainz
//! discards a listen whose user, timestamp, and identity it already holds, so
//! re-reading the whole input every run needs no offset and no state file.

use serde::Deserialize;
use uuid::Uuid;

use crate::document::Listen;

/// Namespace for derived listen identities.
///
/// Fixed forever: changing it would re-identify every listen the arm has
/// already delivered, which is the one way to smuggle duplicates past
/// ListenBrainz's uniqueness constraint.
pub const LISTEN_NAMESPACE: Uuid = uuid::uuid!("6f6a1d1e-2f2b-4c8e-9d3a-8b1c6f0a4e11");

/// Derive a listen's stable identity.
///
/// The inputs are the listen itself — the account's user, the timestamp, the
/// artist, and the title — so two runs over the same listen derive the same
/// identity, and two different listens do not collide. `user` is empty when a
/// run needs no account; the identity is then still stable, because the same
/// invocation shape always derives it the same way.
#[must_use]
pub fn identity(user: &str, listened_at: i64, artist: &str, title: &str) -> String {
    // Unit separator: it cannot occur in a user name, artist, or title, so no
    // two different listen tuples can produce the same name.
    let name = format!("{user}\u{1f}{listened_at}\u{1f}{artist}\u{1f}{title}");
    Uuid::new_v5(&LISTEN_NAMESPACE, name.as_bytes()).to_string()
}

/// A line that could not be turned into a listen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// 1-based line number in the input.
    pub line: usize,
    /// Why it was rejected.
    pub reason: String,
}

/// What an input yielded.
#[derive(Debug, Default)]
pub struct Parsed {
    /// The listens, in input order, with repeats collapsed.
    pub listens: Vec<Listen>,
    /// The lines that were rejected.
    pub rejections: Vec<Rejection>,
}

impl Parsed {
    /// The rejection report, one line per bad line.
    #[must_use]
    pub fn report(&self) -> String {
        self.rejections
            .iter()
            .map(|rejection| format!("  line {}: {}", rejection.line, rejection.reason))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Parse NDJSON: one listen per line, blank lines ignored.
///
/// A bad line is reported and skipped; it never costs the good lines around it.
#[must_use]
pub fn parse_ndjson(text: &str, user: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut seen: Vec<String> = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let raw: RawListen = match serde_json::from_str(trimmed) {
            Ok(raw) => raw,
            Err(error) => {
                parsed.rejections.push(Rejection {
                    line: number,
                    reason: format!("not a valid listen object ({error})"),
                });
                continue;
            }
        };

        let listened_at = match raw.listened_at.to_epoch() {
            Ok(seconds) => seconds,
            Err(reason) => {
                parsed.rejections.push(Rejection {
                    line: number,
                    reason,
                });
                continue;
            }
        };

        let recording_msid = identity(user, listened_at, &raw.artist, &raw.title);
        let listen = Listen {
            listened_at,
            artist: raw.artist,
            title: raw.title,
            album: raw.album.filter(|album| !album.is_empty()),
            duration_ms: raw.duration_ms,
            recording_msid,
        };

        // A repeat inside one input is one listen, not two.
        if seen.contains(&listen.recording_msid) {
            continue;
        }
        seen.push(listen.recording_msid.clone());
        parsed.listens.push(listen);
    }

    parsed
}

/// One line of the input.
#[derive(Debug, Deserialize)]
struct RawListen {
    /// When it was played: UNIX seconds, or an RFC 3339 timestamp.
    listened_at: RawTimestamp,
    /// Track artist.
    artist: String,
    /// Track title.
    title: String,
    /// Release title.
    #[serde(default)]
    album: Option<String>,
    /// Track length in milliseconds.
    #[serde(default)]
    duration_ms: Option<u64>,
}

/// An absolute timestamp, in either of the two shapes a hand-written input uses.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawTimestamp {
    /// UNIX seconds.
    Epoch(i64),
    /// An RFC 3339 timestamp.
    Text(String),
}

impl RawTimestamp {
    /// The timestamp as UNIX seconds.
    fn to_epoch(&self) -> Result<i64, String> {
        match self {
            RawTimestamp::Epoch(seconds) => Ok(*seconds),
            RawTimestamp::Text(text) => chrono::DateTime::parse_from_rfc3339(text)
                .map(|when| when.timestamp())
                .map_err(|error| {
                    format!(
                        "`{text}` is neither a UNIX timestamp nor an RFC 3339 timestamp ({error})"
                    )
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listens(text: &str) -> Parsed {
        parse_ndjson(text, "automason")
    }

    #[test]
    fn a_well_formed_line_becomes_a_listen() {
        let parsed = listens(r#"{"listened_at":1760000000,"artist":"Queen","title":"Misfire"}"#);
        assert!(parsed.rejections.is_empty(), "{:?}", parsed.rejections);
        assert_eq!(parsed.listens.len(), 1);
        assert_eq!(parsed.listens[0].artist, "Queen");
        assert_eq!(parsed.listens[0].title, "Misfire");
        assert_eq!(parsed.listens[0].listened_at, 1_760_000_000);
        assert_eq!(parsed.listens[0].album, None);
    }

    #[test]
    fn optional_fields_are_carried() {
        let parsed = listens(
            r#"{"listened_at":1760000000,"artist":"A","title":"T","album":"R","duration_ms":12345}"#,
        );
        assert_eq!(parsed.listens[0].album.as_deref(), Some("R"));
        assert_eq!(parsed.listens[0].duration_ms, Some(12_345));
    }

    #[test]
    fn an_rfc3339_timestamp_is_accepted() {
        let parsed = listens(r#"{"listened_at":"2025-09-14T12:00:00Z","artist":"A","title":"T"}"#);
        assert!(parsed.rejections.is_empty(), "{:?}", parsed.rejections);
        assert_eq!(parsed.listens[0].listened_at, 1_757_851_200);
    }

    #[test]
    fn blank_lines_are_ignored_rather_than_reported() {
        let parsed = listens("\n\n{\"listened_at\":1,\"artist\":\"A\",\"title\":\"T\"}\n\n");
        assert!(parsed.rejections.is_empty(), "{:?}", parsed.rejections);
        assert_eq!(parsed.listens.len(), 1);
    }

    #[test]
    fn order_is_preserved() {
        let parsed = listens(
            "{\"listened_at\":3,\"artist\":\"c\",\"title\":\"c\"}\n\
             {\"listened_at\":1,\"artist\":\"a\",\"title\":\"a\"}\n\
             {\"listened_at\":2,\"artist\":\"b\",\"title\":\"b\"}",
        );
        let times: Vec<i64> = parsed.listens.iter().map(|l| l.listened_at).collect();
        assert_eq!(times, vec![3, 1, 2]);
    }

    #[test]
    fn one_bad_line_does_not_cost_the_good_ones() {
        let parsed = listens(
            "{\"listened_at\":1,\"artist\":\"a\",\"title\":\"a\"}\n\
             not json\n\
             {\"listened_at\":2,\"artist\":\"b\"}\n\
             {\"listened_at\":\"whenever\",\"artist\":\"c\",\"title\":\"c\"}\n\
             {\"listened_at\":3,\"artist\":\"d\",\"title\":\"d\"}",
        );
        assert_eq!(parsed.listens.len(), 2, "{:?}", parsed.listens);
        assert_eq!(parsed.rejections.len(), 3);
        assert_eq!(parsed.rejections[0].line, 2);
        assert_eq!(parsed.rejections[1].line, 3);
        assert_eq!(parsed.rejections[2].line, 4);
        assert!(parsed.report().contains("line 2"), "{}", parsed.report());
    }

    #[test]
    fn a_repeat_inside_one_input_is_not_two_listens() {
        let line = "{\"listened_at\":1,\"artist\":\"a\",\"title\":\"a\"}";
        let parsed = listens(&format!("{line}\n{line}\n"));
        assert_eq!(parsed.listens.len(), 1);
    }

    #[test]
    fn identity_is_stable_across_runs() {
        let first = identity("automason", 1_760_000_000, "Queen", "Misfire");
        let second = identity("automason", 1_760_000_000, "Queen", "Misfire");
        assert_eq!(first, second);
    }

    #[test]
    fn identity_separates_listens_that_differ() {
        let base = identity("automason", 1_760_000_000, "Queen", "Misfire");
        assert_ne!(
            base,
            identity("automason", 1_760_000_001, "Queen", "Misfire")
        );
        assert_ne!(base, identity("automason", 1_760_000_000, "Queen", "Other"));
        assert_ne!(
            base,
            identity("automason", 1_760_000_000, "Other", "Misfire")
        );
        assert_ne!(base, identity("wife", 1_760_000_000, "Queen", "Misfire"));
    }

    #[test]
    fn identity_separates_listens_a_delimiter_could_merge() {
        // "a\u{1f}b" cannot be split the same way as "a" + "b", because neither
        // field may contain the separator.
        assert_ne!(identity("u", 1, "ab", "c"), identity("u", 1, "a", "bc"));
    }
}
