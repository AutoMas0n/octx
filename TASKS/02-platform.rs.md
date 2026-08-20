# Task 02: platform.rs — Target triple detection

## Contract

```rust
/// Detects the current platform's target triple for downloading the correct binary.
/// Returns strings like "x86_64-unknown-linux-musl", "armv6-unknown-linux-gnueabihf", etc.
pub fn detect() -> &'static str

/// Returns the machine ID for credential encryption (Linux: /etc/machine-id, etc.)
pub fn machine_id() -> Result<String, OctxError>
```

## Dependencies

- Task 01 (error.rs) — for `OctxError::UnsupportedPlatform`

## Tests (RED)

```
- test_detect_returns_non_empty_string
- test_detect_returns_expected_format (contains "linux" on linux)
- test_machine_id_returns_some_string (skipped if not root/no file)
```

## Implementation notes

- `detect()`: run `uname -m`, map to triple:
  - `x86_64` → `x86_64-unknown-linux-musl`
  - `aarch64` → `aarch64-unknown-linux-musl`
  - `armv6l` → `armv6-unknown-linux-gnueabihf`
  - `armv7l` → `armv7-unknown-linux-gnueabihf`
  - `arm64` (macOS) → `aarch64-unknown-linux-musl`
  - Anything else → `OctxError::UnsupportedPlatform`
- Cache result in a `OnceLock` or `std::sync::OnceLock` so it only runs once
- `machine_id()`: Read `/etc/machine-id` (trim whitespace). If missing, try `/var/lib/dbus/machine-id`
- macOS: parse `ioreg -rd1 -c IOPlatformExpertDevice` for `IOPlatformUUID`
- Windows: (stub for now — return `OctxError::UnsupportedPlatform("Windows machine ID not yet implemented")`)
- `ponytail: Windows machine ID is a stub. Implement when adding full Windows support.`

## Output

- Creates: `src/platform.rs`
- Modifies: `src/lib.rs` (add `pub mod platform;`)
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all platform tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `detect()` returns a valid triple string on the current machine
- [ ] `machine_id()` returns `Ok(id)` on Linux with `/etc/machine-id`
- [ ] Result is cached via `OnceLock` (second call doesn't run `uname` again)
