//! The Last.fm-seeded discovery source.
//!
//! `jam discover` answers from Last.fm's similarity data rather than from
//! ListenBrainz's collaborative filtering. The account's own listening supplies
//! the seeds, each seed expands into the artists Last.fm calls similar to it,
//! and those artists' popular tracks become the candidates. Anything the
//! account has already scrobbled is excluded, and the candidates are drawn
//! across seeds so that one artist cannot fill the result.
//!
//! Last.fm yields artist and track *names* and no identifiers, so the entries
//! this source produces carry no recording MBID. That is why `Entry`'s MBID
//! became optional: the sink resolves on artist and title and never needed one.

use std::collections::HashSet;

use anyhow::Result;

use crate::document::Entry;
use crate::http::Http;
use crate::lastfm::{self, Track};

/// How many seeds one run expands, at most.
pub const MAX_SEEDS: usize = 5;

/// How many similar artists one seed contributes, at most.
pub const MAX_SIMILAR: usize = 5;

/// How many popular tracks one artist contributes, at most.
pub const MAX_TRACKS_PER_ARTIST: usize = 3;

/// How many already-scrobbled tracks are read for exclusion, at most.
pub const MAX_KNOWN: usize = 200;

/// Entries asked for when the caller names no count.
pub const DEFAULT_COUNT: usize = 25;

/// The most entries a caller may ask for.
pub const MAX_COUNT: usize = 100;

/// What a run seeds from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seed {
    /// The artists the account listens to most.
    Account,
    /// One artist, expanded through the artists Last.fm calls similar.
    Artist(String),
    /// A Last.fm tag.
    Tag(String),
}

/// What a discovery run produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Discovered {
    /// Entries, in the order the sink should write them.
    Ready(Vec<Entry>),
    /// Nothing was available to recommend.
    Nothing,
}

/// Why a run had nothing to seed from, and what to do about it.
#[must_use]
pub fn no_listening(user: &str) -> String {
    format!(
        "Last.fm has no listening to seed from for `{user}`, so there is nothing to write\n  \
         this source seeds from the artists the account most listens to; scrobble something \
         on Last.fm first, or pass `--seed-artist` or `--prompt`"
    )
}

/// Expand a seed into entries, in the order the sink should write them.
///
/// The seeds are expanded breadth-first and the candidates are interleaved
/// round-robin, so an early seed cannot fill the result before a later one is
/// heard from. Every candidate is filtered through one exclusion set, read
/// once here rather than per candidate.
pub async fn discover(
    http: &Http,
    api_key: &str,
    user: &str,
    seed: &Seed,
    count: Option<usize>,
) -> Result<Discovered> {
    let wanted = count.unwrap_or(DEFAULT_COUNT).min(MAX_COUNT);

    // A prompt reads the tag's own tracks; the other two seeds fan out
    // through similar artists. Reading the account's tags for a prompt would
    // find nothing on a young account, which is the case prompts exist for.
    if let Seed::Tag(tag) = seed {
        let tracks = lastfm::tag_top_tracks(http, api_key, tag, wanted).await?;
        let known = known_tracks(http, api_key, user).await?;
        let entries = interleave(vec![tracks], &known, wanted);
        return Ok(if entries.is_empty() {
            Discovered::Nothing
        } else {
            Discovered::Ready(entries)
        });
    }

    let seeds: Vec<String> = match seed {
        Seed::Account => lastfm::top_artists(http, api_key, user, MAX_SEEDS).await?,
        Seed::Artist(artist) => vec![artist.clone()],
        Seed::Tag(_) => unreachable!("the tag seed returned above"),
    };
    if seeds.is_empty() {
        return Ok(Discovered::Nothing);
    }

    // One exclusion read for the whole run, before any candidate is looked at.
    let known = known_tracks(http, api_key, user).await?;

    // Each seed contributes a bucket: the seed artist's own popular tracks and
    // those of the artists Last.fm calls similar to it. Buckets stay separate
    // so the draw below can spread across them.
    let mut buckets = Vec::with_capacity(seeds.len());
    for seed_artist in &seeds {
        let mut artists = vec![seed_artist.clone()];
        artists.extend(lastfm::similar_artists(http, api_key, seed_artist, MAX_SIMILAR).await?);
        artists.dedup();

        let mut bucket = Vec::new();
        for artist in artists.iter().take(MAX_SIMILAR + 1) {
            let tracks =
                lastfm::artist_top_tracks(http, api_key, artist, MAX_TRACKS_PER_ARTIST).await?;
            bucket.extend(tracks);
        }
        if !bucket.is_empty() {
            buckets.push(bucket);
        }
    }

    let entries = interleave(buckets, &known, wanted);
    Ok(if entries.is_empty() {
        Discovered::Nothing
    } else {
        Discovered::Ready(entries)
    })
}

