# Task 00: paths.rs — Cross-platform directory resolution

## Contract

Expose two functions that return the canonical octx directories using the `dirs` crate:

```rust
/// Returns the data directory for binaries, cache, and state.
/// Linux:   ~/.local/share/octx/
/// macOS:   ~/Library/Application Support/octx/
/// Windows: C:\Users\<user>\AppData\Local\octx\
pub fn data_dir() -> PathBuf

/// Returns the config directory for config.toml, creds.enc, skills.
/// Linux:   ~/.config/octx/
/// macOS:   ~/Library/Application Support/octx/   (same as data on macOS)
/// Windows: C:\Users\<user>\AppData\Roaming\octx\
pub fn config_dir() -> PathBuf

/// Returns the bin directory where arm binaries are stored: {data_dir}/bin/
pub fn bin_dir() -> PathBuf

/// Returns the skills directory where skill files are stored: {config_dir}/skills/
pub fn skills_dir() -> PathBuf
```

## Dependencies

- None (foundational module)

## Tests (RED)

```
- test_data_dir_returns_path_ending_with_octx
- test_config_dir_returns_path_ending_with_octx
- test_bin_dir_is_under_data_dir
- test_skills_dir_is_under_config_dir
- test_data_dir_and_config_dir_are_different_paths_on_unix
```

## Implementation notes

- Use `dirs::data_dir()` and `dirs::config_dir()` — both return `Option<PathBuf>`
- Panic with a clear message if neither is available (extremely rare — only on bare-metal embedded or sandboxed environments)
- `bin_dir()` = `data_dir().join("bin")`
- `skills_dir()` = `config_dir().join("skills")`
- Ensure the returned paths use the OS-native separator

## Output

- Creates: `src/paths.rs`
- Modifies: `src/lib.rs` (add `pub mod paths;`)
## Definition of Done

- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all path tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] `data_dir()` returns `{something}/octx`
- [ ] `config_dir()` returns a path ending in `octx`, different from `data_dir()` on Linux
- [ ] `bin_dir()` is `data_dir()/bin`
- [ ] `skills_dir()` is `config_dir()/skills`
