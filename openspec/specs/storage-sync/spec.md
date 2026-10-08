# storage-sync Specification

## Purpose

Lets the octx head keep non-binary assets (harness YAML, scripts, templates) on the local machine in sync with the latest octx release, so arms can resolve runtime assets from a well-known directory.

## Requirements

### Requirement: Storage assets are distributed as a release archive
The release pipeline SHALL produce a `storage.tar.gz` archive containing the **contents** of the monorepo's `storage/` directory (top-level entries such as `harnesses/`, not a nested `storage/` directory), and the archive SHALL be attached to the GitHub release alongside the head binary and arm binaries. The registry index SHALL contain a `storage` entry with the archive version, download URL, and SHA256 checksum.

#### Scenario: Registry index advertises storage
- **WHEN** the user runs `octx sync` with a registry index that contains a `storage` entry
- **THEN** the storage archive URL, version, and checksum are available for download

#### Scenario: Registry index has no storage entry
- **WHEN** the user runs `octx sync` against a registry index without a `storage` entry
- **THEN** sync is a no-op that reports nothing to sync and exits successfully

### Requirement: Sync downloads and extracts storage
`octx sync` SHALL download the storage archive from the release URL, verify its SHA256 against the registry checksum, and extract it into `{data_dir}/octx/storage/`, replacing any existing content atomically. Extraction MUST NOT leave a partial or corrupted storage directory on failure.

#### Scenario: Successful sync
- **WHEN** the user runs `octx sync` and the download and checksum succeed
- **THEN** the storage archive is extracted to `{data_dir}/octx/storage/` and the directory tree mirrors the repo's `storage/` directory

#### Scenario: Checksum mismatch
- **WHEN** the downloaded archive's SHA256 does not match the registry checksum
- **THEN** sync fails with a non-zero exit code and the existing storage directory is left untouched

#### Scenario: Interrupted extraction
- **WHEN** extraction fails partway through
- **THEN** the previous storage directory is restored and sync exits non-zero

### Requirement: Sync is ETag-cached
`octx sync` SHALL store the ETag from the archive response alongside the cached archive. When a sync runs again and the server returns 304 Not Modified, sync SHALL skip re-extraction and report that storage is already current.

#### Scenario: No changes since last sync
- **WHEN** the user runs `octx sync` and the server returns 304 Not Modified
- **THEN** sync reports storage is current and does not re-extract

#### Scenario: Forced re-sync
- **WHEN** the user runs `octx sync --force`
- **THEN** the archive is re-downloaded and re-extracted regardless of ETag

### Requirement: Update runs sync automatically
`octx update` SHALL run a storage sync as part of its update flow, after arm/skill updates and before self-update. A storage sync failure MUST NOT silently pass; it SHALL be reported, but it MUST NOT prevent other update phases from completing.

#### Scenario: Update with storage changes
- **WHEN** the user runs `octx update` and the release has a newer storage archive
- **THEN** the storage directory is updated as part of the update

#### Scenario: Update when storage sync fails
- **WHEN** the user runs `octx update` and the storage sync fails
- **THEN** update reports the storage failure and continues the remaining phases

### Requirement: Storage directory layout
`{data_dir}/octx/storage/` SHALL mirror the monorepo's `storage/` directory tree exactly, preserving subdirectory names and file names as they exist in the repo.

#### Scenario: Harness directory present after sync
- **WHEN** a sync completes and the repo's `storage/` contains `harnesses/develop-arm/`
- **THEN** `{data_dir}/octx/storage/harnesses/develop-arm/` exists with the same files

### Requirement: Storage is a canonical read-only mirror
`{data_dir}/octx/storage/` SHALL be an octx-owned canonical mirror of the repo's `storage/` tree and SHALL be treated as read-only. Sync SHALL replace the directory contents wholesale rather than merging with or preserving local modifications: content present locally but absent from the archive SHALL be removed, and modified files SHALL be overwritten. The mirror SHALL reflect only the release that is current at sync time — no version history, pinning, or rollback is retained. User-authored content belongs outside this directory (for harnesses, under `{config_dir}/harnesses/`).

#### Scenario: Local edit is dropped on next sync
- **WHEN** a user edits or adds a file under `{data_dir}/octx/storage/` and then runs `octx sync`
- **THEN** the edit is overwritten or removed by the freshly extracted archive, with no backup and no warning

#### Scenario: Stale content removed
- **WHEN** an asset that existed in a previous release is no longer present in the current release archive
- **THEN** it no longer exists under `{data_dir}/octx/storage/` after sync

#### Scenario: Mirror reflects only the current release
- **WHEN** a sync completes against release N after previously syncing release N-1
- **THEN** `{data_dir}/octx/storage/` contains only the release N tree — no N-1 content is retained

### Requirement: Concurrent sync safety
Concurrent `octx sync` invocations SHALL be serialized by a file lock so two processes cannot corrupt the storage directory simultaneously.

#### Scenario: Two syncs at once
- **WHEN** two `octx sync` processes run concurrently
- **THEN** one waits for the other's lock, then reads the fresh state and either skips or re-extracts accordingly