/// The tracks an account has already scrobbled, keyed for case-insensitive
/// matching on artist and title.
async fn known_tracks(http: &Http, api_key: &str, user: &str) -> Result<HashSet<String>> {
    let tracks = lastfm::existing_tracks(http, api_key, user, MAX_KNOWN).await?;
    Ok(tracks
        .into_iter()
        .map(|track| exclusion_key(&track))
        .collect())
}

/// Draw candidates across the buckets, round-robin.
///
/// Each round takes one track from each bucket in turn, so a bucket that holds
/// many acceptable candidates cannot fill the result on its own. A candidate is
/// skipped when it is already scrobbled or already drawn, both matched
/// case-insensitively on artist and title.
fn interleave(buckets: Vec<Vec<Track>>, known: &HashSet<String>, count: usize) -> Vec<Entry> {
    let mut drawn: HashSet<String> = HashSet::new();
    let mut entries = Vec::with_capacity(count);
    let mut round = 0usize;
    let mut progressed = true;

    while entries.len() < count && progressed {
        progressed = false;
        for bucket in &buckets {
            if entries.len() >= count {
                break;
            }
            let Some(track) = bucket.get(round) else {
                continue;
            };
            progressed = true;
            let key = exclusion_key(track);
            if known.contains(&key) || !drawn.insert(key) {
                continue;
            }
            entries.push(entry(track));
        }
        round += 1;
    }

    entries
}

/// The key exclusion and deduplication match on: artist and title, folded to
/// one case so that Last.fm's spelling differences cannot let a repeat through.
fn exclusion_key(track: &Track) -> String {
    format!(
        "{}\t{}",
        track.artist.to_lowercase(),
        track.title.to_lowercase()
    )
}

/// One candidate, as a playlist entry. There is no MBID to carry: Last.fm
/// similarity data has none, and the sink matches on artist and title.
fn entry(track: &Track) -> Entry {
    Entry {
        recording_mbid: None,
        title: track.title.clone(),
        artist_credit: track.artist.clone(),
        album: None,
        duration_ms: None,
        score: None,
        latest_listened_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: &str, title: &str) -> Track {
        Track {
            artist: artist.to_string(),
            title: title.to_string(),
        }
    }

    #[test]
    fn the_exclusion_key_folds_case() {
        assert_eq!(
            exclusion_key(&track("Chappell Roan", "Good Luck, Babe!")),
            exclusion_key(&track("chappell roan", "good luck, babe!"))
        );
    }

    #[test]
    fn the_draw_spreads_across_buckets() {
        // The first bucket alone could fill the result; the draw must not let it.
        let buckets = vec![
            vec![track("A", "a1"), track("A", "a2"), track("A", "a3")],
            vec![track("B", "b1")],
        ];
        let entries = interleave(buckets, &HashSet::new(), 3);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            vec!["a1", "b1", "a2"]
        );
    }

    #[test]
    fn the_draw_skips_known_and_repeated_tracks() {
        let known: HashSet<String> = [exclusion_key(&track("A", "a1"))].into_iter().collect();
        let buckets = vec![
            vec![track("A", "a1"), track("A", "a2")],
            vec![track("A", "A2")],
        ];
        // The first bucket's "a1" is known, and its "a2" is the same track as
        // the second bucket's "A2" — so only the first spelling drawn survives.
        let entries = interleave(buckets, &known, 5);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.title.as_str())
                .collect::<Vec<_>>(),
            vec!["A2"]
        );
    }

    #[test]
    fn an_entry_carries_no_mbid() {
        let entry = entry(&track("Fletcher", "Bitter"));
        assert_eq!(entry.recording_mbid, None);
        assert_eq!(entry.artist_credit, "Fletcher");
        assert_eq!(entry.title, "Bitter");
    }

    #[test]
    fn the_no_listening_message_names_last_fm() {
        let message = no_listening("jesse1819");
        assert!(message.contains("Last.fm"), "{message}");
        assert!(message.contains("jesse1819"), "{message}");
    }
}
