# Proposal

## Why

ListenBrainz exposes a clean, token-authenticated, **on-demand** recommendation API — collaborative-filtering recordings, and LB Radio with its prompt language — but offers no way to carry those recommendations into wherever the user actually listens. The existing `spotify-qobuz` script proved the Qobuz side works end to end, but it is a pinned Python venv plus a headless browser, tied to Spotify's album-shaped Discover Weekly, and depends on scraping a scheduled playlist rather than invoking recommendations directly. A single small Rust binary that turns an on-demand recommendation request into a portable playlist document — and *optionally* materializes it into Qobuz — fits octx's arm model, deletes the runtime, and keeps any single destination from becoming a hard dependency.

## What Changes

- **New `jam` arm**, built as a source → sink pipeline:
  - `jam recs [--count N]` — collaborative-filtering recording recommendations for the authenticated ListenBrainz user. `--count` takes the endpoint's default of 25 and is clamped to its maximum of 1000.
  - `jam radio --prompt <prompt> --mode easy|medium|hard` — LB Radio, driven by ListenBrainz's prompt language (artist and tag prompts included).
  - `jam status` — report whether the arm could produce recommendations (credential present, model generated) and exit non-zero if it could not. Writes nothing, so it can guard a real run.
  - `jam scrobble [--source ndjson|lastfm|listenbrainz]` — read listens from an NDJSON log (`--input <file|->`), from a Last.fm account (`--user <name>`), or from a ListenBrainz account (`--user <name>`, defaulting to the account's ListenBrainz user), and deliver them to ListenBrainz, safely re-runnable from an external scheduler.
  - Shared pipeline: source → normalized document → selected sink, in both directions.
- **`jam` is bidirectional.** One arm reads ListenBrainz (recommendations, radio) and writes to it (listens). The read side has sources and sinks; the write side has an input and a sink. Normalizing to a document in both directions is what keeps a sink addable without touching a source.
- **Scrobbling is idempotent by construction, not by bookkeeping.** Each listen's identity is derived deterministically from the listen itself, and ListenBrainz discards a listen whose user, timestamp, and identity it already holds. The arm therefore keeps no offset and no state file, and re-reading an entire log every run is correct rather than merely safe.
- **An empty listen log is a no-op; an absent recommendation model is a failure.** The two look alike and mean opposite things: nothing to submit is normal for a scheduled run, whereas nothing to recommend means the input `jam` depends on does not exist yet.
- **Qobuz history reaches jam through Last.fm, not through Qobuz.** Qobuz exposes no listening-history API, but it publishes plays to Last.fm from its own servers once the account is linked, and Last.fm's documented `user.getRecentTracks` maps field for field onto a listen. The write side therefore takes a selected source rather than only a local file, and a ListenBrainz source covers migration and repair.
- **`recs` reads a model `jam` cannot create.** ListenBrainz trains its collaborative-filtering model from listens an external scrobbler submits, and exposes the latest result; when no model exists for the user it answers `204 No Content`. `jam` treats that as a distinct, actionable failure rather than an empty result.
- **An absent source is never a destructive write.** A run whose source has nothing to give exits non-zero before touching anything, and the Qobuz sink never converges on an empty document — so a missing or stale model cannot empty a user's playlist.
- **The output is selected explicitly; there is no default destination.** `--output` is required for the document-producing subcommands:
  - `--output json` — write the normalized playlist document to stdout. Requires no Qobuz credential, never contacts Qobuz, and is the mode a viewer or any other consumer uses.
  - `--output qobuz` — resolve entries against the Qobuz catalog and idempotently sync a target playlist; `--playlist` and `--dry-run` apply here.
- **Corrected upstream endpoints.** Normalization uses `POST https://listenbrainz.org/player/?recording_mbids=<csv>`; the previously planned `POST /1/player/` does not exist on the API host. LB Radio uses `GET /1/explore/lb-radio?prompt=&mode=`; the legacy `/1/lb-radio/artist|tags` routes are not targeted.
- **Credentials are one account document, and a run speaks for one account.** `jam` reads a single credential, host `jam`, whose value is a JSON document listing accounts — per person, the Last.fm user, the ListenBrainz user and token, and the Qobuz token — plus one shared Last.fm API key, and `--account` selects whose run this is. A household therefore schedules one invocation per person against one store, and the arm still fetches only the fields its source and sink need, fails fast with the field's path when one is absent, implements no credential storage, decryption, or environment handling, and never prints a value.
- **On-demand only.** No consumption of ListenBrainz `created_for` / scheduled playlists (Weekly Jams, Daily Jams). Scheduling, if wanted, is an external concern (cron/systemd/timer) wrapping a one-shot invocation.
- **Reuse an existing Qobuz Rust client** rather than hand-rolling the Qobuz API (catalog search, playlist write, rate-limit backoff) — provided it accepts a caller-supplied app id and does not download the web client bundle at runtime; otherwise a thin `reqwest` client for the four endpoints. The crate-vs-thin-client choice is recorded in `design.md`.
- **The arm is useful without Qobuz, and is built that way.** The Qobuz sink is one sink, not the pipeline, so the arm ships in two increments: first the Qobuz-free path (recommendations and listens in and out as documents), then the Qobuz sink as a purely additive layer. The heaviest dependency is spiked after the product already works, not before.
- **No enshittification surface.** No Spotify, no browser scraping, no background daemon, no proprietary sync middleware.

## Capabilities

### New Capabilities

- `jam-arm`: On-demand ListenBrainz recommendation sources (`recs`, `radio`) that produce a normalized playlist document and deliver it through a selected sink — `json` to stdout, or `qobuz` for an idempotent playlist sync — plus a `status` diagnostic that reports readiness without writing.
- `jam-scrobble`: The write direction — reading listens from a selected source (`ndjson`, `lastfm`, or `listenbrainz`) and delivering them to ListenBrainz with a stable per-listen identity, so the same source can be replayed from a scheduler without ever creating a duplicate listen.

### Modified Capabilities

<!-- None. Credential delivery is defined by the separate `credential-access` change and is not re-specified here. -->

## Impact

- **New**: workspace member `arms/jam/` (binary `jam`), its `skill.md` written to the `arm-skills` contract, and a `registry-index.json` entry.
- **Modified**: nothing in the head; `jam` depends on the `credential-access` change's shared credential library, and its `skill.md` depends on the `arm-skills` contract.
- **Dependencies**: `clap`, `anyhow`/`thiserror`, `tokio`, `reqwest`+`serde`, plus the chosen Qobuz client crate; no browser, no Python.
- **External**: each account's ListenBrainz token must carry the `listenbrainz:submit-listens` scope for `jam scrobble`, while reads need no scope — one token per account serves both, so it must hold both permissions. Last.fm's API key is granted to the application rather than to a person, so one key reads any public account's scrobbles and the account selects whose; reading a public ListenBrainz account needs no token at all, only the name. The Last.fm key lives in the same `jam` document as the accounts, under `lastfm.api_key`. The Last.fm API terms additionally require attribution on any copy of its data made public, so the `lastfm` source stamps the account's Last.fm page and the API terms into the listen document and prints the same credit on stderr; a consumer that republishes the document must keep it.
- **Unchanged**: `storage-sync`, `harness-arm`, and `agent-arm` behavior; credential delivery changes are owned by the `credential-access` change.
