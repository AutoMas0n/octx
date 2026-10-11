# Proposal

## Why

Arms need credentials, but octx has no reusable, safe way to give them one. The encryption implementation lives privately inside the head (`src/creds.rs`), so each arm either depends on the head to hand it secrets or would have to reimplement decryption. The original plan (`jam`'s `arm-credentials` capability, since removed) delivered secrets through `OCTX_TOKEN_*` environment variables, but the environment is a leaky channel: it is readable by any same-UID process via `/proc/<pid>/environ`, inherited by every subprocess an arm spawns, and captured in crash dumps. As the number of arms grows, every one of them becomes another place secrets can escape.

## What Changes

- **New reusable credential library** (`octx-creds`, a workspace crate) holding the single implementation of at-rest encryption, key derivation, and retrieval. The head and every arm link it instead of duplicating crypto.
- **Secrets never travel through the environment or argv.** The head does not place credentials in an arm's environment; arms request a specific credential from the library in-process.
- **Credentials are handles, not strings.** A retrieved credential is a wrapper that redacts itself in all formatting, is not serializable, and zeroes its memory on drop. The plaintext is reachable only through an explicit point-of-use method.
- **Least-privilege retrieval.** An arm asks for one named host and receives only that credential; listing exposes host names, never values.
- **Supersedes `jam`'s `arm-credentials` capability.** That capability has already been removed and `jam`'s planning artifacts reconciled to the library, so exactly one mechanism defines credential delivery.
- **An unreadable store is recoverable, not silently repaired.** Because the key is derived from the machine identity, a store copied from another machine cannot be decrypted here, and every credential command fails until it is dealt with. Such a failure now names the file and the remedy, and a new `octx creds reset` moves the file aside under a backup name — automatically discarding a store is never an option.
- **BREAKING**: arms may no longer rely on head-injected `OCTX_TOKEN_*`. `agent-arm`'s credential handling changes accordingly; when it launches a third-party agent that expects credentials in its environment, it supplies them explicitly and only for configured hosts.

## Capabilities

### New Capabilities

- `credential-access`: How any octx component obtains a stored credential — a shared library with encrypted at-rest storage, redacted and zeroized handles, per-host least-privilege retrieval, and no credential passed through the environment or process arguments except the agent arm's explicit, per-host delegation to the child it starts.

### Modified Capabilities

- `agent-arm`: The credential-handling requirement changes. The arm obtains credentials through the `credential-access` library rather than reading head-injected `OCTX_TOKEN_*` environment variables, and passes credentials to a spawned agent only explicitly and for configured hosts.

## Impact

- **New**: workspace crate `octx-creds` (library), its public API, and unit/integration tests.
- **Modified**: `src/creds.rs` becomes a thin re-export over the library; `src/dispatch.rs` stays free of credential env injection (it never injected, and will not); `arms/agent` switches to library retrieval and explicit, configured subprocess injection; `openspec/changes/jam/` has been reconciled (its `arm-credentials` capability and env-injection references removed); the `octx creds` CLI gains `reset`.
- **Dependencies**: `secrecy` and `zeroize` for redaction and memory clearing (existing `argon2`, `aes-gcm`, `machine-uid`, `serde_json` move into the library).
- **Binary size**: arms that link the library carry the crypto dependencies; release profile (`opt-level = "z"`, LTO, strip) applies as for the head.
- **Compatibility**: the `OCTX_TOKEN_*` env contract was documented and assumed but never implemented in the head; any arm written against it must adopt the library.
