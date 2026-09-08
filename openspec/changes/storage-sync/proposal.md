## Why

The octx head knows how to install compiled arm binaries from the registry, but has no mechanism for syncing non-binary assets (YAML, scripts, templates) that arms depend on at runtime. The `harness` arm needs a directory of harness definitions (YAML + scripts) to be present on disk, but these are not compiled binaries — they're source files that should be fetched from the same repo release. Without a sync mechanism, each arm would have to invent its own asset-fetching logic, duplicating effort and fragmenting the update story.

## What Changes

- **New `octx sync` subcommand** — downloads and extracts a `storage.tar.gz` archive from the latest release into `{data_dir}/octx/storage/`
- **`registry-index.json` gains a `storage` entry** — version, URL, SHA256, and ETag for the storage archive
- **`octx update` runs sync as its final phase** — keeps storage assets current alongside arms and skills
- **`octx sync --force` flag** — re-download even if the ETag matches
- **Storage directory layout** — `{data_dir}/octx/storage/` mirrors the monorepo's `storage/` directory tree
- **Release pipeline builds `storage.tar.gz`** — archived from the `storage/` directory in the repo root

## Capabilities

### New Capabilities
- `storage-sync`: Mechanism for fetching, verifying, and extracting non-binary storage assets from the octx release registry. Covers the sync subcommand, ETag caching, atomic extraction, the registry storage entry, and update integration.

### Modified Capabilities
<!-- No existing specs to modify — this is a brand-new capability. -->
- None

## Impact

- **`src/registry.rs`** — RegistryIndex struct gains an optional `storage` field with version, downloads, etag
- **`src/update.rs`** — Update phase adds storage sync after arm update and before self-update
- **`src/cli.rs`** — New `sync` subcommand in the clap definition
- **`src/install.rs`** — Reuses `fetch_binary` / `fetch_with_cache` machinery for the storage tarball
- **`src/manifest.rs`** — InstalledManifest optionally tracks last storage sync version/etag
- **New module `src/sync.rs`** — Sync orchestration: download, verify, extract, manifest tracking
- **`.github/workflows/release.yml`** — New step: `tar -czf storage.tar.gz storage/` and attach to release
- **`registry-index.json`** — New `storage` entry alongside `head` and `arms`
- No new external dependencies — reuses tokio, reqwest, sha2, serde that are already in Cargo.toml