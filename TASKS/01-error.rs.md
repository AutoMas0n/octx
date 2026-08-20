# Task 01: error.rs — OctxError enum

## Contract

Define the error type used throughout the head:

```rust
#[derive(Error, Debug)]
pub enum OctxError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("Platform not supported: {0}")]
    UnsupportedPlatform(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("{0}")]
    Config(String),

    #[error("Credential error: {0}")]
    Creds(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}
```

Provide a helper for exit codes:

```rust
impl OctxError {
    /// Returns the appropriate exit code (0 = success, 1 = generic, 2 = not found, 3 = network)
    pub fn exit_code(&self) -> i32
}
```

## Dependencies

- None (foundational, uses only thiserror + std + reqwest)

## Tests (RED)

```
- test_error_display_io
- test_error_display_http
- test_error_display_checksum_mismatch
- test_exit_code_for_not_found_returns_2
- test_exit_code_for_network_returns_3
- test_error_can_be_converted_from_io_error_via_into
```

## Implementation notes

- Use `thiserror::Error` derive macro
- `reqwest::Error` and `serde_json::Error` use `#[from]` for auto-conversion
- `std::io::Error` uses `#[from]` as well
- Exit codes: generic=1, not_found=2, network=3, config=1, creds=1, platform=1
- Remove the placeholder `Generic(String)` variant from lib.rs — replace with this enum

## Output

- Creates: `src/error.rs`
- Modifies: `src/lib.rs` (add `pub mod error;`, remove old `pub enum OctxError`)
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all error tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `OctxError` has all required variants (IO, Http, Registry, Platform, Checksum, Config, Creds, NotFound, Network, Serde)
- [ ] Each variant has a useful `Display` message
- [ ] `exit_code()` returns correct codes (1 generic, 2 not-found, 3 network)
- [ ] `#[from]` conversions work for `std::io::Error`, `reqwest::Error`, `serde_json::Error`
