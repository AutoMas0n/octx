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

### The model depends on an asynchronous mapper — and Last.fm hands us the IDs we discard

**Resolved: forward the recording MBID when the source provides one.** Measured, not assumed.

Last.fm's `user.getRecentTracks` carries a track-level `mbid` in addition to `artist.mbid` and `album.mbid`. The top-level value is a **MusicBrainz Recording MBID** — confirmed by resolving one against MusicBrainz (`/recording/<id>` returns 200; `/track/<id>` and `/release/<id>` return 404). That is precisely the entity type ListenBrainz accepts as `recording_mbid`.

Coverage on live data: 31 of 32 of one account's scrobbles carried it, 29 of 87 of the other's. So it is neither universal nor negligible.

`jam` currently throws the value away at three separate points: the `Text` enum keeps only `#text` and drops the `mbid` beside it, the `Scrobble` struct has no field for a track-level `mbid`, and the listen sent to ListenBrainz carries only `recording_msid`. The value never reaches the wire.

Forwarding it converts a queue-dependent outcome into a deterministic one for every track that has one, and costs nothing — the field arrives in the response `jam` already parses, so no extra request and no `extended` parameter is involved (which the mapper is documented never to request).

**It is a mitigation, not a cure.** Where the MBID is absent, ListenBrainz's own mapper must still match on artist, title and release, on its own schedule. The gap that remains is not the mapping itself but the *silence*: when no model exists, the status check exits non-zero, the scheduler's guard skips the run, and no playlist appears. That is correct — an empty or partial playlist would be worse — but it is indistinguishable from "the model does not exist yet", and it is the thing worth making legible.

## Deferred

- **Log rotation.** The scheduler's logs grow without bound. Not urgent at current volume; recorded so it is a decision rather than an oversight.
- **The removal path is only exercised by the second run.** The first materialization can only add. The `playlist_track_ids` fix is therefore not proven in production until something is recommended today and not tomorrow.
- **Shared destination password.** Two accounts currently share one destination password. Rotating it means updating every account that references it; the harvest fails loudly until then, which is the desired direction of failure.
