# Task 08: dispatch.rs — Arm discovery, JIT install, and execution

## Contract

```rust
/// Prepare and run an arm. JIT-installs if missing, then executes.
/// Never returns — calls std::process::exit() with the arm's exit code.
pub async fn run_arm(name: &str, args: &[String]) -> Result<(), OctxError>

/// Get the path to an arm binary if it exists.
pub fn find_arm(name: &str) -> Option<PathBuf>

/// Execute a binary with given args, passing through stdin/stdout/stderr.
/// Returns the exit code of the child process.
pub fn execute(bin_path: &Path, args: &[String]) -> Result<i32, OctxError>
```

## Dependencies

- Task 00 (paths.rs) — for bin_dir()
- Task 01 (error.rs)
- Task 07 (install.rs) — for from_registry()
- Task 03 (config.rs) — for is_noninteractive()

## Tests (RED)

```
- test_find_arm_returns_none_when_not_installed
- test_execute_propagates_exit_code (run "true" and "false" commands)
- test_execute_passthrough_stdin (echo piped through)
```

## Implementation notes

- `run_arm()`:
  1. Check `find_arm(name)`
  2. If not found, print message to stderr, call `install::from_registry(name).await`
  3. Call `execute(bin_path, args)`
  4. `std::process::exit(code)` — this function never normally returns
- `find_arm()`: check `bin_dir().join(name)` — return `Some(path)` if file exists and is executable (unix) / exists (windows)
- `execute()`: `std::process::Command::new(bin_path)` with inherited stdio, `.status()`, map exit code
- For tests: use `/bin/true` and `/bin/false` or equivalent
- Stderr messages during JIT install: `"octx: installing '{name}' (latest)..."` and `"  ✓ {name} installed"`
- Do NOT prompt the user — install silently. `OCTX_NONINTERACTIVE` env var is irrelevant here (it's always non-interactive for JIT)

## Output

- Creates: `src/dispatch.rs`
- Modifies: `src/lib.rs`
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all dispatch tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] find_arm() returns None when arm not in bin_dir()
- [ ] execute() propagates exit code (test with /bin/true and /bin/false)
- [ ] run_arm() is async and never returns (calls std::process::exit)
- [ ] Stderr messages printed during JIT install are informative
