# Proposal

## Why

Setting `jam` up for two real accounts on the scheduler box turned up four gaps between what the specs say and what the system does. Three were live bugs, now fixed in code but **still undescribed by any spec**, and one is a design fragility that no spec covers at all.

The most serious is that `credential-access` was *satisfied* while the store was world-readable. Its requirements cover encryption at rest, redaction, zeroization and least privilege — but nothing about the file's mode, so a store holding the Last.fm key, both ListenBrainz tokens and both Qobuz passwords sat at `0664` on the scheduler box and every requirement still passed. A spec that cannot fail on that is under-specified, not merely stale.

Separately, the Qobuz token harvester — a component that holds a plaintext session token in memory, drives a browser, and writes to the credential store — has **no spec area at all**.

## What Changes

- **`credential-access` gains a requirement on the store's file mode.** The store, its lock file and its directory must be owner-only, and the mode must be applied after writing rather than only at creation, because `mode()` is ignored for a file that already exists. An existing loose store must be tightened rather than left as found.
- **New `qobuz-harvest` spec area.** Captures what the harvester already does but nothing records: observe the token off a real request rather than replaying a login; never print or log the token's value; refuse a malformed credential document instead of overwriting it; one browser profile per account; confirm the token's owner before storing it and fail closed when that cannot be established.
- **`jam-arm` gains the multi-account document contract.** `accounts.<name>` with a document-level default, per-account credentials resolved by name, and the guarantee that one account's failure cannot write into another account's library.
- **Record the mapping fragility as a design note.** `jam` does not send MusicBrainz IDs, so the recommendation model depends on ListenBrainz's asynchronous mapper having caught up before the daily snapshot. When it has not, `jam status` exits non-zero and the run is skipped silently — correct, but indistinguishable from "no model yet".

## Non-goals

- No change to how tokens are obtained (still observe-not-replay), to the arm's no-browser rule, or to the broker-daemon non-goal.
- No new credential exposure path. Nothing here adds an environment or argument source.
- Not addressing log rotation or the `recs` removal path; both are noted in `design.md` and belong to a separate change.
