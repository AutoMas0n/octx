# Tasks

## 1. Make recording MBIDs optional in the document

- [x] 1.1 Make the entry's recording MBID optional in `arms/jam/src/document.rs` and update every construction and read site so the workspace compiles — verify `cargo check` succeeds and `cargo clippy -- -D warnings` is clean.
- [x] 1.2 Extend the document and JSON-sink tests to cover an entry that carries no MBID, alongside the existing entries that do — verify `cargo test` passes and the new case is exercised.
- [x] 1.3 Confirm the Qobuz sink needs nothing for these entries by adding a resolution test for an entry with no MBID, matched on artist and title — verify the test passes and the resolved set contains the expected track.

## 2. Last.fm read endpoints

- [x] 2.1 Add client methods for the account's top artists, an artist's similar artists, an artist's top tracks, and the account's existing tracks, reusing the existing escaping, paging, and optional-field handling in `arms/jam/src/lastfm.rs` — verify a unit test parses a representative payload for each method.
- [x] 2.2 Handle Last.fm returning a lone result as an object instead of a one-element array, with a fixture test for the single-item shape of each list — verify the tests fail when the one-element case is treated as an array.
- [x] 2.3 Propagate Last.fm error and rate-limit answers through the existing reporting path rather than a new one — verify a test asserting a non-success answer produces an error naming Last.fm and writes nothing.

## 3. The discovery source

- [x] 3.1 Build seed expansion from top artists through similar artists to top tracks, bounded by an explicit cap on seeds and on per-seed fan-out — verify a test where the input exceeds the cap stays within it and makes no unbounded calls.
- [x] 3.2 Draw candidates across seeds rather than exhausting one seed first — verify a test where the first seed alone could fill the result still returns entries from more than one seed.
- [x] 3.3 Exclude tracks the account has already scrobbled, matched case-insensitively on artist and title, using a set read once per run — verify a test asserting an already-scrobbled candidate is absent and that the known-track reads happen once.
- [x] 3.4 Treat an account with no scrobbles as a source with nothing to give — verify a test asserting no document is produced, the message names Last.fm, and no sink is invoked.
- [x] 3.5 Accept a seed artist, fanning out through similar artists instead of returning that artist alone, and replacing the account's own artists as seeds — verify a test where one seed yields entries from more than one similar artist.
- [x] 3.6 Accept a prompt resolved through Last.fm tags, reading the tag rather than the account's own tags — verify a tag-seeded run returns entries for that tag, and that an account with no top tags still produces a result.

## 4. CLI, orchestration, and the dated default

- [x] 4.1 Add the `discover` subcommand with count, output, playlist, account, seed-artist, prompt, and keep options in `arms/jam/src/cli.rs` — verify `--help` lists the subcommand and its options and the existing usage tests still pass.
- [x] 4.2 Wire the source in `arms/jam/src/run.rs` so it reads the shared Last.fm API key and the selected account's Last.fm user only — verify a test asserting the run completes with no ListenBrainz credential present and never requests one.
- [x] 4.3 Make the default playlist name source-aware in shape in `arms/jam/src/sync.rs`, date-stamped in UTC for the discovery source — verify unit tests for the produced name, that the `recs` and `radio` defaults are unchanged, and that an explicit `--playlist` is used verbatim with no date added.
- [x] 4.4 Compute the run's date once and pass it down, so a run spanning midnight UTC still targets one playlist — verify a test deriving the name from a single supplied date rather than a repeated lookup.

## 5. Discovery playlist retention

- [x] 5.1 Add playlist listing and deletion to the Qobuz client, kept separate from the existing track-diff path — verify tests covering a successful delete and an error answer, and that existing sync tests are unchanged.
- [x] 5.2 Prune to the configured limit after a successful delivery, selecting candidates only by an exact match on the discovery naming pattern — verify tests asserting the newest ten survive, that other playlists including the `recs` and `radio` defaults are untouched, and that the run's own playlist is never removed.
- [x] 5.3 Add the keep-everything option and skip pruning entirely when it is set — verify a test asserting no delete is attempted.
- [x] 5.4 Ensure pruning never runs when delivery fails — verify a test asserting no delete is attempted on a failed delivery.

## 6. Skill and help documentation

- [x] 6.1 Document `discover` in `arms/jam/skill.md` under the existing Usage, Options, Examples, and Failure modes sections, including its seed, prompt, and retention options, keeping the seven required sections in their fixed order — verify the `arm-skills` conformance test passes.
- [x] 6.2 Confirm the subcommand's help text is self-documenting enough to stand alone as its documentation — verify the `--help` output names what seeds the recommendations, what the default playlist is, which credential it needs, and that pruning is on by default.

## 7. Integration and deployment

- [ ] 7.1 Run `jam discover --output qobuz` end to end on the scheduler box against real credentials — verify a playlist named with the run's UTC date exists with resolved tracks, and that a second run on the same day changes nothing.
- [ ] 7.2 Verify retention against the live account by running discovery enough times to exceed the limit — verify that only the newest ten remain, that no other playlist is removed, and that the keep-everything option removes nothing.
- [ ] 7.3 Add the daily `jam discover --output qobuz` line to the scheduler's crontab — verify `crontab -l` shows it and a manual invocation reproduces 7.1's result.
- [ ] 7.4 Confirm ListenBrainz's own Last.fm connection is delivering scrobbles by observing listen counts rise while the submission line is disabled, then remove that line — verify `crontab -l` and a rising listen count with no submission job.
- [ ] 7.5 Retire the `jam status && jam recs` crontab line once `discover` has produced a playlist, leaving `recs` available for manual use — verify `crontab -l`.

## Workflow follow-up

- Archive this change once the implementation and the project's review requirements are satisfied.
- Verify the archived specs merge cleanly into `openspec/specs/jam-arm/spec.md`.
