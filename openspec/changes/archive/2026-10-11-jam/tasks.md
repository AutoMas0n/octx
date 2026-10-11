# Tasks

## Execution order

Task numbers group work by concern, not by build order. Build in two increments, so the heaviest and riskiest dependency comes last:

- **Increment 1 — no Qobuz dependency:** 1, 2.1–2.2, 3, 4, 5.1, 7.2–7.4, 8.2, 8.4, 8.6–8.8, 9, 10. This already ships a complete arm: recommendations and listens in and out as documents, with the whole write side working, and more than one account servable from one store.
- **Increment 2 — the Qobuz sink:** 2.3–2.4 (the crate spike and the dependency it chooses), 5.2, 6, 7.1, 8.1, 8.3, 8.5. Everything here is additive; nothing in increment 1 changes.

Start the spike (2.3) only once increment 1 is done, and let it decide the dependency rather than the dependency deciding the plan.

## 1. Credential acquisition

- [x] 1.1 Define and parse the credential document the arm expects at host `jam`: a required `version`, an optional `default`, the shared `lastfm.api_key`, and an `accounts` map whose entries carry `lastfm.user`, `listenbrainz.user`/`token`, and `qobuz.user_auth_token` with an optional `app_id`. Load it through `octx-creds`, and only when the selected source and sink need a field from it. Verify `cargo check -p octx-jam` plus tests over a two-account fixture, including one run that touches the store and one public read (explicit `--user`, `--output json`) that does not.
- [x] 1.2 Implement `--account <name>` selection: the named account, else the document's `default`, else the sole account, with several accounts and no default failing while naming them, and an unknown name failing while listing the accounts present. Verify a test per case, asserting no request is made on the failing paths.
- [x] 1.3 Fail fast naming the JSON path when a field a needed source or sink requires is absent (`accounts.wife.listenbrainz.token`), naming host `jam` and the `octx creds add jam` command, before any authenticated request. Verify tests for the absent document, a malformed document, and an absent field, asserting no request is made and no part of the stored value is echoed.
- [x] 1.4 Ensure no credential value is ever printed. Verify a test exercising both the success and failure paths and asserting the token substring appears in neither stdout nor stderr.


## 2. Arm scaffold and output selection

- [x] 2.1 Create the `arms/jam` workspace member with a clap skeleton exposing the `recs`, `radio`, `scrobble`, and `status` subcommands, a required `--output <json|qobuz|listenbrainz>`, a required `--input` for `scrobble`, a global `--account <name>`, and `--help`/`--version`. Verify `cargo build -p octx-jam` and `jam --help` list every subcommand and both sink families.
- [x] 2.2 Enforce explicit output selection for the document-producing subcommands: a missing `--output` and an unknown sink each exit non-zero before any upstream request, a sink invalid for the chosen subcommand (for example `jam recs --output listenbrainz`) is rejected, and `status` is accepted without `--output`. Verify with tests asserting the usage text and that no HTTP client call occurs.
- [x] 2.3 Spike the `qobuz-api` crate: confirm it compiles in the arm and exposes playlist create / track add / track remove and catalog track search, and that it accepts a caller-supplied app id rather than fetching the web client bundle. Record whether an ordered convergence can be expressed (a reorder call, or one update carrying the full ordered track list). Measure the stripped release binary size against the arm budget and record the adopt-or-fallback outcome in `design.md`. Verify the spike binary builds and the recorded decision is present.
- [x] 2.4 Add the chosen Qobuz dependency (crate, or `reqwest` for the thin-client fallback) to `arms/jam/Cargo.toml`. Verify `cargo check -p octx-jam` succeeds with the dependency in place.

## 3. ListenBrainz sources

