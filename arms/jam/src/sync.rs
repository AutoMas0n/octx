//! Converging a playlist document onto a Qobuz playlist.
//!
//! The target is resolved on every run rather than remembered: a name is looked
//! up in the account's own playlists and created when it is missing, so a run
//! needs no state file and cannot act on a stale identifier. The default target
//! name differs per source, so two scheduled lines cannot converge on each
//! other's playlist.

use anyhow::Result;

use crate::document::Entry;
use crate::qobuz::{Client, PlaylistRef};

/// The default target for a source.
///
/// `recs` and `radio` keep their fixed names. The discovery source's name
/// carries the run's UTC date, so each day's run targets that day's playlist and
/// a same-day re-run converges on the same one. The date is passed in rather
/// than read here, so a run that spans midnight still targets one playlist.
#[must_use]
pub fn default_playlist(source: &str, date: &str) -> String {
    match source {
        "radio" => "ListenBrainz Radio".to_string(),
        "discover" => format!("Last.fm Discovery {date}"),
        _ => "ListenBrainz Discovery".to_string(),
    }
}

/// How many discovery playlists are kept when pruning runs.
///
/// The retention rule removes all but the newest ten by default. `--keep`
/// switches the whole step off rather than raising this number, so there is one
/// number to reason about and one way to opt out.
pub const KEEP: usize = 10;

/// The prefix every discovery playlist name carries.
const DISCOVERY_PREFIX: &str = "Last.fm Discovery ";

/// Whether a name is one the discovery source produced.
///
/// The name is the only provenance the arm has. The match is exact: the prefix,
/// a `YYYY-MM-DD` date, and nothing else. A name that merely starts with the
/// prefix — or carries anything after the date — is not a candidate.
#[must_use]
pub fn is_discovery_playlist(name: &str) -> bool {
    name.strip_prefix(DISCOVERY_PREFIX).is_some_and(is_iso_date)
}

/// Whether `value` is shaped exactly like `YYYY-MM-DD`.
fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

/// What a run did, for the summary and the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The playlist's name, or its id when the caller named an id.
    pub target: String,
    /// The playlist's identifier, for the retention step to keep it safe.
    pub playlist_id: String,
    /// Whether the playlist had to be created.
    pub created: bool,
    /// Entries that matched a Qobuz track.
    pub resolved: usize,
    /// Entries that did not match any Qobuz track.
    pub unresolved: usize,
    /// Tracks added to the playlist.
    pub added: usize,
    /// Tracks removed from the playlist.
    pub removed: usize,
    /// Whether the run stopped short of writing.
    pub dry_run: bool,
    /// Older discovery playlists removed by the retention step.
    pub pruned: usize,
}

impl Summary {
    /// The one-line report written to stderr.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "jam: `{}` — resolved {}, unresolved {}, added {}, removed {}{}{}{}",
            self.target,
            self.resolved,
            self.unresolved,
            self.added,
            self.removed,
            if self.created { ", created" } else { "" },
            if self.pruned > 0 {
                format!(", pruned {}", self.pruned)
            } else {
                String::new()
            },
            if self.dry_run {
                " (dry run, nothing written)"
            } else {
                ""
            }
        )
    }
}

/// The additions and removals that take the playlist to the document.
///
/// Order is preserved and repeats are collapsed: two entries that resolve to
/// one track are one addition, not two.
#[must_use]
pub fn plan(desired: &[String], current: &[String]) -> (Vec<String>, Vec<String>) {
    let mut seen: Vec<&String> = Vec::new();
    let mut wanted: Vec<&String> = Vec::new();
    for id in desired {
        if !seen.contains(&id) {
            seen.push(id);
            wanted.push(id);
        }
    }

    let added = wanted
        .iter()
        .filter(|id| !current.contains(id))
        .map(|id| (*id).clone())
        .collect();
    let removed = current
        .iter()
        .filter(|id| !wanted.contains(id))
        .map(Clone::clone)
        .collect();
    (added, removed)
}

