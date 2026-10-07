## Context

See proposal.md for motivation. The octx head currently fetches, verifies, and installs compiled arm binaries from a registry index. The download/verify/extract machinery exists in `src/install.rs` and `src/util.rs`, with ETag caching in `src/update.rs`. Storage sync is the same pattern applied to a tarball of non-binary assets instead of a single gzipped binary.

No new external dependencies are needed — the head already has tokio, reqwest, sha2, serde, fs2, and the temp-file/rename pattern used by install.

## Goals / Non-Goals

**Goals:**
- Single `octx sync` command that downloads, verifies, and extracts the storage archive
- `octx update` runs storage sync as part of the update flow
- ETag caching: repeated syncs with no upstream changes are a no-op
- Atomic extraction: partial/corrupted extraction never leaves the storage directory inconsistent
- Concurrent safety via file lock (same pattern as `update.lock`)

**Non-Goals:**
- Selective sync of individual subpaths (sync everything or nothing — YAGNI)
- Storage version pinning (always latest, like arms)
- Preserving local modifications or merging edits in the storage mirror (it is canonical and read-only)
- Relocating the storage mirror itself (user-authored content lives outside the mirror — e.g. `{config_dir}/harnesses/`, or a `--local-dir` on the harness arm)
- Sync from non-GitHub sources (v1 uses the same registry source as arms)

## Decisions

### Decision: Storage is a canonical read-only mirror, not a user workspace
`{data_dir}/octx/storage/` is octx-owned and replaced wholesale. Sync does not merge, back up, or detect local modifications — anything absent from the archive is dropped. This follows the project's separation principle: an arm is the execution contract, while harnesses are independently-authored content, so user-authored copies live under `{config_dir}/harnesses/` or a `--local-dir`, never in the mirror. Alternatives: preserving/merging local edits (rejected — that is a full package manager, out of scope for this change), or a read-only mount (overkill). Version tracking stays simple: a manifest-recorded version/etag, the mirror reflecting only the current release, no per-file history or pinning.

### Decision: Storage is a registry entry, not a separate index
The `storage` entry lives in `registry-index.json` alongside `head` and `arms`, using the same version/etag/downloads/sha256 structure. This reuses the existing fetch-with-cache machinery and avoids a second HTTP request to a different URL.

### Decision: Reuse `fetch_with_cache` from install.rs
The existing `fetch_with_cache(url, cache_path)` function handles ETag conditional requests, streaming to a temp file, and atomic rename. Storage sync calls it with the archive URL and a `{data_dir}/octx/storage.tar.gz` cache path. This keeps the sync module small.

### Decision: Atomic extraction via temp dir + rename
1. Download to `{data_dir}/octx/storage.tar.gz` (cached, ETag-checked)
2. If new content was downloaded, extract to `{data_dir}/octx/.storage.tmp/`
3. Rename `{data_dir}/octx/storage/` → `{data_dir}/octx/.storage.old/` (if exists)
4. Rename `{data_dir}/octx/.storage.tmp/` → `{data_dir}/octx/storage/`
5. Remove `{data_dir}/octx/.storage.old/`
6. On failure at any step, revert: remove `.storage.tmp/`, keep existing `storage/`

This mirrors the install binary pattern of writing to a temp sibling then renaming.

### Decision: `octx sync` uses `update.lock` for concurrency
The same file lock at `{data_dir}/octx/update.lock` serializes both `octx sync` and `octx update`. This prevents simultaneous sync and update from clobbering each other's state.

### Decision: Storage sync failure is reported, not fatal to update
If the storage sync fails during `octx update`, the error is printed to stderr but the update continues to its later phases (self-update). This follows the principle that a stale storage directory is better than no update at all.

### Decision: Release pipeline builds `storage.tar.gz` from `storage/` dir contents
A step in `.github/workflows/release.yml` runs `tar -czf storage.tar.gz -C storage .` from the repo root (note `-C storage .` archives the **contents**, so top-level entries like `harnesses/` land at the archive root) and attaches it to the release. This matches the extraction target `{data_dir}/octx/storage/` — without `-C storage .`, extraction would double-nest into `{data_dir}/octx/storage/storage/`. The `registry-index.json` is updated with the archive URL, checksum, and version (same as the head version).

## Risks / Trade-offs

- [Size] `storage.tar.gz` adds to the release download size. Mitigation: the storage directory is expected to be small (KB, not MB) — YAML and script files compress well. If it grows large, sync can be made selective.
- [Staleness] If a user runs `octx sync` infrequently, harnesses may be out of date. Mitigation: `octx update` always runs sync, and `octx x harness` could print a "last synced" warning (future).
- [Extraction failure] A corrupted archive could leave a partial tree. Mitigation: temp dir + atomic rename guarantees either the old or new tree is complete.
- [Bandwidth] ETag caching keeps repeated syncs lightweight. Only the first sync or a changed archive triggers a download.