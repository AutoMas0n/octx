# Proposal

## Why

On 2026-10-19 the credential store on the Android/Termux device (`~/.config/octx/creds.enc`, 179 bytes) could not be decrypted. The file was intact — larger than its 44-byte salt-and-nonce header, unchanged since 2026-10-11 — but every key the device could produce failed against it. The credential document it held was also implausibly small: the `jam` document alone is 690 bytes, so the 179-byte file could not have contained it. The store was almost certainly written on a *different* machine identity and later copied or restored onto this device.

The encryption key is derived from `machine_id()` (`crates/octx-creds/src/lib.rs`), whose fallback chain is:

1. `/etc/machine-id` — Linux standard
2. `/var/lib/dbus/machine-id` — Linux fallback
3. `getprop ro.build.fingerprint` — Android/Termux
4. macOS `ioreg IOPlatformUUID`
5. `/proc/sys/kernel/random/boot_id` — last resort, **changes on every boot**

On this device the first two do not exist; the id resolves from `getprop ro.build.fingerprint`. A fingerprint changes when the OS build is updated. If the store is ever written while `getprop` cannot resolve — early boot, a stripped ROM, a sandbox where `getprop` is absent — the key falls through to the boot-id, which is regenerated on the next reboot. Either way the key that encrypted the file can never be reproduced, and the failure surfaces only as the generic "wrong machine or corrupted file".

The current spec already requires the *unreadable-store* error to be actionable and recoverable (naming the path, the "nothing changed" reassurance, and the reset command) — and that behavior worked as intended. What is missing is any way to *diagnose the cause* or to prevent the store from being encrypted under a key that will not survive the next boot. A user who has lost the key cannot tell "the file is corrupt" from "this device's identity moved", and the second case is both more common and silently permanent.

## What Changes

- **The machine identity records its origin.** The id used for key derivation is paired with the name of the source it came from, so a later mismatch can name *which* source moved instead of reporting only "wrong machine".
- **The volatile last resort is never used silently.** If the id would come from `/proc/sys/kernel/random/boot_id`, the store refuses to encrypt a *new* credential under it (or clearly marks the store as boot-scoped), so a credential cannot be lost by a reboot without warning.
- **The unreadable-store error diagnoses the drift.** When decryption fails, the error reports the source the store was written under, when known, against the source resolving now, so the message distinguishes a moved machine identity from a corrupted file.
- **A read-only diagnostic reports the resolved identity.** A command surfaces which source produced the current machine id and whether the existing store decrypts, without ever revealing a credential value or the id itself in full.

This is a change to existing behavior owned by `credential-access`; it does not add a new capability and does not change the storage format's encryption algorithm or the redaction/zeroization guarantees.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `credential-access`: the machine-identity requirement gains an origin-aware, non-volatile identity and a diagnosable failure mode. The existing "unreadable store is actionable and recoverable" requirement is extended, not replaced; the existing "credentials encrypted at rest" requirement keeps its guarantee that a different machine cannot decrypt.

## Impact

- `crates/octx-creds/src/lib.rs` — `machine_id()` returns or pairs with its source; key derivation is gated on a non-volatile source; the decryption error carries source information.
- The credential store header or metadata — enough to record the source the store was encrypted under, if the format allows it without a breaking change.
- The head's credential CLI (`src/creds.rs`, `src/cli.rs`) — the diagnostic subcommand and the improved error text.
- `openspec/specs/credential-access/spec.md` — requirements and scenarios updated to match.
- Tests — unit coverage in `crates/octx-creds` for source selection and the volatile-source refusal; a diagnostic test that runs against a store encrypted for a different id.

## Open questions for the implementing agent

- **Format compatibility.** Does the current on-disk format leave room for source metadata without a version bump? If not, decide between a versioned envelope and an out-of-band sidecar. Existing stores must keep working when the identity has *not* moved.
- **Recovery over prevention.** Prevention only protects stores written after the change. Decide whether to also offer a migration path for an already-undecryptable store — for example, accepting a user-supplied machine id, or a one-time re-encryption when the plaintext is supplied by another means. The `jam` document's real copy lives in the production environment, so a fresh `octx creds add` is currently sufficient; a general recovery path is still worth deciding on.
- **Scope of "non-volatile".** Whether a changed fingerprint should be treated as a hard refusal or a warned, opt-in override. A legitimate OS upgrade changes the fingerprint, and refusing outright would strand credentials the user still wants.
