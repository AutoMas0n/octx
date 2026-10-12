# Tasks

## 1. Sync the specs with behaviour that already exists

- [ ] 1.1 Apply the `credential-access` delta to `openspec/specs/credential-access/spec.md` (owner-only modes; encryption is not access control).
- [ ] 1.2 Create `openspec/specs/qobuz-harvest/spec.md` from the delta — the harvester currently has no spec area at all.
- [ ] 1.3 Apply the `jam-arm` delta to `openspec/specs/jam-arm/spec.md` (named accounts, no cross-account writes, owner-confirmed destination token).
- [ ] 1.4 Confirm each new requirement is actually exercised by an existing test or a recorded manual verification, and note the evidence inline. A requirement nothing checks will drift again.

## 2. Close the documentation gap in the repo

- [ ] 2.1 `CONTEXT.md` still says "the browser profile lives in a Docker named volume" — singular. Correct it to one profile **per account** and say why, so the shared-profile bug is not reintroduced by someone reading the docs.
- [ ] 2.2 Document the ownership check in the same section: a token is refused unless the service confirms it belongs to the account.
- [ ] 2.3 Document the scheduler shape — per-account harvest and run lines, and why concurrent runs must not share a container name.

## 3. Forward the recording MBID — decided

Decision and evidence in `design.md`. Last.fm's track-level `mbid` is a MusicBrainz **Recording** MBID (verified against MusicBrainz: `/recording/<id>` 200, `/track/<id>` and `/release/<id>` 404), which is the entity type ListenBrainz takes as `recording_mbid`. It is present on a large share of live scrobbles and is currently discarded.

**Do not start this before the first end-to-end run is observed.** The pipeline has never completed once; changing the code under test before the one scheduled verification would discard the only clean result we can get today.

- [ ] 3.1 Carry the track-level `mbid` through parsing: keep it alongside `#text` in the Last.fm text fields, and add it to the scrobble and listen shapes.
- [ ] 3.2 Validate it as a UUID before sending, so a malformed or truncated value from the source is dropped rather than rejected by ListenBrainz mid-submission.
- [ ] 3.3 Send it as `recording_mbid` on submission, omitting the field entirely when absent rather than sending an empty string.
- [ ] 3.4 Confirm the mapper still requests no `extended` parameter — the track-level `mbid` is present without it.
- [ ] 3.5 Test both directions: a payload with the MBID, one without, and one with an empty string, plus a submission that carries an ID and one that does not.
- [ ] 3.6 Make the skip legible in the scheduler's own output, so "no model yet" and "run succeeded" are distinguishable without reading ListenBrainz's response codes.

## 4. Prove the removal path

- [ ] 4.1 Confirm the second materialization run removes tracks that are no longer recommended, exercising the `playlist_track_ids` path fixed in v0.4.1. The first run only proves adding.
- [ ] 4.2 Interrupt a run mid-way and confirm the playlist is left with extra tracks rather than hollowed out, as the "additions before removals" contract requires.

## 5. Scheduler hygiene

- [ ] 5.1 Add rotation for the scheduler's logs; nothing rotates them today.
- [ ] 5.2 Document that rotating the shared destination password means updating `password` under every account that uses it, and that the harvest then fails loudly until it is updated.
