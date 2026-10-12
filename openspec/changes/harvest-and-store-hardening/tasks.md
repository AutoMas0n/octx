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

## 3. Decide on the mapping dependency (needs a decision, not just work)

- [ ] 3.1 Establish whether the recommendation model actually requires **mapped** listens, by checking a real run after the daily snapshot.
- [ ] 3.2 If it does: forward the MusicBrainz ID from the Last.fm payload when it carries one, so the common case does not depend on the asynchronous mapper.
- [ ] 3.3 Whatever the outcome, make the skip distinguishable from "no model yet" in the scheduler's log, so a mapping lag is not an invisible no-op.
- [ ] 3.4 Record the decision and its evidence in `design.md`.

## 4. Prove the removal path

- [ ] 4.1 Confirm the second materialization run removes tracks that are no longer recommended, exercising the `playlist_track_ids` path fixed in v0.4.1. The first run only proves adding.
- [ ] 4.2 Interrupt a run mid-way and confirm the playlist is left with extra tracks rather than hollowed out, as the "additions before removals" contract requires.

## 5. Scheduler hygiene

- [ ] 5.1 Add rotation for the scheduler's logs; nothing rotates them today.
- [ ] 5.2 Document that rotating the shared destination password means updating `password` under every account that uses it, and that the harvest then fails loudly until it is updated.
