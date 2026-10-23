# Design

## Context

`jam discover` (`arms/jam/src/discover.rs`) is deterministic: seeds are the listener's all-time top artists, candidates are those artists' fixed "top tracks" rankings, and selection walks them in order. The only exclusion is the most recent 200 scrobbles (`MAX_KNOWN`). A track played further back, or already delivered by a previous discovery playlist, is not excluded. See proposal.md — Why.

Two facts constrain where memory may live:

- The release-owned mirror `{data_dir}/octx/storage/` is read-only and replaced wholesale by `octx sync`; the `storage-sync` spec states user-authored content belongs outside it. Memory must not go there.
- `octx`'s own `src/paths.rs` documents `data_dir()` as "for binaries, cache, and state", and already resolves `installed-manifest.json` under it. The `jam` arm resolves its own paths (its `Cargo.toml` currently has no `dirs` dependency, unlike `arms/harness`).

## Goals / Non-Goals

**Goals:**

- A persisted, per-listener memory of recently delivered discovery tracks, so daily runs rotate instead of repeating.
- A wider candidate pool and per-artist spread so the memory cannot starve a run and no artist dominates a playlist.
- No change to the `jam discover` CLI contract, sinks, or playlist-retention behavior.

**Non-Goals:**

- No memory of recommendations from the `recs` or `radio` sources; this is discovery-specific.
- No shared or cross-machine sync of the memory; it is local user state.
- No change to the scrobble exclusion's source or size, and no attempt to compare against previous Qobuz playlists directly.
- No second-order similarity expansion (`track.getSimilar` or similar-of-similar); the widening stays within the existing `artist.getSimilar` → `artist.getTopTracks` reads.

## Decisions

### Memory location: `{data_dir}/octx/jam/discovery-state.json`

Resolve the directory as `data_dir()/jam`, following the per-purpose convention already used for `bin/` and `skills/`, and keep the document name specific to discovery. This places the memory under the documented state root and beside `installed-manifest.json` precedent, and it survives `octx sync` because it is not under `storage/`.

The `jam` arm resolves this itself: add `dirs.workspace = true` to `arms/jam/Cargo.toml` and mirror the resolution `arms/harness/src/resolve.rs` already performs. *Alternative considered:* expose a `data_dir()` from `octx-creds`, which the arm already depends on — rejected because that crate owns credentials, not general state, and the `dirs` crate is already a workspace dependency used directly by another arm.

Add an `OCTX_JAM_STATE_DIR` environment override in `config.rs`, mirroring the existing `OCTX_JAM_STORE_DIR` test hook, so integration tests point the memory at a scratch directory. Resolution order: the override when set, else `{data_dir}/octx/jam`.

### Memory is keyed by listener, not account name

The document is keyed by the Last.fm user the run resolves (the selected account's `lastfm.user`, or `--user` when given). Recommendations are derived from that listener's listening, so the memory belongs to the listener. Keying by account name would split the memory when the same person is reached under two account entries. The key is the same identity used to seed and to read scrobbles, so `--user` stays coherent.

### Memory document shape

A small versioned JSON document:

```json
{
  "version": 1,
  "listeners": {
    "jesse1819": [
      { "artist": "Chappell Roan", "title": "Good Luck, Babe!", "served_at": "2026-10-19" }
    ]
  }
}
```

Identity matches the existing exclusion key — artist and title, case-folded — so the source and the memory agree on what "the same track" means. `served_at` is recorded for diagnostics and future age-based pruning; the window itself is count-based (below). The document is written atomically (write to a sibling temp file, then rename) so a crash cannot leave a half-written memory. An unreadable, unparsable, or wrong-version document is treated as empty and replaced on the next successful write; a memory read or write failure never fails the run — the playlist has already been delivered.

### Bounded window: the most recent 200 delivered tracks per listener

Keep the newest 200 records and drop the oldest. A count window needs no clock and is deterministic in tests. At the default 25 tracks per run this rotates roughly every eight runs; with the widened pool (below) a run still has candidates to draw from. *Alternative considered:* an age-based window — more intuitive for "recent", but needs an injected clock to test and can silently empty when runs are irregular. The `served_at` field leaves the door open to move to age-based later without a document-shape change.

### Memory is read once and applied as one exclusion set

`run.rs` loads the remembered set before the run and hands the source the remembered keys; the source unions them with the scrobble exclusion and filters every candidate through the one set. This keeps the existing "read the listener's data once per run" property and keeps all network reads inside the source. The exclusion-key helper currently private to `discover.rs` is exposed (or shared) so the memory and the source cannot drift on identity.

### Memory records only a materialized Qobuz playlist

Recording happens in the `Discover` path after a successful, non-dry-run Qobuz sync — the same place discovery retention already runs. A `--dry-run` run, a failed sync, and a `--output json` run record nothing. *Alternative considered:* record on any sink, including `json`. Rejected because a `json` run is inspection or a pipeline step whose consumption the arm cannot observe; silently consuming a recommendation for it would surprise users and make repeated previews unstable. This is noted as a deliberate limitation rather than an oversight.

### Widening: more artists per seed, more tracks per artist, per-artist spread

Raise the read limits in `discover.rs`:

| Constant | Now | After | Why |
| --- | --- | --- | --- |
| `MAX_SIMILAR` | 5 | 10 | more neighbourhoods per seed |
| `MAX_TRACKS_PER_ARTIST` | 3 | 10 | each artist offers fallback tracks for later rounds |
| `MAX_SEEDS` | 5 | 5 | top artists are stable; more seeds add calls without much reach |

The candidate pool grows from about `5 × 6 × 3 ≈ 90` to about `5 × 11 × 10 ≈ 550` before track-level dedup — enough headroom over the 200-track window plus 200 scrobbles. The request cost stays bounded: about 62 Last.fm calls per run (1 top-artists, 5 similar-artist, 55 top-track reads, 1 recent), roughly 16s at the arm's 250 ms spacing — acceptable for a daily run and well inside Last.fm's tolerance.

Selection changes from round-robin across seed *buckets* to round-robin across the individual *artists* within each seed. Today a seed's bucket is its artists' tracks concatenated, so the first three rounds draw three tracks by the seed artist alone; the fix makes each artist its own bucket and draws one track from each artist before any artist's second. This both satisfies the artist-spread requirement and lets later days advance to artists the first days never reached.

## Risks / Trade-offs

- **Candidate starvation after the window fills.** A small Last.fm catalogue could leave too few unexcluded candidates. → The widened limits plus "return what you have" behaviour; if it proves insufficient the window size is a single tunable constant.
- **More Last.fm calls.** ~62 requests per run, up from ~37. → Still bounded and rate-spaced; the increase is the cost of the wider pool. A future second-order expansion is explicitly out of scope for this reason.
- **Concurrent runs race on the memory file.** A scheduler run and a manual run can interleave, and the last writer wins. → Writes are atomic (temp file + rename) so the file is never corrupt; a lost record only weakens one run's exclusion. A lock is not warranted for a once-a-day job.
- **Memory persists after a user deletes their playlists.** The memory still excludes tracks the user may want back. → The document's location is documented in help so it can be removed; `served_at` is retained for future tooling.
- **Listener identity when `--user` differs from the account.** → Keying by the resolved Last.fm user keeps the memory aligned with the seed and scrobble reads, so the same user name always maps to the same memory.
