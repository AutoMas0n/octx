# Tasks

## 1. Identity resolution reports its source

- [ ] 1.1 Add an internal `machine_id_with_source()` in `crates/octx-creds/src/lib.rs` that returns the id together with the name of the fallback mechanism that produced it, and keep `machine_id()` as the thin, id-only wrapper so existing callers do not change
- [ ] 1.2 Add unit tests asserting the source name for each resolvable branch (an `/etc/machine-id` fixture, a `getprop` fixture where available, and the boot-id last resort), and verify with `cargo test -p octx-creds`
- [ ] 1.3 Expose a non-secret accessor for the current identity's source (source name only), and verify a unit test shows the id itself is not returned

## 2. Do not silently encrypt under a volatile identity

- [ ] 2.1 Gate key derivation in the write path on a non-volatile source: when only the boot-scoped last resort resolves, refuse the new write with an error naming the source and the remedy, or mark the store boot-scoped — pick one and document it in the design's decision
- [ ] 2.2 Verify with a test that a credential added while only the volatile source resolves does not produce a silently-strandable store, and that the pre-existing store is left untouched
- [ ] 2.3 Verify with a test that a credential written under a stable source still stores and retrieves normally

## 3. Diagnose drift on decryption failure

- [ ] 3.1 Extend the decryption-failure error in `crates/octx-creds/src/lib.rs` to name the current identity's source and to distinguish "identity moved" from "corrupt file", while keeping the existing "nothing was changed" reassurance and the reset command
- [ ] 3.2 Verify with a test that a store encrypted for a different id produces the diagnostic error, still leaves the file's bytes intact, and never reveals a credential value

## 4. Read-only diagnostic command

- [ ] 4.1 Add a read-only `octx creds` diagnostic subcommand (for example `octx creds doctor`) that reports the identity's source and whether the configured store decrypts
- [ ] 4.2 Verify with a test that the diagnostic prints the source name and a decrypt verdict and prints neither the full machine id nor any credential
- [ ] 4.3 Verify with a test that the diagnostic leaves the store's bytes and location unchanged when it cannot decrypt

## 5. Verification

- [ ] 5.1 Run `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings`, and verify both are clean
- [ ] 5.2 Run `cargo test` for the affected crates and the head, and verify the suites pass
- [ ] 5.3 Run `openspec validate credential-machine-id-drift --strict` and verify the change validates
- [ ] 5.4 Confirm an existing store written under an unchanged machine identity still decrypts, so the change is backward compatible

## Workflow follow-up

- Decide the format-compatibility question raised in proposal.md (versioned envelope vs sidecar) and record the decision in design.md before implementing any header change.
- Decide the recovery path for an already-undecryptable store and record it; the immediate operational remedy is to re-add the credential from its authoritative copy in the production environment.
- Sync the `credential-access` delta into `openspec/specs/credential-access/spec.md` and archive the change once review requirements are satisfied.
