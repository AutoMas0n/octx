# Task 09: creds.rs — Encrypted credential storage

## Contract

```rust
/// Add a credential (encrypt and write to {config_dir}/octx/creds.enc)
pub fn add(host: &str, token: &str) -> Result<(), OctxError>

/// Get a credential (decrypt and return token). Prints to stdout if --raw.
pub fn get(host: &str, raw: bool) -> Result<(), OctxError>

/// Remove a credential
pub fn remove(host: &str) -> Result<(), OctxError>

/// List all stored hosts (tokens redacted)
pub fn list() -> Result<(), OctxError>

/// Read all credentials from the encrypted file.
fn read_creds() -> Result<HashMap<String, HashMap<String, String>>, OctxError>

/// Write credentials to the encrypted file.
fn write_creds(creds: &HashMap<String, HashMap<String, String>>) -> Result<(), OctxError>

/// Derive a 256-bit encryption key from the machine ID + random salt.
fn derive_key(machine_id: &str, salt: &[u8]) -> Vec<u8>

/// Get a stored token by host (used internally by dispatch.rs for env var injection).
pub fn get_token(host: &str) -> Option<String>
```

## Dependencies

- Task 00 (paths.rs) — for config_dir()
- Task 01 (error.rs) — for OctxError
- Task 02 (platform.rs) — for machine_id()
- Task 05 (util.rs) — for FileLock
- Cargo deps: `machine-uid = "0.2"`, `aes-gcm = "0.10"`, `argon2 = "0.5"`

## Tests (RED)

```
- test_add_and_get_roundtrip
- test_remove_removes_entry
- test_get_returns_error_for_unknown_host
- test_list_returns_hosts_only
```

## Implementation notes

- File format: `{config_dir}/octx/creds.enc` — binary blob: salt(32) || nonce(12) || ciphertext
- Encryption flow:
  1. Get machine ID from `platform::machine_id()`
  2. Generate 32-byte random salt (`rand` crate, add to Cargo.toml: `rand = "0.8"`)
  3. Derive key: `argon2::Argon2::default().hash(machine_id.as_bytes(), &salt)` → 32 bytes
  4. Encrypt JSON-serialized creds with `AES-256-GCM`: `aes_gcm::Aes256Gcm::new_from_slice(&key)`
  5. Prepend nonce, append to salt
- `add()`: load existing creds, insert/update, write back. Uses `FileLock` for concurrency.
- `get(host, raw)`: if `raw`, print token to stdout. Otherwise print formatted message.
- `list()`: print each host (token is `"***"`)
- `get_token()`: internal, returns `Option<String>` — used by dispatch.rs to set `OCTX_TOKEN_<HOST>`
- Store format (JSON, decrypted): `{ "github": { "token": "..." }, ... }`
- Host sub-qualifiers with dots: `"github.my-org"` is a valid key

## Output

- Creates: `src/creds.rs`
- Modifies: `src/lib.rs`, `Cargo.toml` (add `machine-uid`, `aes-gcm`, `argon2`, `rand`)
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all creds tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] add() + get() roundtrip: stored token matches decrypted token
- [ ] remove() deletes entry and subsequent get() fails
- [ ] list() shows hosts without exposing tokens
- [ ] get(host, --raw) prints plaintext token to stdout
- [ ] creds.enc exists and is binary (not plaintext)
