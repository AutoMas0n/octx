# octx — Task Verification Protocol

After implementing any task, run these checks in order. All must pass before reporting "done".

## Universal checks (every task)

```bash
# 1. Compiles without errors
cargo check 2>&1

# 2. All tests pass
cargo test 2>&1

# 3. No clippy warnings
cargo clippy -- -D warnings 2>&1

# 4. Code is formatted
cargo fmt --check 2>&1
```

If any of these fail, fix before proceeding.

## Task-specific checks

Each task file (TASKS/*.md) ends with a `## Definition of Done` section listing additional pass/fail checks specific to that module. Run those after the universal checks pass.

## What "done" means

- All universal checks pass ✅
- All task-specific checks pass ✅
- No `unwrap()` in library code (binary-only in main.rs if absolutely necessary)
- No `dbg!()` calls
- No compilation warnings
- Tests include the RED cases specified in the task

## Integration test (after all tasks 0–14)

Once all core modules are complete, run the full test suite and manually test:

```bash
# Full suite
cargo test

# Build release
cargo build --release

# Help text
./target/release/octx --help

# Smoke test: try to run an arm (will JIT-fail gracefully since no registry yet)
./target/release/octx x fmt --help
```

## Dogfooding checklist (after task 15+)

```bash
# Build the workspace (head + arms)
cargo build --release

# Verify arm binary is a standalone executable
./target/release/fmt --help

# Verify octx can find and dispatch to a pre-placed arm
mkdir -p ~/.local/share/octx/bin
cp target/release/fmt ~/.local/share/octx/bin/fmt
./target/release/octx x fmt --help   # should run the arm

# Verify ls shows installed arms
./target/release/octx ls

# Verify init adds PATH
./target/release/octx init
```