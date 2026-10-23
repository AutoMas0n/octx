# Proposal

## Why

`jam discover` rebuilds its daily Qobuz playlist from Last.fm's similarity data with no memory between runs. Its seeds are the account's all-time top artists and its candidates come from those artists' fixed "top tracks" rankings, so each day's run reads the same stable pool and — being deterministic — draws the same stable selection. Exclusion covers only the last 200 scrobbles, so a track played further back, or already delivered by a previous discovery playlist, returns. The result is a daily playlist that repeats itself in the same or similar tracks.

## What Changes

- **Cross-run memory.** A discovery run remembers the tracks it delivered and excludes them from later runs while they remain inside a bounded recent window, so day-to-day output rotates instead of repeating. A run's own candidate count is still met from a widened pool.
- **State persisted per listener.** The memory lives in a small versioned JSON document under the octx data directory, keyed by the Last.fm user the run speaks as. It is written only after a successful, non-dry-run delivery to the Qobuz sink, so rehearsal and `--output json` inspection never consume a recommendation.
- **A wider, artist-spread candidate pool.** The discovery source reads more similar artists per seed and more tracks per artist, and draws round-robin across *artists* rather than seed buckets. This both supplies enough candidates to survive the exclusion set and stops one artist's tracks from filling a playlist.
- **No change to the CLI contract.** `jam discover` keeps its subcommand, flags, defaults, and exit behavior. `--keep` still governs only playlist pruning. The memory's location is reported in `--help`/docs so it can be inspected or removed by hand.

## Capabilities

### New Capabilities

None. This changes existing behavior owned by `jam-arm`.

### Modified Capabilities

- `jam-arm`: the Last.fm-seeded discovery source gains a persisted, bounded cross-run exclusion memory and a widened, artist-spread candidate selection. Its existing exclusion requirement (the account's own scrobbles) is extended rather than replaced.

## Impact

- `arms/jam/src/discover.rs` — exclusion set union with remembered tracks; wider read limits; round-robin across artists.
- New discovery-state module (for example `arms/jam/src/state.rs`) — read, merge, retain, and write the state document.
- `arms/jam/src/config.rs` — a resolved state directory with an `OCTX_JAM_STATE_DIR` test hook, matching the existing `OCTX_JAM_STORE_DIR` pattern.
- `arms/jam/Cargo.toml` — add `dirs.workspace = true` so the arm resolves `{data_dir}/octx/jam` itself.
- `arms/jam/src/run.rs` — thread the resolved listener identity into the source and record delivered entries after a successful Qobuz delivery; not on a dry run.
- Tests: `arms/jam/tests/discover.rs` and `arms/jam/tests/discover_cli.rs`.