/// Resolve and sync a document into the account's playlist.
pub async fn sync(
    client: &Client<'_>,
    entries: &[Entry],
    requested: Option<&str>,
    source: &str,
    date: &str,
    dry_run: bool,
) -> Result<Summary> {
    let (playlist_id, target, created) = resolve_target(client, requested, source, date).await?;

    // Resolve every entry; a miss or a failed search is reported and skipped,
    // never fatal — one entry must not cost the rest of the run.
    let mut desired = Vec::with_capacity(entries.len());
    let mut unresolved = 0usize;
    for entry in entries {
        match client
            .search_track(&entry.artist_credit, &entry.title)
            .await
        {
            Ok(Some(id)) => desired.push(id),
            Ok(None) => {
                unresolved += 1;
                eprintln!(
                    "jam: no Qobuz match for \"{}\" by {} — skipped",
                    entry.title, entry.artist_credit
                );
            }
            Err(error) => {
                unresolved += 1;
                eprintln!(
                    "jam: could not search Qobuz for \"{}\" by {} — skipped ({error:#})",
                    entry.title, entry.artist_credit
                );
            }
        }
    }

    let current = client.playlist_entries(&playlist_id).await?;
    let current_track_ids: Vec<String> =
        current.iter().map(|entry| entry.track_id.clone()).collect();
    let (added, removed) = plan(&desired, &current_track_ids);

    // Deletion takes the playlist's own entry ids, so the stale track ids are
    // mapped back through the entries that carry them. Playlist order is
    // preserved and an entry is named once however often its track repeats.
    let removals: Vec<String> = current
        .iter()
        .filter(|entry| removed.contains(&entry.track_id))
        .map(|entry| entry.entry_id.clone())
        .collect();

    if !dry_run {
        // Additions first: a run interrupted between the two leaves extra
        // tracks, never fewer than it found.
        if !added.is_empty() {
            client.add_tracks(&playlist_id, &added).await?;
        }
        if !removals.is_empty() {
            client.remove_entries(&playlist_id, &removals).await?;
        }
    }

    Ok(Summary {
        target,
        playlist_id,
        created,
        resolved: desired.len(),
        unresolved,
        added: added.len(),
        removed: removals.len(),
        dry_run,
        pruned: 0,
    })
}

/// Remove older discovery playlists so at most `keep` remain, newest kept.
///
/// Candidates are named by the discovery source and nothing else: a playlist
/// whose name does not match the discovery pattern is never touched. The run's
/// own playlist is excluded by id, so it survives even when its name would sort
/// past the limit. Called only after the delivery that wrote it, so a failed
/// delivery leaves every playlist in place. Returns how many were removed.
pub async fn prune(client: &Client<'_>, current: &str, keep: usize) -> Result<usize> {
    let mut candidates: Vec<PlaylistRef> = client
        .owned_playlists()
        .await?
        .into_iter()
        .filter(|playlist| is_discovery_playlist(&playlist.name) && playlist.id != current)
        .collect();

    // Newest first. A discovery name carries an ISO date, so lexical order is
    // date order and the newest sort to the front.
    candidates.sort_by(|left, right| right.name.cmp(&left.name));

    // The run's own playlist is already out of the list, so the kept count is
    // the remaining slots after it: `keep - 1` others, for `keep` in total.
    let mut removed = 0;
    for playlist in candidates.into_iter().skip(keep.saturating_sub(1)) {
        client.delete_playlist(&playlist.id).await?;
        removed += 1;
    }
    Ok(removed)
}

