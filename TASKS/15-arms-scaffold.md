# Task 15: Arms scaffold — Workspace members + Cargo.toml setup

## Contract

Set up the monorepo workspace and create the first arm scaffold.

## Root Cargo.toml changes

```toml
[workspace]
members = ["arms/*"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
clap = { version = "4", features = ["derive"] }
anyhow = "1"
```

Note: the `[package]` section for `octx` (the head) stays in the root Cargo.toml — it is both the root package AND the workspace root. The `[workspace]` section is added alongside it.

## arms/fmt/ structure

```
arms/fmt/
├── Cargo.toml
├── src/main.rs
└── skill.md
```

### arms/fmt/Cargo.toml

```toml
[package]
name = "octx-fmt"
version.workspace = true
edition.workspace = true

[[bin]]
name = "fmt"
path = "src/main.rs"

[dependencies]
clap.workspace = true
anyhow.workspace = true

[profile.release]
opt-level = "z"
lto = true
strip = "symbols"
codegen-units = 1
panic = "abort"
```

### arms/fmt/src/main.rs

A minimal binary that does one useful thing. For the first arm, implement a "format checker" that reads a file and checks line length:

```rust
use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "fmt", about = "Opinionated code formatter")]
struct Args {
    files: Vec<String>,
    #[arg(long)]
    check: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.files.is_empty() {
        anyhow::bail!("no files specified");
    }
    for path in &args.files {
        check_line_lengths(path, 100)?;
    }
    Ok(())
}
```

Complete this with a `check_line_lengths()` that reads lines and reports any > 100 chars. Exit 1 if any line is too long. Print nothing on success (silent = formatted correctly). Print violations to stdout.

### arms/fmt/skill.md

```markdown
# fmt

## Description
Opinionated code formatter. Checks source file line lengths and formatting conventions.

## Usage
```
octx x fmt [files...] [--check]
```

## Examples
- `octx x fmt src/` — format all files in src/
- `octx x fmt --check src/` — check if files are formatted (exit 1 if not)

## Environment
- `OCTX_TOKEN_GITHUB` — injected when arm is spawned via `octx x`
```

## Dependencies

- Task 14 (main.rs) must be complete so the workspace structure is finalized

## Tests (RED)

- `cargo build -p octx-fmt` must compile
- The arm binary `fmt` must accept `--help` and `--version`
- `fmt --check` returns 0 for clean files, 1 for violations

## Output

- Modifies: `Cargo.toml` (add `[workspace]` section)
- Creates: `arms/fmt/Cargo.toml`, `arms/fmt/src/main.rs`, `arms/fmt/skill.md`
- Creates (optional stub): `arms/parse/` and `arms/deploy/` directories with minimal Cargo.toml (no src needed yet)
## Definition of Done
- [ ] `cargo check` compiles the entire workspace
- [ ] `cargo test` — passes
- [ ] `cargo build -p octx-fmt --release` produces a binary
- [ ] `./target/release/fmt --help` works
- [ ] `./target/release/fmt --version` works
- [ ] `fmt --check` returns 0 for clean files, 1 for violations
- [ ] skill.md exists with Description, Usage, Examples, Environment sections
- [ ] arms/fmt/Cargo.toml uses opt-level = z, lto, strip, panic = abort
