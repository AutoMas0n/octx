# Task 14: main.rs — Binary entrypoint

## Contract

The thinnest possible entrypoint. Parse args, run, handle errors.

```rust
#[tokio::main]
async fn main() {
    if let Err(e) = octx::cli::run().await {
        eprintln!("error: {}", e);
        std::process::exit(e.exit_code());
    }
}
```

## Dependencies

- Task 13 (cli.rs) — the only import

## Tests (RED)

- No unit tests for main.rs — too thin. Integration tests in `tests/` cover the CLI interface.

## Implementation notes

- Replace the current `main.rs` entirely (the placeholder that prints hello)
- Remove old `CliArgs` struct and `run()` function from `lib.rs` — they're replaced by `cli.rs`
- `lib.rs` should just re-export the `cli` module and any other public API needed for testing
- `#[tokio::main]` sets up the Tokio runtime for async operations
- Exit code comes from `OctxError::exit_code()`
- All logic lives in `cli.rs` — main.rs contains no business logic

## Output

- Modifies: `src/main.rs` (full rewrite)
- Modifies: `src/lib.rs` (remove old placeholder code, keep re-exports)
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — passes (even if no main.rs tests)
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] main.rs is thin: parse args, run, print error + exit
- [ ] lib.rs exports cli module and has no placeholder code
- [ ] #[tokio::main] is present
- [ ] Exit code from OctxError::exit_code() is used
