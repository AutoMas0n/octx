# Task 07: install.rs — Download, verify, and install arms and skills

## Contract

```rust
/// Install an arm from the registry by name (binary + skill file).
/// Downloads, verifies checksum, places binary in {data_dir}/octx/bin/<name>,
/// and skill file in {config_dir}/octx/skills/<name>.md.
pub async fn from_registry(name: &str) -> Result<(), OctxError>

/// Install an arm from a remote GitHub repo URL.
/// URL format: "github.com/owner/repo"
/// Binary name derived from repo name (strip "octx-" prefix if present).
/// Supports optional `--bin` override.
pub async fn from_remote(url: &str, bin_name: Option<&str>) -> Result<(), OctxError>

/// Construct a predictable GitHub release download URL.
pub fn construct_remote_url(host: &str, owner: &str, repo: &str, bin: &str, target: &str) -> String

/// Parse a remote URL into its components.
pub fn parse_remote_url(url: &str) -> Result<(String, String, String), OctxError>

/// Download bytes from a URL with optional Authorization header and ETag caching.
/// Returns (bytes, was_cached).
pub async fn fetch(url: &str, etag_path: &Path) -> Result<(Vec<u8>, bool), OctxError>

/// Download a skill file from a URL and save to {config_dir}/octx/skills/<name>.md
pub async fn install_skill(name: &str, skill_url: &str) -> Result<(), OctxError>
```

## Dependencies

- Task 00 (paths.rs)
- Task 01 (error.rs)
- Task 02 (platform.rs) — for target triple
- Task 05 (util.rs) — for install_binary, verify_checksum
- Task 06 (registry.rs) — for RegistryIndex::resolve_arm

## Tests (RED)

```
- test_parse_remote_url_parses_github
- test_parse_remote_url_errors_on_bad_format
- test_construct_remote_url_produces_expected_url
- test_parse_remote_url_strips_octx_prefix
- test_parse_remote_url_keeps_non_prefix_name
```

## Implementation notes

- `from_registry()`: resolve URL + checksum from registry, `fetch()`, `verify_checksum()`, `install_binary()`, then `install_skill()`
- `from_remote()`: parse URL, detect platform, construct release URL, fetch (no checksum), install binary
- `parse_remote_url()`: split by `/` — expect 3 parts: `github.com`, `owner`, `repo`. Validate.
- `construct_remote_url()`: `https://{host}/{owner}/{repo}/releases/latest/download/{bin}-{target}.gz`
- `fetch()`: reqwest GET. If etag_path exists, read it and send `If-None-Match`. Store response ETag on success. Return `(bytes, true)` on 304.
- `install_skill()`: fetch from `skill_url`, write to `{config_dir}/octx/skills/{name}.md`
- Do NOT add test that makes real HTTP calls — use mock data or test in isolation
- Remote install has no checksum verification — `ponytail: no checksum for remote installs, add when we have a checksum convention`

## Output

- Creates: `src/install.rs`
- Modifies: `src/lib.rs`
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all install tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] parse_remote_url() correctly parses github.com/owner/repo
- [ ] parse_remote_url() errors on invalid format
- [ ] construct_remote_url() produces expected URL format
- [ ] fetch() downloads bytes (test with a local HTTP mock or just parsing)
- [ ] install_skill() writes .md file to skills_dir()
