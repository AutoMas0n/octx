## Why

The octx head knows how to install compiled arm binaries from the registry, but has no mechanism for syncing non-binary assets (YAML, scripts, templates) that arms depend on at runtime. The `harness` arm needs a directory of harness definitions (YAML + scripts) to be present on disk, but these are not compiled binaries — they're source files that should be fetched from the same repo release. Without a sync mechanism, each arm would have to invent its own asset-fetching logic, duplicating effort and fragmenting the update story.

## What Changes

- **New `octx sync` subcommand** — downloads and extracts a `storage.tar.gz` archive from the latest release into `{data_dir}/octx/storage/`
- **`registry-index.json` gains a `storage` entry** — version, ETag, and a single platform-independent `download` (URL + SHA256)
- **`octx update` runs sync as part of its flow** — after arm/skill updates and before self-update, keeping storage assets current alongside arms and skills
- **`octx sync --force` flag** — re-download even if the ETag matches
- **Storage is a canonical read-only mirror** — `{data_dir}/octx/storage/` is octx-owned and replaced wholesale on sync; local edits are silently dropped, and user-authored copies live outside it (harnesses under `{config_dir}/harnesses/`)
- **Storage directory layout** — `{data_dir}/octx/storage/` mirrors the monorepo's `storage/` directory tree
- **Repo `storage/` directory** — seeded with a tracked `storage/.gitkeep` so the release archive has a directory to package before harness content (YAML + scripts) lands
- **Release pipeline builds `storage.tar.gz`** — archived from the `storage/` directory in the repo root (contents only, via `-C storage .`)

## Capabilities

### New Capabilities
- `storage-sync`: Mechanism for fetching, verifying, and extracting non-binary storage assets from the octx release registry. Covers the sync subcommand, ETag caching, atomic extraction, the registry storage entry, and update integration.

### Modified Capabilities
<!-- No existing specs to modify — this is a brand-new capability. -->
- None

## Impact

- **`src/registry.rs`** — RegistryIndex gains an optional `storage` field with version, etag, and a single platform-independent `download` (url + sha256)
- **`src/update.rs`** — Update phase adds storage sync after arm/skill updates and before self-update
- **`src/cli.rs`** — New `sync` subcommand in the clap definition
- **New module `src/sync.rs`** — Sync orchestration: conditional GET (ETag/304), checksum verify, atomic extract; owns the cached archive and its ETag
- **`Cargo.toml`** — Adds `flate2` and `tar` for gzip/tar extraction; everything else (tokio, reqwest, sha2, serde) is already present
- **`storage/.gitkeep`** — New tracked placeholder so the repo has a `storage/` directory to package before harness content lands
- **`.github/workflows/release.yml`** — New step `tar -czf storage.tar.gz -C storage .` (contents only) attached to the release, plus a `storage` entry in the generated `registry-index.json` `jq` block (the index is generated, not committed)