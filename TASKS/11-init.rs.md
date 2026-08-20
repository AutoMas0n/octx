# Task 11: init.rs — Shell integration (PATH setup)

## Contract

```rust
/// Detect the user's shell from $SHELL env var and return the type.
pub fn detect_shell() -> ShellType

/// Get the path to the user's shell rc file.
pub fn rc_file_path(shell: &ShellType) -> PathBuf

/// Append the bin directory to the shell rc file (idempotent — checks for existing entry).
pub fn install_path_hook() -> Result<(), OctxError>

pub enum ShellType {
    Bash,
    Zsh,
    Fish,
    PowerShell,  // Windows
    Unknown(String),
}
```

## Dependencies

- Task 00 (paths.rs) — for bin_dir()
- Task 01 (error.rs)

## Tests (RED)

```
- test_detect_shell_defaults_to_unknown_when_no_env
- test_rc_file_path_bash_returns_dot_bashrc
- test_rc_file_path_zsh_returns_dot_zshrc
- test_rc_file_path_fish_returns_config_fish
- test_install_path_hook_adds_bin_dir_to_rc (with temp file mock)
- test_install_path_hook_is_idempotent (second call doesn't duplicate)
```

## Implementation notes

- `detect_shell()`: read `$SHELL` env var, check the filename for known patterns:
  - ends with `bash` → `ShellType::Bash`
  - ends with `zsh` → `ShellType::Zsh`
  - ends with `fish` → `ShellType::Fish`
  - contains `powershell` or `pwsh` → `ShellType::PowerShell`
  - default → `ShellType::Unknown(shell_path)`
- `rc_file_path()`:
  - Bash: `~/.bashrc`
  - Zsh: `~/.zshrc`
  - Fish: `~/.config/fish/config.fish`
  - PowerShell: PowerShell profile path (use `$PROFILE` semantics: `~\Documents\PowerShell\Microsoft.PowerShell_profile.ps1`)
  - Unknown: `~/.bashrc` (safe default)
- `install_path_hook()`:
  1. Detect shell
  2. Get rc file path
  3. Read rc file (if exists), check if `{data_dir}/octx/bin` is in `$PATH` line
  4. If not, append `export PATH="$PATH:{bin_dir}"` (Unix) or appropriate PowerShell syntax
  5. Print success message to stderr
- The `PATH=$PATH:{bin_dir}` line must be guarded — check if it already exists to avoid duplicates
- No sudo. Everything is user-level.

## Output

- Creates: `src/init.rs`
- Modifies: `src/lib.rs`
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all init tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] detect_shell() parses $SHELL into correct ShellType variant
- [ ] rc_file_path() returns known paths for bash/zsh/fish/pwsh
- [ ] install_path_hook() appends PATH line to rc file
- [ ] install_path_hook() is idempotent (second call doesn't duplicate)
