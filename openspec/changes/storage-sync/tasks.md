## 1. Registry — Add Storage Entry

- [ ] 1.1 Add `StorageEntry` struct to `src/registry.rs` with version, etag, downloads fields (reusing `DownloadEntry`) and verify the struct compiles with `cargo check`
- [ ] 1.2 Add optional `storage: Option<StorageEntry>` field to `RegistryIndex` and add a `resolve_storage()` method that returns `(url, sha256, version)` for the current platform, and verify existing tests still pass
- [ ] 1.3 Add a `StorageEntry` struct to the `registry-index.json` schema in `ARCHITECTURE.md` so the release pipeline documents the expected format

## 2. Storage Sync Module

- [ ] 2.1 Create `src/sync.rs` with a `run()` public function that downloads the storage archive from the registry, verifies SHA256, and reports progress, and verify it compiles
- [ ] 2.2 Implement atomic extraction: download to `{data_dir}/octx/storage.tar.gz`, extract to `{data_dir}/octx/.storage.tmp/`, atomic rename over `{data_dir}/octx/storage/`, clean up on failure, and verify the directory is consistent after extraction via a test on a temp dir
- [ ] 2.3 Add ETag caching: store ETag alongside the cached archive, return early on 304, and verify repeated calls with no upstream changes skip the download
- [ ] 2.4 Add `--force` flag to re-download and re-extract regardless of ETag, and verify the flag forces fresh download even when the ETag matches
- [ ] 2.5 Add file lock integration (reuse `update.lock` or `storage.lock`) to serialize concurrent sync invocations, and verify two concurrent syncs serialize correctly

## 3. CLI — Sync Subcommand

- [ ] 3.1 Add `Sync { force: bool }` variant to `Command` enum in `src/cli.rs` with a `--force` flag, wire it to `crate::sync::run()`, and verify `octx sync --help` and `octx sync --force` parse correctly
- [ ] 3.2 Add `pub mod sync;` to `src/lib.rs` and verify the module loads

## 4. Update Integration

- [ ] 4.1 In `src/update.rs`, add a storage sync call after the skill sync phase and before the self-update phase, catching and reporting errors without failing the overall update, and verify the update flow completes with a synced storage directory

## 5. Release Pipeline

- [ ] 5.1 Add a step to `.github/workflows/release.yml` that runs `tar -czf storage.tar.gz -C storage .` from the repo root (contents only, no nested `storage/` dir) and attaches it to the release, and verify the archive is present in the release assets and extracts to top-level entries like `harnesses/`
- [ ] 5.2 Update `registry-index.json` in the repo with a `storage` entry pointing to the archive URL and SHA256, and verify the format is parseable by the existing `RegistryIndex::fetch` method

## 6. Verification

- [ ] 6.1 End-to-end: run `octx sync` with a local registry, verify the storage directory is created with the expected tree, then run it again and verify it's a no-op (ETag cache hit)
- [ ] 6.2 Run `octx update` and verify storage sync runs as part of the update flow
- [ ] 6.3 Run `octx sync --force` and verify a fresh download and extraction occurs
- [ ] 6.4 Verify mirror semantics: edit a file under `{data_dir}/octx/storage/`, run `octx sync`, and confirm the local edit is dropped with no backup, and that a harness removed upstream is gone locally
- [ ] 6.5 Run `cargo test` and `cargo clippy -- -D warnings` to confirm no regressions