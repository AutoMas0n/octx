# Task 12: update.rs — Update everything (arms, skills, self)

## Contract

```rust
/// Update all installed arms, sync skills, and self-update octx.
/// Concurrent across arms (worker-pooled), ETag-cached, race-free.
pub async fn run() -> Result<(), OctxError>

/// Update a single registry arm (binary + skill).
async fn update_registry_arm(name: &str, index: &RegistryIndex) -> Result<(), OctxError>

/// Update a single remote arm.
async fn update_remote_arm(name: &str, host: &str, owner: &str, repo: &str) -> Result<(), OctxError>

/// Self-update: check registry for newer octx binary, download, atomically replace.
async fn self_update() -> Result<(), OctxError>

/// Number of concurrent workers based on available parallelism (capped at 16).
fn worker_count() -> usize
```

## Dependencies

- Task 01 (error.rs)
- Task 02 (platform.rs) — for detect()
- Task 04 (manifest.rs) — for loading installed manifest
- Task 05 (util.rs) — for FileLock, install_binary, verify_checksum
- Task 06 (registry.rs) — for fetching index, resolving arms + head
- Task 07 (install.rs) — for fetch(), from_remote(), construct_remote_url()
- Task 10 (skills.rs) — for sync_all()
- Task 09 (creds.rs) — for get_token()

## Tests (RED)

```
- test_worker_count_returns_at_least_1
- test_worker_count_returns_at_most_16
- test_update_empty_manifest_returns_ok
```

## Implementation notes

- `run()`:
  1. Acquire `FileLock` on `{data_dir}/octx/update.lock`
  2. Load manifest
  3. Fetch registry index (ETag-cached) — if 304 and no arms changed, much faster
  4. Concurrent phase: for each arm, spawn a Tokio task gated by `Semaphore::new(worker_count())`
     - Registry arm: `update_registry_arm(name, &index)`
     - Remote arm: `update_remote_arm(name, host, owner, repo)`
  5. Wait for all with `tokio::try_join_all`
  6. `Manifest::touch()` to update timestamp
  7. `skills::sync_all()` — Phase 2
  8. `self_update().await` — Phase 3 (last, after skills)
- `update_registry_arm()`: resolve URL+checksum from registry, `install::fetch()`, verify, `util::install_binary()`, fetch skill
- `update_remote_arm()`: construct release URL, fetch (no checksum), install binary
- `self_update()`:
  1. Get head entry from registry index (or fetch index if not loaded)
  2. Check `{data_dir}/octx/octx.etag` — skip if ETag matches
  3. Download new binary to `{data_dir}/octx/octx.new`
  4. Windows: rename current→.old, rename .new→current, delete .old
  5. Unix: rename .new over current binary (inode stays alive)
  6. Save new ETag
- `worker_count()`: `std::thread::available_parallelism()` → min(n, 16) → or 4 fallback
- `tokio::sync::Semaphore` gates concurrent downloads to match worker count
- If `self_update()` fails (permissions, disk), it's non-fatal — log to stderr, continue

## Output

- Creates: `src/update.rs`
- Modifies: `src/lib.rs`
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all update tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] worker_count() returns at least 1 and at most 16
- [ ] run() acquires file lock, processes arms, syncs skills, self-updates
- [ ] self_update() checks ETag before downloading
- [ ] update is concurrent (Semaphore-gated, try_join_all)
