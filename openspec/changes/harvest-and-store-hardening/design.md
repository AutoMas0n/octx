# Design

## Context

`jam` was put into real service on an always-on scheduler box, driving two people's accounts. That exercise is what surfaced this change: the failures below were all invisible until something ran on a schedule, against live accounts, with more than one of everything.

## Goals / Non-goals

**Goals** — write down contracts the code already honours but no spec states, and record the fragilities that are not yet worth fixing but must not be forgotten.

**Non-goals** — changing how a token is obtained, adding a credential exposure path, or introducing any daemon. The arm stays invocation-only.

## Decisions

### Access control is a separate requirement from encryption

The store was encrypted correctly and still readable by every user on the box (`0664`, with a `0775` directory). Every existing `credential-access` requirement passed. Encryption protects contents in transit and at rest against someone who obtains the *file*; it is not a substitute for the operating system deciding who may open it. These are now two requirements, and the second one explicitly says the first does not imply it.

The mode must be applied **after** writing. `mode()` is ignored for a path that already exists, so a creation-time-only rule would leave every existing store loose forever. Applying it after the write also means a store left loose by an older build is corrected by its next write rather than staying as found.

### Professor: the ownership check is only as good as the expectation

Confirming a captured token's owner catches a profile holding the wrong session. It does **not** catch a wrong *expectation* — if the stored identity is itself wrong, the check confirms the wrong thing and reports success quietly. Encountered exactly this: a seeding script keyed by variable name let the active account's email overwrite the commented-out one, the harvest then signed in as the wrong person, and the verification reported a confident match because that was what had been stored.

Nothing in the check can detect this. The mitigation is procedural: seed from a source you have actually inspected, and print both identities side by side rather than one. Recorded here because it will happen again.

### Reads are not protected by the store lock

The store lock covers a single write. A read-modify-write across two processes — read the document, merge, write it back — is not atomic, so two concurrent runs can lose one another's update. The impact is mild (an account keeps a stale token, which remains valid), but it is the reason scheduled runs are staggered rather than left to collide. A future change could expose a compare-and-set or a read-modify-write under one lock; not worth it now.

### The model depends on an asynchronous mapper nobody controls

`jam` sends listens without MusicBrainz IDs even though the Last.fm payload carries them for a minority of tracks. ListenBrainz's mapper fills them in on its own schedule, and the recommendation model is built from mapped listens. So the first useful run depends on a third party's queue having drained before a fixed daily snapshot.

When it has not drained, nothing looks broken: the status check exits non-zero, the scheduled run is skipped by its guard, and no playlist appears. That is the correct behaviour — writing an empty or partial playlist would be worse — but it is indistinguishable from "the model does not exist yet". Making those two states tell themselves apart matters more than eliminating the dependency.

## Deferred

- **Log rotation.** The scheduler's logs grow without bound. Not urgent at current volume; recorded so it is a decision rather than an oversight.
- **The removal path is only exercised by the second run.** The first materialization can only add. The `playlist_track_ids` fix is therefore not proven in production until something is recommended today and not tomorrow.
- **Shared destination password.** Two accounts currently share one destination password. Rotating it means updating every account that references it; the harvest fails loudly until then, which is the desired direction of failure.
