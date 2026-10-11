# Design

## Context

See `proposal.md` — Why. Constraints that shape this design:

- `src/creds.rs` already implements the at-rest scheme (machine-ID-derived key via argon2, AES-256-GCM, `salt || nonce || ciphertext`) but is private to the head binary.
- The `jam` change originally specified credential delivery as head-injected `OCTX_TOKEN_*` environment variables (capability `arm-credentials`). That capability has since been removed and its references reconciled; this change replaces that mechanism.
- `agent-arm` has a main-spec requirement, `OCTX_TOKEN injection`, that depends on head-injected variables and therefore must change.
- Arms are separate binaries and may spawn subprocesses; some spawned third-party agents expect credentials in their own environment.
- octx deliberately has no daemon and favors small, self-contained binaries.

## Goals / Non-Goals

**Goals:**

- One shared, auditable credential implementation used by the head and arms.
- No secret passes through an environment variable or a process argument, with one deliberate exception: the agent arm's explicit, per-host delegation to the child agent it starts (see Decisions). The head never places a credential in the environment of an arm it dispatches, and the library never takes one from the environment or from arguments.
- Credentials surfaced as redacted, zeroized handles with an explicit point of use.
- Per-host least-privilege retrieval; listing never reveals values.

**Non-Goals:**

- No credential broker or daemon that performs authenticated requests on an arm's behalf. A bearer token must exist in-process at send time; a broker would add a service (against octx's design) and a same-UID attacker could speak to its socket anyway, so it does not improve protection against the threat in scope.
- No protection against root or OS-level memory inspection; that is stated as residual risk, not solved.
- No change to the at-rest encryption algorithm.

## Decisions

**Extract an `octx-creds` workspace library.** The encryption, key derivation, storage format, and retrieval move into a new crate that both the head and arms depend on; `src/creds.rs` becomes a thin re-export. *Alternatives:* leave it in the head and keep arms dependent on head behavior (rejected — forces env or reimplementation); duplicate the crypto per arm (rejected — untestable drift, and the same crypto bugs in N places).

**Use `secrecy` and `zeroize` rather than hand-rolling.** The secret wrapper comes from a maintained crate that already redacts in `Debug`/`Display`, is not `Serialize`, and zeroes on drop. *Alternatives:* a bespoke wrapper (rejected — the project's own guidance is not to reinvent wheels, and redaction/zeroization have subtle failure modes).

**API shape: `get(host) -> Secret`, `list_hosts() -> [String]`, no bulk value export.** Arms name exactly the host they need. The `Secret` exposes the value only through an explicit accessor and a request-header helper. *Alternatives:* return a `HashMap` of all credentials (rejected — maximizes blast radius); return a bare `String` (rejected — invites logging and copies).
**Stored values are opaque to the library.** A host may hold a single token or a structured document its consumer owns — `jam` keeps one account document at host `jam` — and the library neither parses nor validates it, so a consumer can evolve its own schema without a change here and without the library needing to know what a credential means.

**Keep credentials out of the head's dispatch path.** `dispatch::execute()` does not add `OCTX_TOKEN_*` today (the previous plan was to add it; this change abandons that) and must continue not to, covered by a test. This is a contract change relative to the assumed `OCTX_TOKEN_*` behavior, accepted in the proposal. *Alternative:* implement and keep injection while adding the library (rejected by the chosen "supersedes" scope — two mechanisms drift, and env stays leaky).

**The agent arm delegates only configured credentials explicitly.** Since a spawned third-party agent may require credentials in its environment, the agent arm reads the specific hosts configured for that agent from the library and sets only those on the child. *Alternative:* forward all `OCTX_TOKEN_*` (rejected — that is exactly the over-broad exposure being removed).

**Reconciled the `jam` change in this change set.** `jam`'s `arm-credentials` capability and env-injection references have already been removed and its need re-pointed at the library, so exactly one mechanism defines credential delivery. The remaining task verifies the reconciliation.

**An unreadable store is reported, never repaired silently.** The key is derived from the machine identity, so a `creds.enc` copied from another machine is undecryptable here by design — and because `add` reads before it writes, one bad file blocks every credential command. A command that cannot decrypt therefore fails with the file path, the fact that nothing changed, and the recovery command, and the library gains a `reset` operation that renames the file to `creds.enc.bak` (or the next free `creds.enc.bak.N`) without reading it. *Alternatives:* treat the file as corrupt and start over (rejected — silently destroys the only copy of credentials, and a machine-identity change or a restore would silently wipe a working store); instruct the user to `rm` the file (rejected — hands the destructive step to a human with no backup, and no path in the message means guessing which file).

## Risks / Trade-offs

- **Every arm now carries crypto dependencies** (argon2, aes-gcm, secrecy, zeroize via the shared library) → measured against the arm size budget; the shared release profile (`opt-level = "z"`, LTO, strip) applies. Verified: the agent arm (which links the library) builds to 1.56 MB against a 2 MB budget; a crypto-free arm (`fmt`) is 0.45 MB.
- **Arms need filesystem access to the config store and machine identity** → arms already run as the invoking user with access to octx's directories; direct invocation (outside `octx x`) now also works, which is a benefit.
- **Contract change for anything written against `OCTX_TOKEN_*`** → migrate `agent-arm`, verify `jam`'s reconciliation, and update docs/skills in the same change.
- **`secrecy` cannot guarantee every intermediate copy is zeroized** → avoid cloning secrets, expose a single accessor, and document the residual risk rather than claiming it is solved.
- **Same-UID memory inspection remains possible** → explicitly documented as out of scope; the change reduces exposure surface, it does not claim immunity.
- **A delegated credential is visible in the child's environment** → the child inherits `OCTX_TOKEN_<HOST>`, so anything running as the same user can read it from `/proc/<pid>/environ` while that child lives. This is the same-UID exposure already out of scope, and the delegation stays limited to hosts an operator named with `--credential-host` — never every configured host.
- **Machine-bound keys fail legitimately** → a restored backup, a re-flashed device, or a changed Android fingerprint makes an existing store undecryptable. The failure is expected behavior, not corruption, so the response is an actionable error plus a non-destructive `reset` rather than automatic recovery.

## Migration Plan

1. Extract `octx-creds`; point the head at it (no behavior change yet).
2. Point the head's dispatch path at the library and confirm it adds no credentials to an arm's environment (it did not before; keep it that way).
3. Update `agent-arm` to retrieve via the library and delegate only configured hosts.
4. Verify `jam`'s artifacts reference the library and carry no env-injection claims (reconciliation already done at planning time).
5. Update docs and skill files.
6. Rollback: the env-injection path can be restored behind the old code, but this is not recommended; the library path is independently revertible before steps 2–3 land.

## Open Questions

- Whether to add an optional OS-keyring backend behind the library later. Deferrable: it does not change the API, the specs, or the task breakdown.
