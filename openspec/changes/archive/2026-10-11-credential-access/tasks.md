# Tasks

## 1. Extract the shared library

- [x] 1.1 Create the `octx-creds` workspace crate and move the existing encryption, key derivation, and storage-format code out of `src/creds.rs` into it. Verify the moved unit tests (round-trip, wrong machine, empty file) pass in the new crate with `cargo test -p octx-creds`.
- [x] 1.2 Define the library's public API: retrieve one host, add, remove, and list host names, with no bulk value export. Verify with a test per operation.
- [x] 1.3 Add the `secrecy` and `zeroize` dependencies to the crate. Verify `cargo check -p octx-creds` succeeds.

## 2. Secure handle semantics

- [x] 2.1 Represent a retrieved credential as a wrapper that renders a redacted placeholder in `Debug` and `Display` and does not implement serialization. Verify a test asserting the formatted output is redacted and contains the value nowhere.
- [x] 2.2 Zeroize the wrapper's backing memory on drop. Verify a test asserting the wrapper clears its buffer on drop.
- [x] 2.3 Provide an explicit accessor plus a helper that applies a credential to an outbound request's authorization header. Verify a test that the header is set correctly and that no caller-local plaintext string is required.
- [x] 2.4 Ensure the library never sources credentials from environment variables or process arguments. Verify a test setting an environment variable named after a host and asserting the library ignores it.

## 3. Head adoption

- [x] 3.1 Reduce `src/creds.rs` to a thin re-export over the library and delete the duplicated crypto. Verify the existing head credential tests pass unchanged with `cargo test`.
- [x] 3.2 Confirm `dispatch::execute()` adds no credential environment injection (it does not today; keep it that way) and cover it. Verify a test that dispatching an arm produces an environment containing no stored credential value.
- [x] 3.3 Keep `octx creds add/get/list/remove` working through the library with redacted output. Verify the existing CLI credential tests pass.

## 4. Agent arm adoption

- [x] 4.1 Switch `arms/agent` to retrieve credentials through the library and remove reliance on `OCTX_TOKEN_*`. Verify the arm builds and its credential tests pass.
- [x] 4.2 Delegate only the credentials configured for a given agent to that agent's subprocess. Verify a test that a host which is not configured for the agent does not appear in the child environment.
- [x] 4.3 Update the agent-arm tests that asserted `OCTX_TOKEN_*` pass-through, including the mock agent fixtures. Verify the updated test suite passes.

## 5. Reconcile the jam change

- [x] 5.1 Confirm the `jam` change references the shared credential library and declares no credential controls (the reconciliation was completed at planning time). Verify `openspec validate jam --strict` passes.
- [x] 5.2 Confirm the `jam` change contains no environment-injection claims. Verify by searching its artifacts for `arm-credentials`, `OCTX_TOKEN`, and injection wording and finding none.

## 6. Documentation and skills

- [x] 6.1 Update the credentials sections of `CONTEXT.md` and `ARCHITECTURE.md` to describe library access and the absence of environment injection. Verify the text no longer advertises `OCTX_TOKEN_*` injection.
- [x] 6.2 Update `arms/fmt/skill.md`, `arms/harness/skill.md`, and `arms/agent/skill.md` environment notes. Verify no skill claims credentials are injected through the environment.

## 7. Integration verification

- [x] 7.1 End-to-end: store a credential through the head, then retrieve it from an arm built against the library and verify the arm receives the correct value.
- [x] 7.2 Negative: dispatch an arm and inspect its environment and arguments for any credential value and verify none appears.
- [x] 7.3 Build an arm against the library in release and verify the binary stays within the arm size budget; record the measured size.
- [x] 7.4 Regression: existing arms still dispatch and propagate their exit codes, and the `octx creds` CLI still works. Verify with a dispatch of an existing arm and a credential round-trip.

## 8. Unreadable store recovery

- [x] 8.1 Report an undecryptable store as a distinct, actionable error naming the store path and the remedy, and never as a generic failure. Verify a test asserting the error text names the file, the remedy, and that nothing changed.
- [x] 8.2 Add a store-level reset operation that moves the store aside under a backup name without reading or deleting it. Verify a test asserting the backup keeps the original bytes and that the store path is then free.
- [x] 8.3 Expose `octx creds reset` through the CLI. Verify an integration test that a foreign store makes `creds list` fail with the actionable error while leaving the file in place, and that `creds reset` then allows `creds add` and `creds list` to succeed.
- [x] 8.4 Document the failure and the recovery path in `ARCHITECTURE.md`. Verify the commands list includes `octx creds reset` and states that nothing is discarded automatically.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result reflects the modified `agent-arm` requirement in the main spec.
