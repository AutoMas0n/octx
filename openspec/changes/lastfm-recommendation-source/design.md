# Design

## Context

See `proposal.md` — Why. The current state that shapes the approach:

- The arm already has two recommendation sources, `recs` and `radio`, both of which read ListenBrainz and yield **recording MBIDs**. A shared expander turns MBIDs into entries by calling the ListenBrainz player endpoint in chunks of 50.
- `document.rs` holds one normalized `PlaylistDocument` with a `Provenance` block and `Entry` values; every sink consumes that shape and none of them knows which source produced it.
- `qobuz.rs` resolves each entry to a Qobuz track by **searching the catalog on artist and title**. It never reads the entry's recording MBID. Convergence is a diff against the playlist's current contents, additions applied before removals.
- `sync.rs` resolves the target on every run: a name is looked up among the account's owned playlists and **created when missing**. Nothing in the arm can rename a playlist.
- `default_playlist(source)` returns a fixed `&'static str` per source, and a unit test asserts `recs` and `radio` differ.
- `lastfm.rs` already holds a Last.fm client: URL escaping, `PAGE_SIZE`, `MAX_PAGES`, optional-field handling, and the `scrobble` path's use of `user.getRecentTracks`.
- The credential document already carries a shared `lastfm.api_key` and `accounts.<name>.lastfm.user`.

## Goals / Non-Goals

**Goals:**

- A recommendation source that answers from data Last.fm holds, for any account with scrobbles, with no dependency on ListenBrainz's batch pipeline.
- Each day's result is an identifiable, self-contained playlist; a re-run on the same day converges instead of duplicating.

**Non-Goals:**

- Resolving entries to MusicBrainz IDs through a second service.
- Changing the `recs` or `radio` default names, or removing either source.
- Teaching `jam status` about the new source; the new source has a readiness signal only when the account has no scrobbles, which the existing empty-source behaviour already covers.

## Decisions

### The subcommand is `jam discover`

The existing subcommands are named for the outcome (`recs`, `radio`), not the provider. `discover` continues that and matches the playlist it produces. Rejected alternatives: `jam lastfm`, because `--source lastfm` already means "read listens from Last.fm" in the scrobble direction and would read as the same knob; and `jam recs --source lastfm`, because `recs` is defined in the spec as the collaborative-filtering endpoint and overloading it would leave two different behaviours under one name.

### An entry's recording MBID becomes optional, rather than resolving one

Last.fm's similarity API yields artist and track names and no identifiers. The only consumer that could need one — Qobuz resolution — already ignores it and matches on artist and title, and the JSON sink is for inspection. Making the field optional keeps the source free of any second service. Rejected alternatives: resolving through the ListenBrainz metadata endpoint, which re-couples the source to the service this change exists to step away from and silently drops entries that fail to match; and querying MusicBrainz directly, which adds a rate-limited dependency to populate a field nothing consumes.

The cost is real and accepted: discovery entries carry no stable identifier, so deduplication inside the source happens on case-insensitive artist and title, and a JSON consumer cannot key on an MBID for these entries.

### The dated default is a name, not a rename

`default_playlist` becomes source-aware in shape: `recs` and `radio` keep their fixed names — `ListenBrainz Discovery` and `ListenBrainz Radio` — and the discovery source produces `Last.fm Discovery YYYY-MM-DD` in UTC. The discovery name is deliberately not `ListenBrainz`-branded, because this source does not read ListenBrainz and the two `Discovery` playlists must not be mistaken for each other. This falls straight out of the existing "look the name up, create it when missing" logic, so a same-day re-run resolves to the same playlist and converges. Rejected alternative: one playlist renamed on every run, which would need Qobuz `playlist/update` (not implemented anywhere in the arm) and would discard the per-run artifact the change is for.

The date is read once at the start of a run and reused for the whole run, so a run that straddles midnight UTC still targets exactly one playlist. An explicit `--playlist` is used verbatim and is unaffected.

