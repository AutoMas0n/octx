# Proposal

## Why

`jam recs` depends on ListenBrainz's collaborative-filtering recommendations, which are produced by a daily batch job on MetaBrainz's infrastructure that we cannot trigger and that only covers users present in a dataset rebuilt from dumps. Both accounts this pipeline runs for are days old and have returned `204 No Content` throughout, so the only recommendation source the arm has delivers nothing.

Last.fm answers the same question immediately, from data it already holds, for any account with scrobbles — and the arm already turns a source into a Qobuz playlist. We are depending on the most fragile link in the chain for no benefit.

## What Changes

- **New `jam discover` source**: recommendations seeded from the account's own Last.fm listening — top artists, their similar artists, and those artists' top tracks, excluding what the account already scrobbles. A run MAY instead be given a seed artist, which fans out through Last.fm's similar-artist data rather than returning one artist's catalogue, or a prompt, which selects by Last.fm tag.
- **Date-stamped default playlist, for this source only**: `Last.fm Discovery YYYY-MM-DD` in UTC. Each run targets that day's playlist and creates it when missing; re-running on the same day converges on the same playlist. `jam recs` and `jam radio` keep their current fixed default names.
- **Retention**: after a successful delivery, older discovery playlists are pruned so that the most recent ten remain. Only playlists named by the discovery source are candidates, the run's own playlist is never removed, and a run can be configured to keep everything.
- **BREAKING**: the recording MBID becomes optional on a playlist entry. The Last.fm source yields no MBIDs, and the Qobuz sink already resolves entries by searching artist and title, so an MBID is provenance rather than a requirement. Existing sources continue to supply one.
- **The Last.fm source needs no ListenBrainz credential**: it requires the shared Last.fm API key and the selected account's Last.fm user, both already in the credential document.
- **Retire the redundant `jam scrobble` schedule**: ListenBrainz's own Last.fm connection imports history and periodically adds new scrobbles, so our submission loop duplicates it. The `jam scrobble` subcommand stays; only the schedule entry goes.

## Capabilities

### New Capabilities

None. The recommendation sources, the normalized document, and the playlist sinks are one capability, and this change adds to it rather than introducing a new behaviour area.

### Modified Capabilities

- `jam-arm`: adds a Last.fm-seeded recommendation source; makes an entry's recording MBID optional; and makes the default target playlist name source-dependent in shape — fixed for `recs` and `radio`, date-stamped for the new source.

## Impact

- **Code**: `arms/jam/src/cli.rs` (new subcommand and its seed, prompt, and retention options), `run.rs` (dispatch and credential selection), a new module for the Last.fm-seeded source, `lastfm.rs` (reuse of the existing client and its rate-limit handling), `document.rs` (optional `recording_mbid`), `sync.rs` (date-stamped default name and retention), `qobuz.rs` (listing and deleting playlists, for retention).
- **Unchanged**: `qobuz.rs`'s artist/title resolution and crash-safe convergence already handle entries with no MBID. Deletion is a new capability in that module, not a change to the existing one.
- **Credentials**: no new fields; the existing `lastfm.api_key` and `accounts.<name>.lastfm.user` are sufficient.
- **Deployment**: the scheduler box gains a daily `jam discover --output qobuz` line, and loses the `jam status && jam recs` guard and the `jam scrobble` line.

## Non-goals

- Removing the `jam recs` and `jam radio` sources, or the ListenBrainz credential. `recs` becomes usable if ListenBrainz ever catches up, and nothing here forecloses that.
- Making `jam status` aware of the new source. Last.fm has data whenever the account has scrobbles, so the new source needs no readiness guard.
- Resolving entries to MusicBrainz IDs through a second service.
