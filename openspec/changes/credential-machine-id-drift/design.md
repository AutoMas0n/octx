# Design

## Context

`crates/octx-creds/src/lib.rs` derives the credential encryption key from `machine_id()`. The function walks a fallback chain and returns the first id it can read:

| # | Source | Stability |
| --- | --- | --- |
| 1 | `/etc/machine-id` | stable per host |
| 2 | `/var/lib/dbus/machine-id` | stable per host |
| 3 | `getprop ro.build.fingerprint` | stable per device+ROM build; **changes on OS update** |
| 4 | macOS `ioreg IOPlatformUUID` | stable per host |
| 5 | `/proc/sys/kernel/random/boot_id` | **changes on every boot** |

`derive_key` (`crates/octx-creds/src/lib.rs:419`) runs Argon2 over the id and the file's salt. The store file is `{config_dir}/octx/creds.enc`; its first 44 bytes are salt and nonce, and the rest is the encrypted credential document. Decryption failure is reported by `CredsError::Decrypt` as "failed to decrypt (wrong machine or corrupted file)".

Observed on 2026-10-19: the Android device had no `/etc/machine-id` or `/var/lib/dbus/machine-id`; the id resolved from the fingerprint. The 179-byte store could not be decrypted by the current fingerprint, by an older installed binary, or by the boot-id — and it was too small to hold the credential it appeared to hold. It was a store from a different machine identity, not a corrupt one. See proposal.md — Why.

## Goals / Non-Goals

**Goals:**

- Make the encryption identity's *origin* observable, so "wrong machine" can name what moved.
- Stop a credential from being encrypted under a key that dies at the next boot without warning.
- Keep existing stores readable when the machine identity has not moved, and keep the encryption algorithm and the redaction/zeroization guarantees unchanged.
- Give the user a read-only diagnostic that reports the resolved identity source and whether the store decrypts.

**Non-Goals:**

- No weakening of "a different machine cannot decrypt": the key must still be unavailable to another host.
- No change to the credential storage *format's* encryption, only to the metadata that travels with it (if any).
- No recovery of the already-lost phone store in this change; that is an operational fix (re-add the credential from its production copy).
- No attempt to make a mutable fingerprint immutable — an OS upgrade genuinely changes it.

## Decisions

### Return the identity together with its source

Change the internal identity resolution to yield `(id, source)` where `source` names the mechanism (`/etc/machine-id`, `dbus-machine-id`, `android-fingerprint`, `macos-platform-uuid`, `boot-id`). The value that feeds key derivation stays exactly the id string, so existing stores remain readable — only the *diagnostic* gains information. Expose it through a small, non-secret accessor (source name only, never the id in full).

*Alternative considered:* store the source in the credential file's header. Rejected as the primary mechanism because it changes the format and would break reading an existing store that has no header field. The source belongs to the *host*, so resolving it at read time is both simpler and sufficient for diagnosis. A header field can be added later if a store ever needs to travel with its own origin.

### Gate key derivation on a non-volatile source

When the resolved source would be the boot-id, do not encrypt a new credential silently. The two candidate behaviors:

- **Refuse** the write with a clear error naming the reason and the remedy.
- **Warn and mark** the store as boot-scoped, so the user knows it will not survive a reboot.

Pick one in implementation; both require the same underlying change — the source is known before the write. *Alternative considered:* hard-fail on *any* source that is not `/etc/machine-id`. Rejected because Android has no such file; the fingerprint is the correct identity there. The gate is specifically about the *volatile* source.

### Diagnose drift at read time

When decryption fails, the error message reports the source that resolves now and, when available, the source recorded when the store was written. With the source-only metadata above, the "recorded" side may only be known for stores written after this change; for older stores the message states the current source and that the store predates the metadata. The wording must keep the "nothing was changed" reassurance and the reset command — the existing requirement still governs.

### Add a read-only diagnostic command

A subcommand under `octx creds` reports:

- which source produced the current machine id (name only),
- whether the configured store exists and decrypts,
- and, when it does not, the same actionable guidance as the error.

It prints no credential value and no full machine id. This is the tool an operator runs before assuming corruption.

## Risks / Trade-offs

- **A changed fingerprint after an OS upgrade now surfaces as a refused write.** → The user's credentials are not lost — the old store still cannot be read, but the new error explains why, and the user can re-add. The trade is a louder failure in exchange for a diagnosable one.
- **Source names are a new public string surface.** → Keep the set small and closed; treat unknown sources as "unknown" rather than exposing a raw command line.
- **The diagnostic must not leak the id.** → Print the source name and a match/mismatch verdict; never the id, never a credential.
- **Prevention does not help already-lost stores.** → Called out as a non-goal; the operational remedy is to re-add the credential from its authoritative copy (the production environment), which is already the working path.
