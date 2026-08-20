# Task 05: util.rs — Shared utility functions

## Contract

```rust
/// Compute SHA-256 hex digest of a byte slice
pub fn sha256_digest(data: &[u8]) -> String

/// Acquire an exclusive file lock at path. Release on Drop.
pub struct FileLock {
    file: std::fs::File,
    path: PathBuf,
}
impl FileLock {
    /// Create or open file at path, acquire exclusive lock (blocking).
    pub fn acquire(path: &Path) -> Result<FileLock, OctxError>
}

/// Atomically install a binary: write to temp, set permissions, rename into place.
/// source: where the downloaded bytes are
/// dest: final path in {data_dir}/octx/bin/<name>
pub fn install_binary(source: &Path, dest: &Path) -> Result<(), OctxError>

/// Verify that a file's SHA-256 matches the expected hex string.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<(), OctxError>

/// Create directory and all parents if they don't exist.
pub fn ensure_dir(path: &Path) -> Result<(), OctxError>
```

## Dependencies

- Task 01 (error.rs) — for `OctxError`

## Tests (RED)

```
- test_sha256_digest_returns_expected_hex
- test_file_lock_acquires_and_releases
- test_file_lock_from_two_processes_blocks (use std::process::Command or thread)
- test_install_binary_sets_permissions (unix: 0o755)
- test_verify_checksum_passes_on_match
- test_verify_checksum_fails_on_mismatch
- test_ensure_dir_creates_nested_dirs
- test_install_binary_is_atomic (partial write doesn't leave corrupt file)
```

## Implementation notes

- `sha256_digest`: use `sha2::Sha256` + `hex` crate (add `hex = "0.4"` to Cargo.toml)
- `FileLock`: use `fs2::FileExt::lock_exclusive()` — add `fs2 = "0.4"` to Cargo.toml
- `install_binary`: write to `dest.with_extension("tmp")`, set permissions, `std::fs::rename` to `dest` (atomic on same filesystem)
- Unix permissions: `std::os::unix::fs::PermissionsExt::from_mode(0o755)`
- Windows: skip permission setting (all files are executable by default)
- `verify_checksum`: read file, hash, compare lowercased hex strings
- `ensure_dir`: `std::fs::create_dir_all()`

## Output

- Creates: `src/util.rs`
- Modifies: `src/lib.rs`, `Cargo.toml` (add `hex`, `fs2`)
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all util tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `sha256_digest()` returns correct hex for known input
- [ ] `FileLock::acquire()` blocks concurrent processes (test via thread)
- [ ] `install_binary()` creates file with 0o755 on Unix
- [ ] `verify_checksum()` passes for matching hash, fails for mismatch
- [ ] `ensure_dir()` creates nested directories
