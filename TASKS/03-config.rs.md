# Task 03: config.rs — Config file reader/writer

## Contract

```rust
/// Represents the parsed config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub registry_url: Option<String>,        // defaults to DEFAULT_REGISTRY_URL
    pub noninteractive: Option<bool>,
    pub links: Option<HashMap<String, String>>,  // agent_name → directory path
}

impl Config {
    /// Load config from {config_dir}/octx/config.toml. Returns defaults if file missing.
    pub fn load() -> Result<Config, OctxError>

    /// Save config to {config_dir}/octx/config.toml
    pub fn save(&self) -> Result<(), OctxError>

    /// Get effective registry URL (override or default)
    pub fn registry_url(&self) -> String

    /// Get effective noninteractive setting
    pub fn is_noninteractive(&self) -> bool

    /// Add or update a link
    pub fn add_link(&mut self, agent_name: &str, path: &str)

    /// Remove a link
    pub fn remove_link(&mut self, agent_name: &str) -> bool
}

/// The default registry URL when config has no override
pub const DEFAULT_REGISTRY_URL: &str =
    "https://github.com/AutoMas0n/octx/releases/latest/download/registry-index.json";
```

## Dependencies

- Task 00 (paths.rs) — for `config_dir()`
- Task 01 (error.rs) — for `OctxError`

## Tests (RED)

```
- test_load_returns_defaults_when_no_config_exists
- test_load_parses_valid_config_toml
- test_registry_url_uses_override_when_set
- test_registry_url_falls_back_to_default
- test_add_link_inserts_into_links_map
- test_save_writes_valid_toml
- test_is_noninteractive_defaults_to_false
```

## Implementation notes

- Use `toml` crate for parsing/writing (add to Cargo.toml: `toml = "0.8"`)
- Config file is optional — if file doesn't exist, return Config with all None fields
- Serialize/deserialize with `serde`
- `save()` creates parent dirs if they don't exist
- Defaults on the getters (`registry_url()`, `is_noninteractive()`), not on the struct itself
- `links` map values are directory paths (e.g. `"pi" → "/home/user/.pi/agent/skills"`)

## Output

- Creates: `src/config.rs`
- Modifies: `src/lib.rs` (add `pub mod config;`), `Cargo.toml` (add `toml = "0.8"`)
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all config tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `load()` returns defaults when no config file exists
- [ ] `save()` writes valid TOML
- [ ] `add_link()` and `remove_link()` modify and persist links
- [ ] Config file is at `{config_dir}/octx/config.toml`