/// The target playlist's id and name, creating it when a name does not exist.
async fn resolve_target(
    client: &Client<'_>,
    requested: Option<&str>,
    source: &str,
    date: &str,
) -> Result<(String, String, bool)> {
    // A bare number is an identifier: it needs no lookup and no creation.
    if let Some(value) = requested
        && !value.is_empty()
        && value.chars().all(|c| c.is_ascii_digit())
    {
        return Ok((value.to_string(), value.to_string(), false));
    }

    let name = requested
        .map(str::to_string)
        .unwrap_or_else(|| default_playlist(source, date));
    if let Some(found) = client
        .owned_playlists()
        .await?
        .into_iter()
        .find(|playlist| playlist.name == name)
    {
        return Ok((found.id, name.clone(), false));
    }

    let id = client.create_playlist(&name).await?;
    Ok((id, name, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn the_source_order_is_the_added_order() {
        let (added, removed) = plan(&ids(&["c", "a", "b"]), &[]);
        assert_eq!(added, ids(&["c", "a", "b"]));
        assert!(removed.is_empty());
    }

    #[test]
    fn an_unchanged_playlist_needs_no_calls() {
        let (added, removed) = plan(&ids(&["a", "b"]), &ids(&["a", "b"]));
        assert!(added.is_empty(), "{added:?}");
        assert!(removed.is_empty(), "{removed:?}");
    }

    #[test]
    fn a_track_that_fell_out_of_the_ranking_is_removed() {
        let (added, removed) = plan(&ids(&["a"]), &ids(&["a", "b"]));
        assert!(added.is_empty());
        assert_eq!(removed, ids(&["b"]));
    }

    #[test]
    fn two_entries_that_resolve_to_one_track_are_one_addition() {
        let (added, _) = plan(&ids(&["a", "a"]), &[]);
        assert_eq!(added, ids(&["a"]));
    }

    #[test]
    fn defaults_differ_per_source() {
        let date = "2025-10-13";
        assert_ne!(
            default_playlist("recs", date),
            default_playlist("radio", date)
        );
    }

    #[test]
    fn the_discovery_default_carries_the_run_date() {
        assert_eq!(
            default_playlist("discover", "2025-10-13"),
            "Last.fm Discovery 2025-10-13"
        );
    }

    #[test]
    fn the_fixed_defaults_ignore_the_date() {
        assert_eq!(
            default_playlist("recs", "2025-10-13"),
            "ListenBrainz Discovery"
        );
        assert_eq!(
            default_playlist("radio", "2025-10-13"),
            "ListenBrainz Radio"
        );
    }

    #[test]
    fn the_summary_reports_what_happened() {
        let summary = Summary {
            target: "ListenBrainz Discovery".to_string(),
            playlist_id: "1234".to_string(),
            created: true,
            resolved: 23,
            unresolved: 2,
            added: 23,
            removed: 0,
            dry_run: true,
            pruned: 0,
        };
        let line = summary.line();
        assert!(line.contains("resolved 23"), "{line}");
        assert!(line.contains("unresolved 2"), "{line}");
        assert!(line.contains("added 23"), "{line}");
        assert!(line.contains("removed 0"), "{line}");
        assert!(line.contains("created"), "{line}");
        assert!(line.contains("dry run"), "{line}");
        assert!(!line.contains("pruned"), "{line}");
        assert!(!line.contains("deduplicat"), "{line}");
    }

    #[test]
    fn a_pruned_run_says_so_in_the_summary() {
        let summary = Summary {
            target: "Last.fm Discovery 2025-10-13".to_string(),
            playlist_id: "1234".to_string(),
            created: false,
            resolved: 20,
            unresolved: 0,
            added: 20,
            removed: 0,
            dry_run: false,
            pruned: 2,
        };
        let line = summary.line();
        assert!(line.contains("pruned 2"), "{line}");
    }

    #[test]
    fn only_the_discovery_names_are_candidates() {
        for name in [
            "Last.fm Discovery 2025-10-13",
            "Last.fm Discovery 2024-01-01",
        ] {
            assert!(is_discovery_playlist(name), "{name}");
        }
        for name in [
            "ListenBrainz Discovery",
            "ListenBrainz Radio",
            "Last.fm Discovery",
            "Last.fm Discovery ",
            "Last.fm Discovery 2025-10-13 (old)",
            "Last.fm Discovery 2025-1-3",
            "Last.fm Discovery 20251013",
            "last.fm discovery 2025-10-13",
            "Last.fm Discovery 2025-10-13x",
            "My Last.fm Discovery 2025-10-13",
        ] {
            assert!(!is_discovery_playlist(name), "{name}");
        }
    }
}
