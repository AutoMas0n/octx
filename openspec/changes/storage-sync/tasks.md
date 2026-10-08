## 1. Registry — Add Storage Entry

- [x] 1.1 Add `StorageEntry` struct to `src/registry.rs` with `version`, `etag`, and a single `download: DownloadEntry` field (the archive is platform-independent, so no target map), and verify the struct compiles with `cargo check`
- [x] 1.2 Add optional `storage: Option<StorageEntry>` field to `RegistryIndex` and add a `resolve_storage()` method that returns `(url, sha256, version)` (no platform argument), and verify existing tests still pass
- [x] 1.3 Add the `storage` entry (`version`, `etag`, single `download`) to the `registry-index.json` schema in `ARCHITECTURE.md` §5.2 so the release pipeline documents the expected format

## 2. Storage Sync Module

- [x] 2.1 Add `flate2` and `tar` to `Cargo.toml` for gzip/tar extraction and verify `cargo check` passes
- [x] 2.2 Create `src/sync.rs` with a `run()` public function that downloads the storage archive from the registry, verifies SHA256, and reports progress, and verify it compiles
- [x] 2.3 Implement atomic extraction: unpack (`flate2` + `tar`) into `{data_dir}/octx/.storage.tmp/`, atomic rename over `{data_dir}/octx/storage/`, clean up on failure, and verify the directory is consistent after extraction via a test on a temp dir
- [x] 2.4 Add ETag caching: download to `{data_dir}/octx/storage.tar.gz`, store the ETag at `{data_dir}/octx/storage.tar.gz.etag`, and on 304 report storage is current and skip extraction (issue the conditional GET in sync.rs — `install::fetch` errors on 304), and verify repeated calls with no upstream changes skip the download
- [x] 2.5 Add a `force` parameter to `run()` that re-downloads and re-extracts regardless of ETag, and verify it forces a fresh download even when the ETag matches
- [x] 2.6 Add file lock integration (reuse `update.lock`) to serialize concurrent sync invocations, and verify two concurrent syncs serialize correctly

## 3. CLI — Sync Subcommand

- [x] 3.1 Add `Sync { force: bool }` variant to `Command` enum in `src/cli.rs` with a `--force` flag, wire it to `crate::sync::run(force)`, and verify `octx sync --help` and `octx sync --force` parse correctly
- [x] 3.2 Add `pub mod sync;` to `src/lib.rs` and verify the module loads _(declared during 2.2 so sync.rs compiles)_

## 4. Update Integration

- [x] 4.1 In `src/update.rs`, add a storage sync call after the skill sync phase and before the self-update phase, catching and reporting errors without failing the overall update, and verify the update flow completes with a synced storage directory

## 5. Release Pipeline

- [x] 5.1 Add a tracked `storage/.gitkeep` so the repo has a `storage/` directory for the release archive to package (harness content lands later in `storage/harnesses/`)
- [x] 5.2 Add a step to `.github/workflows/release.yml` that runs `tar -czf storage.tar.gz -C storage .` from the repo root (contents only, no nested `storage/` dir) and attaches it to the release, and verify the archive is present in the release assets and extracts to top-level entries (`.gitkeep` now; a `harnesses/` tree once harness content lands)
- [x] 5.3 Add the `storage` entry (`version`, `etag`, single `download` with archive URL + SHA256) to the `registry-index.json` `jq` generator in `.github/workflows/release.yml` — the index is generated, not committed — and verify the emitted JSON parses with the `RegistryIndex` deserializer

## 6. Verification

- [x] 6.1 End-to-end: run `octx sync` against a local registry fixture, verify the storage directory is created with the expected tree, then run it again and verify it's a no-op (ETag cache hit)
- [x] 6.2 Run `octx update` and verify storage sync runs as part of the update flow
- [x] 6.3 Run `octx sync --force` and verify a fresh download and extraction occurs
- [x] 6.4 Verify mirror semantics: edit a file under `{data_dir}/octx/storage/`, run `octx sync`, and confirm the local edit is dropped with no backup, and that a harness removed upstream is gone locally _(verified on an extracting sync — changed upstream + forced; see note: a 304 sync skips extraction per the ETag requirement)_
- [x] 6.5 Run `cargo test` and `cargo clippy -- -D warnings` to confirm no regressions