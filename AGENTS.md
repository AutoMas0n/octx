# AGENTS.md — octx

## Project structure
- Use clap for argument parsing (the de facto standard)
- Use thiserror + anyhow for error handling (thiserror for library errors, anyhow for CLI binary top-level)
- Put library code in src/lib.rs, binary entrypoint in src/main.rs — keeps the logic testable
- Tests live in src/ (unit) and tests/ (integration)

## Code quality
- cargo fmt before every review/submit
- cargo clippy -- -D warnings — deny warnings in CI
- No unwrap() in library code — use ? with proper error types
- No dbg!() in committed code

## Release profile
- Start every CLI project with lto = true, strip = "symbols", codegen-units = 1 in [profile.release]
- Version follows semver: 0.1.0 -> 0.2.0 for new features, 0.1.1 for fixes

## Workflow
- cargo check for fastest feedback loop during development
- cargo test before merging/committing
- Keep main.rs thin — delegate logic to lib.rs
- Help text should be self-documenting: --help output is the primary docs

## Memory & skills
- Save tricky CLI patterns as a skill for reuse
- Project-specific preferences go in memory