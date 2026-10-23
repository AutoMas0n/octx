# Tasks

## 1. State storage and path resolution

- [x] 1.1 Add `dirs.workspace = true` to `arms/jam/Cargo.toml` and verify `cargo check -p octx-jam` succeeds
- [x] 1.2 Add a `state_dir` to `config::Config`, defaulting to `{data_dir}/octx/jam` and overridable by `OCTX_JAM_STATE_DIR`, and verify a unit test shows the override wins and the default ends with `octx/jam`
- [x] 1.3 Implement the discovery memory in a new `arms/jam/src/state.rs`: a versioned JSON document keyed by listener, `load`/`record`/`save`, a bounded most-recent window, and an atomic temp-file-plus-rename write; register the module in `lib.rs`
- [x] 1.4 Add unit tests in `state.rs` proving per-listener keying, oldest-record eviction at the window bound, a missing document reads as empty, a malformed document reads as empty and is replaced on the next save, and the written document carries its version — verify with `cargo test -p octx-jam --lib`
- [x] 1.5 Make the exclusion-key helper (`discover::exclusion_key`) reachable by the memory so both agree on track identity, and verify a unit test asserts the memory and the source produce the same key for the same track
- [x] 1.6 State the memory's location in the `jam discover --help` text (`arms/jam/src/cli.rs`) and in `arms/jam/skill.md`, and verify `cargo run -p octx-jam --bin jam -- discover --help` prints it

## 2. Source: remember deliveries and widen the pool

- [x] 2.1 Extend `discover::discover` to take the remembered exclusion keys and union them with the scrobble exclusion in the single read already performed, and verify tests in `arms/jam/tests/discover.rs` cover a remembered track being excluded, the union being applied in one pass, and a track absent from the memory being delivered
- [x] 2.2 Widen the read limits in `discover.rs` (`MAX_SIMILAR` 5→10, `MAX_TRACKS_PER_ARTIST` 3→10) and rework selection to draw round-robin across individual artists within each seed, and verify tests cover each artist contributing a first track before any second and the requested count still being met when the first candidates are excluded
- [x] 2.3 Wire the source in `arms/jam/src/run.rs`: resolve the listener, load its remembered keys, and pass them to `discover::discover`, and verify integration tests in `arms/jam/tests/discover_cli.rs` run against a scratch `OCTX_JAM_STATE_DIR`
- [x] 2.4 Update `arms/jam/skill.md` to describe the rotation (memory window) and the wider, artist-spread selection, and verify the described commands match `jam discover --help`

## 3. Write path: record only a materialized playlist

- [x] 3.1 Record the delivered tracks into the memory only after a successful, non-dry-run Qobuz discovery delivery (the same point retention runs), and verify tests prove a successful Qobuz run records, while a `--dry-run` run, a `--output json` run, and a failed Qobuz sync record nothing
- [x] 3.2 Make a memory read or write failure non-fatal to a run whose playlist already delivered — report it on stderr and keep the exit status of the delivery — and verify a test drives an unwritable memory path and observes a successful run
- [x] 3.3 Create the memory's directory when writing rather than failing when it is absent, and verify a test points `OCTX_JAM_STATE_DIR` at a non-existent path and observes the document being written

## 4. Integration verification

- [x] 4.1 Run `cargo fmt --all` and `cargo clippy -p octx-jam --all-targets -- -D warnings`, and verify both are clean
- [x] 4.2 Run `cargo test -p octx-jam` and verify the whole arm suite passes
- [x] 4.3 Run `openspec validate jam-discovery-variety --strict` and verify the change validates

## Workflow follow-up

- Sync the `jam-arm` delta into `openspec/specs/jam-arm/spec.md` and archive the change once review requirements are satisfied.
- Confirm the memory document is created at `{data_dir}/octx/jam/discovery-state.json` on a real run and survives an `octx sync`.