- [x] 3.1 Implement the `recs` source against the collaborative-filtering recording endpoint with an optional count (default 25, clamped to the endpoint's 1000 maximum), taking the user name from the selected account's `listenbrainz.user` or deriving it from that account's token (`/1/validate-token`) rather than requiring it on the command line. Verify with a mock server asserting the requested path/params, the derived user name, the clamped count, and the returned MBID set.
- [x] 3.2 Implement the `radio` source against `GET /1/explore/lb-radio` with a required `--prompt` and `--mode`. Verify with a mock server asserting prompt and mode are forwarded and the returned JSPF is parsed.
- [x] 3.3 Reject `radio` invocations missing `--prompt` or `--mode`, exiting non-zero and naming the missing argument without making a request. Verify with tests.
- [x] 3.4 Implement `jam status`: report credential presence, the derived user name, and model readiness (the recommendation endpoint's `last_updated` and `total_mbid_count`), requiring only the ListenBrainz credential, writing nothing, and exiting 0 only when `recs` could succeed. Verify with tests for the ready, 204, 404, and missing-credential cases.

## 4. Normalization and throttling

- [x] 4.1 Implement the player-endpoint normalizer: `POST https://listenbrainz.org/player/` with `recording_mbids`, chunked at 50, parsed from the double-wrapped JSPF into entries carrying the recording MBID (from `identifier`), title, artist credit, album, and duration. Verify with tests at 49, 50, and 51 MBIDs asserting the number of requests and per-request chunk sizes, plus a fixture test pinning the response nesting so an upstream shape change fails loudly.
- [x] 4.2 Map the `radio` JSPF into the same entry shape, tolerating its extra nesting under `payload.jspf` — one mapper with one extra unwrap, not a second code path. Verify with a fixture test.
- [x] 4.3 Implement a shared request throttle enforcing at most one ListenBrainz request per second. Verify by asserting the observed spacing between requests the mock server receives, rather than introducing a clock abstraction.

## 5. Output sinks

- [x] 5.1 Implement the `json` sink: serialize the playlist document to stdout. Verify with a test asserting stdout parses as JSON, that entries carry the recording MBID, title, and artist credit, and that no Qobuz client call is made.
- [x] 5.2 Implement the Qobuz resolver: search on artist + title, accept the best hit, skip when none, and carry MBID/album/duration on the entry. Verify with tests for a match, a miss (skipped and reported, run continues), and carried fields surviving.

## 6. Playlist sync and resilience

- [x] 6.1 Implement target resolution from `--playlist` (a name or an id): resolve a name through the user's playlist list on every run, create it when it does not exist, and fall back to the default name registered for the selected source when no target is given, which no two sources share. Verify with tests for existing-name, missing-name, id, and no-target inputs.
- [x] 6.2 Implement diff-and-converge against the current playlist contents, leaving the playlist in the source's order and applying additions before removals. Verify idempotency with a test where a second identical sync performs no adds or removes, a removal test when an entry disappears upstream, an order test where the converged playlist matches the source's ranking, and a test asserting the observed mutation order is adds-then-removes.
- [x] 6.3 Implement `--dry-run` reporting intended adds/removes without writing. Verify with a test asserting no mutating call reaches the Qobuz client.
- [x] 6.4 Implement 429 backoff and partial-failure tolerance: an HTTP 429 is retried, and an unresolved or failed entry does not abort the remaining entries. Verify with tests that inject a 429 response and a failing search.
- [x] 6.5 Treat an absent source as a failure, not as an empty playlist: when the recommendation source answers 204 (no model) or otherwise yields no MBIDs because nothing exists, exit non-zero before any write with an actionable message, and never converge. Verify with a test asserting no mutating Qobuz call occurs and the target playlist is unchanged.

## 7. Reporting, skill, and distribution

- [x] 7.1 Report the run summary (resolved/unresolved/added/removed) to stderr for the Qobuz sink. Verify with a test asserting the reported counts.
- [x] 7.2 Write `arms/jam/skill.md` conforming to the `arm-skills` contract (Description, Usage, Options, Examples, Credentials, Prerequisites, Failure modes), documenting all four subcommands, all three sinks, the single credential host `jam` with the shape of the account document it must hold, that `--account` picks one, that the target playlist is jam-owned (hand-added or hand-reordered tracks are removed on the next run) and that the per-source default names do not collide, and the prerequisites — a scrobbling account for `recs` to have anything to recommend. Verify it names every subcommand, every sink, host `jam`, `--account`, the jam-owned warning, and both prerequisites.
- [x] 7.3 Add the `jam` entry to `registry-index.json` (description, repository, skill URL, downloads) and confirm the release workflow includes the arm binary and skill. Verify the entry resolves to a downloadable asset and the skill URL is present.
- [x] 7.4 Document usage and the external-scheduling note (invoke via cron/systemd timer/`termux-job-scheduler`; no in-arm scheduler), including one command per account so several people are scheduled independently against one store, one target playlist per scheduled line (the defaults differ per source), a `jam status` guard so a scheduler skips rather than fails when no model exists yet, and `--dry-run` for the first run. Verify each documented command runs as written.

## 8. Integration verification

- [x] 8.1 Exercise the full pipeline against mocked ListenBrainz and Qobuz endpoints: `recs` → player normalizer → `json` sink, and `recs` → resolver → diff → write for the Qobuz sink. Verify the JSON document is produced, a playlist is created/populated on the first run, left in the source's order, and unchanged on the second.
- [x] 8.2 Verify the Qobuz-free path: with only the ListenBrainz credential available, `jam radio --prompt <p> --mode easy --output json` completes and makes no Qobuz call and requests no Qobuz credential.
- [x] 8.3 Opt-in live smoke test with real credentials: run `jam recs --count 5 --output json` and `jam radio --prompt "artist:(queen)" --mode easy --output json` and verify each emits a document without mutating any playlist.
- [x] 8.4 Regression-check the head: existing arms still dispatch and propagate their exit codes. Verify by running an existing arm (e.g. `octx x fmt --help`).
- [x] 8.5 Verify the empty-source path end-to-end against a mocked 204: `jam recs --output qobuz --playlist <p>` exits non-zero, prints guidance, and leaves the playlist untouched, while `jam recs --output json` writes no document.
- [x] 8.6 Exercise the write direction end-to-end against a mocked ListenBrainz submit endpoint: `jam scrobble --input <fixture> --output listenbrainz` submits the listens with deterministic identities, and a second identical run derives the same identities, so the endpoint observes no new listens.
- [x] 8.7 Verify the scrobble no-network path: `jam scrobble --input <fixture> --output json` emits the listen document without contacting any ListenBrainz write endpoint.
- [x] 8.8 Verify two accounts in one document run independently: `jam recs --account jesse --output json` and `jam recs --account wife --output json` each read their own ListenBrainz user, and `jam scrobble --source lastfm --account wife --output listenbrainz` submits with her token and never his. Verify with a mock server asserting the distinct users and tokens observed per run, and that an unknown or ambiguous account fails before any request.

## 9. Scrobble (listen ingestion)

- [x] 9.1 Implement the NDJSON input reader: accept a path or `-` for standard input, parse one listen per line with required `listened_at`, `artist`, and `title` and optional `album` and duration, ignore blank lines, and build the normalized listen document. Verify with tests for a file, standard input, blank lines, optional fields, and order preservation.
- [x] 9.2 Derive each listen's `recording_msid` deterministically from the user, timestamp, artist, and title, and place it at `track_metadata.additional_info.recording_msid`. Verify with tests asserting stability across runs and distinctness across differing listens.
- [x] 9.3 Implement the `json` sink for listens: serialize the listen document to stdout without contacting ListenBrainz. Verify with a test asserting stdout parses as JSON and no write endpoint is called.
- [x] 9.4 Implement the `listenbrainz` sink: submit with `listen_type=import` for a batch and the single-listen form for exactly one listen, reusing the shared throttle. Verify with a mock server asserting the request shape, the batching choice, and the request spacing.
- [x] 9.5 Report malformed input with its line number without discarding valid lines, and exit non-zero when any line was rejected; exit zero when the input yields no listens. Verify with tests for one bad line among good ones, all lines bad, and empty input.
- [x] 9.6 Fail fast when the selected account carries no `listenbrainz.token`, and report a submit-permission rejection distinctly from an unknown credential. Verify with tests for both the missing-token and insufficient-scope cases.
- [x] 9.7 Report submitted and rejected counts on stderr, without claiming a count of newly created listens. Verify with a test asserting the summary contains no deduplicated count.

## 10. Networked listen sources

- [x] 10.1 Add `--source <ndjson|lastfm|listenbrainz>` to `jam scrobble`, defaulting to `ndjson`, make `--input` required for that source, and reject `--input` for the networked sources. Verify with tests for the default, the missing-input case, and the rejected-input case.
- [x] 10.2 Implement the Last.fm source: fetch `user.getRecentTracks` with the shared `lastfm.api_key` from the credential document and the user from `--user` or the account's `lastfm.user`, map `date.uts`/`artist.#text`/`name`/`album.#text` onto a listen, request no `extended` parameter (it re-shapes `artist` and `album` to a `name` key), omit the entry that carries no timestamp, and page within the rate budget. Verify with a mock server asserting the request shape, the mapping, the now-playing omission, and paging.
- [x] 10.3 Fail fast before any request when the Last.fm key is absent, naming the missing field and `octx creds add jam`, and surface Last.fm's own API errors. Verify with tests for both the missing-key and error-response cases.
- [x] 10.4 Implement the ListenBrainz source: fetch `GET /1/user/{user}/listens` bounded by `--limit`, map `artist_name`/`track_name`/`release_name`, and preserve the returned `recording_msid`. Verify with a mock server asserting the request and the mapping.
- [x] 10.5 Resolve the ListenBrainz account name from `--user`, else the account's `listenbrainz.user`, else that account's token, and fail fast naming what is missing when neither is available. Verify with tests for the fallback and the failure.
- [x] 10.6 Generalize listen identity: preserve an upstream `recording_msid` when the source supplies one, and derive the deterministic identity otherwise. Verify with tests asserting the preserved identity on re-delivery and the derived identity for NDJSON.
- [x] 10.7 Extend the empty-source rule to the networked sources: a fetch that returns no listens reports nothing to do and exits zero. Verify with a mock returning an empty list.
- [x] 10.8 Integration: each source delivers to `--output json` with the same document shape as NDJSON and to `--output listenbrainz` through a mocked submit endpoint, and a second identical run is observed as no new listens. Verify end to end.
- [x] 10.9 Add the `lastfm` credential host, the new sources, and the attribution obligation (a copy of the document that is made public must keep the credit) to `arms/jam/skill.md` and to the impact notes, and verify `openspec validate jam --strict` passes.
- [x] 10.10 Emit the Last.fm attribution the API terms require: stamp a top-level credit (name, the account's Last.fm page, the API terms) into the document whenever the `lastfm` source ran, print the same credit on stderr, and add none for the other sources. Verify with tests asserting the stamped document, the stderr line, and the absent credit for `ndjson` and `listenbrainz`.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