### Seeds are interleaved, and the account's own listening is excluded

The source reads the account's top artists, expands each into similar artists, and takes those artists' popular tracks, skipping anything the account has already scrobbled. Candidates are drawn across seeds rather than exhausting one seed before the next: an early prototype that walked seeds in order returned twelve tracks by one artist, because the first seed alone filled the result. The exclusion set is read once per run, not per candidate.

### The Last.fm credential is reused as-is

The source needs the shared `lastfm.api_key` and the selected account's `lastfm.user`; both are already in the credential document, so no new fields, no new credential path, and the existing account-selection behaviour applies unchanged.

### A seed artist is a fan-out; a prompt is a tag

A given seed replaces the account's own top artists rather than adding to them, so a run means one thing at a time.

A seed artist expands through similar artists, which was verified rather than assumed: `artist.getSimilar` for one artist returns five distinct artists. A seed artist is therefore a starting point, not a single-artist catalogue dump, which is what made it worth including.

A prompt is matched against Last.fm **tags**, not free text. Last.fm has no free-text prompt: its search endpoints match literal names, so a prompt like `chillwave` would return tracks literally called "chillwave" rather than music of that character. `tag.getTopTracks` was verified to return real tracks for a tag. Seeding by tag must not read the account's own top tags, because those are empty for a new account — verified against a two-day-old account — so a usable seed would otherwise be silently unavailable.

### Retention candidates are identified by name alone

Pruning considers only playlists whose name matches the discovery pattern exactly. The name is the only provenance the arm has, and it is sufficient: it encodes both the source and the run's date. Rejected alternative: persisting created playlist identifiers in a state file, which adds a second source of truth to keep in sync with the account and would still be wrong if the state were lost.

The cost is accepted and bounded: a playlist a person created by hand that happens to match the pattern could be pruned. The pattern is exact, the survivors are always the newest, and the behaviour can be switched off.

Pruning runs only after a delivery succeeds, so a failed run leaves extra playlists rather than removing the one it just wrote. This matches the ordering the sink already uses for track additions and removals.

## Risks / Trade-offs

- **[A young account produces narrow picks]** → Mitigation: none available; the alternative is no result at all. Picks widen as listening accumulates. This is why the result is spread across seeds.
- **[Unbounded playlist growth on the account]** → Mitigation: names are deterministic and deletable, and each is self-describing. Pruning is deferred (see Open Questions).
- **[More upstream calls than the LB sources make]** → Mitigation: bound both the seed count and the per-seed fan-out, reuse the existing client and its page limits, and let the existing rate-limit reporting handle Last.fm's error. A run must not be permitted to loop unboundedly over seeds.
- **[The existing `defaults_differ_per_source` test and `default_playlist` signature both change]** → Mitigation: the test is extended to cover the discovery default, and the signature change is a compile-time break the compiler will surface at every call site.
- **[A date-stamped default could surprise a scheduled line that also passes `--playlist`]** → Mitigation: an explicit target always wins and is never date-modified; this is covered by a scenario.
- **[Deletion is destructive and unrecoverable]** → Mitigation: only exact pattern matches are candidates, the run's own playlist is never removed, pruning happens only after a successful delivery, the default retains ten, and a run can be configured to keep everything.
- **[A hand-made playlist matching the pattern could be pruned]** → Mitigation: the pattern is exact and date-shaped rather than a prefix match, and the opt-out removes the exposure entirely.

## Migration Plan

1. Ship the code with `discover` available; nothing existing changes behaviour.
2. Add a daily `jam discover --output qobuz` entry to the scheduler's crontab.
3. Confirm ListenBrainz's own Last.fm connection is delivering listens on its own (listen counts keep rising while our submission line is disabled), then remove that line.
4. Retire the `jam status && jam recs` line once `discover` has produced a playlist; `recs` remains available by hand.

Rollback: remove the added crontab line. The subcommand is additive, and no existing source or default name changes, so no data migration is required.
