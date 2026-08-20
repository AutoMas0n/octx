# octx — Octopus CLI (shared project context)

## What is octx?

A CLI tool (the "head") that discovers, installs (JIT), and dispatches standalone binary "arms" — single-purpose tools like `fmt`, `parse`, `deploy`. Arms come from a built-in registry or from any GitHub repo (`octx install github.com/user/repo`). The head also manages skill files for AI agent integration, encrypted credentials, shell PATH setup, and self-update.

## Monorepo layout

```
octx/
├── Cargo.toml              # Root: [package] name = "octx" + [workspace]
├── src/                    # Head source (the octx binary)
│   ├── main.rs             # Thin entrypoint
│   ├── lib.rs              # Re-exports
│   └── *.rs                # One module per concern
├── arms/                   # Workspace members (each is a separate [[bin]])
│   ├── fmt/
│   │   ├── Cargo.toml
│   │   ├── src/main.rs
│   │   └── skill.md        # AI agent skill definition
│   ├── parse/
│   └── deploy/
├── registry-index.json     # Published with releases (registry manifest)
├── install.sh              # One-liner installer script
├── CONTEXT.md              # ← you are here
├── AGENTS.md               # Project-specific preferences
├── ARCHITECTURE.md          # Design reference (read only when needed)
└── TASKS/                  # One file per independently executable task
```

## Cross-platform paths (dirs crate)

```rust
use dirs::{data_dir, config_dir};

// Data: binaries, cache, state
// Linux:   ~/.local/share/octx/
// macOS:   ~/Library/Application Support/octx/
// Windows: C:\Users\<user>\AppData\Local\octx\
let octx_data = data_dir().unwrap().join("octx");

// Config: config.toml, creds.enc, skills/
// Linux:   ~/.config/octx/
// macOS:   ~/Library/Application Support/octx/
// Windows: C:\Users\<user>\AppData\Roaming\octx\
let octx_config = config_dir().unwrap().join("octx");
```

## Data directory layout

```
{data_dir}/octx/
├── bin/                    # Flat: one file per arm (no octx- prefix)
│   ├── fmt
│   ├── parse
│   └── deploy
├── registry-index.json     # Cached registry manifest
├── installed-manifest.json # What's installed, versions, sources
├── update.lock             # File lock for concurrent safety
└── *.etag                  # HTTP ETag cache alongside each downloaded file
```

## Config directory layout

```
{config_dir}/octx/
├── config.toml             # Optional settings (registry URL override, agent links)
├── creds.enc               # Encrypted credentials (machine-ID key, chmod 600)
├── creds.lock              # File lock for concurrent access
└── skills/                 # Skill .md files, one per installed arm
```

## Registry index format (registry-index.json)

```json
{
  "registry_version": 1,
  "updated": "2025-08-19T00:00:00Z",
  "head": {
    "version": "0.1.0",
    "etag": "\"abc123def\"",
    "downloads": {
      "x86_64-unknown-linux-musl": {
        "url": "https://github.com/AutoMas0n/octx/releases/download/v0.1.0/octx-x86_64-unknown-linux-musl.gz",
        "sha256": "abc123..."
      },
      "armv6-unknown-linux-gnueabihf": {
        "url": "https://github.com/AutoMas0n/octx/releases/download/v0.1.0/octx-armv6-unknown-linux-gnueabihf.gz",
        "sha256": "def456..."
      }
    }
  },
  "arms": {
    "fmt": {
      "description": "Opinionated code formatter",
      "repository": "https://github.com/AutoMas0n/octx",
      "skill_url": "https://github.com/AutoMas0n/octx/releases/download/v0.1.0/fmt.skill.md",
      "versions": {
        "0.1.0": {
          "downloads": {
            "x86_64-unknown-linux-musl": {
              "url": "https://github.com/AutoMas0n/octx/releases/download/v0.1.0/fmt-x86_64-unknown-linux-musl.gz",
              "sha256": "abc123..."
            },
            "armv6-unknown-linux-gnueabihf": {
              "url": "https://github.com/AutoMas0n/octx/releases/download/v0.1.0/fmt-armv6-unknown-linux-gnueabihf.gz",
              "sha256": "def456..."
            }
          }
        }
      }
    }
  }
}
```

Default fetch URL (compile-time constant, overridable in config):
```
https://github.com/AutoMas0n/octx/releases/latest/download/registry-index.json
```

## Installed manifest format (installed-manifest.json)

```json
{
  "version": 1,
  "arms": {
    "fmt": {
      "source": "registry",
      "source_url": "https://github.com/AutoMas0n/octx",
      "version": "0.1.0",
      "checksum": "sha256:abc123...",
      "installed_at": "2025-08-20T00:00:00Z"
    },
    "deploy": {
      "source": "remote",
      "source_url": "github.com/AutoMas0n/octx-deploy",
      "version": "latest",
      "checksum": null,
      "installed_at": "2025-08-20T01:00:00Z"
    }
  }
}
```

## Config file format (config.toml)

```toml
# {config_dir}/octx/config.toml
registry_url = "https://github.com/AutoMas0n/octx/releases/latest/download/registry-index.json"
noninteractive = false

[links]
# Agent directories to sync skill files to
# "pi" = "/home/user/.pi/agent/skills"
# "claude" = "/home/user/.claude/skills"
```

## Creds schema (decrypted content)

```json
{
  "github": { "token": "ghp_abc123..." },
  "gitlab": { "token": "glpat_..." },
  "github.my-org": { "token": "ghp_def456..." }
}
```

Encrypted at `{config_dir}/octx/creds.enc` using machine-ID derived key (argon2 + AES-256-GCM).

## Arm contract

Every arm is a standalone binary:
- Named `<name>` (no `octx-` prefix) in `{data_dir}/octx/bin/`
- Accepts `--help` and `--version` (free with clap)
- Exits 0 on success, non-zero on failure
- stdout = output, stderr = diagnostics
- Input from stdin (for pipelines)
- Can receive tokens via `OCTX_TOKEN_<HOST>` env vars (injected by the head)

## Code quality

Every module follows TDD: RED → GREEN → REFACTOR.
- Write tests first (they fail — RED)
- Implement the minimum to pass (GREEN)
- Refactor without changing tests (REFACTOR)
- `cargo fmt` before any commit
- `cargo clippy -- -D warnings` — deny warnings
- No `unwrap()` in library code — use `?` with proper error types
- No `dbg!()` in committed code
- Tests in `src/` (unit) and `tests/` (integration)

## Build commands

```bash
cargo check                # Fastest feedback loop
cargo test                 # Run all tests
cargo build --release      # Final binary
cargo clippy -- -D warnings
cargo fmt
```

## Task dependency graph

```
T0: paths.rs      ─┐
T1: error.rs      ─┤
T2: platform.rs   ─┤       T6: registry.rs ─┐
T3: config.rs     ─┤       T7: install.rs  ──┤
T4: manifest.rs   ─┤       T8: dispatch.rs ──┤
T5: util.rs       ─┘       T9: creds.rs    ──┤
                           T10: skills.rs  ──┤    T13: cli.rs → T14: main.rs
                           T11: init.rs    ──┤
                           T12: update.rs  ──┤
                                            T15: arms (workspace members)
                                            T16: release.yml
                                            T17: install.sh
```

Tasks 0–5 have no deps (foundational). Tasks 6–12 depend on one or more of 0–5. Task 13 (cli.rs) wires every subcommand together and depends on all prior modules. Task 14 (main.rs) is thin and depends only on cli. Tasks 15–17 are independent.

## Current state

- `Cargo.toml`: has root deps (clap, anyhow, thiserror, tokio, reqwest, serde, sha2, dirs). Does NOT yet have `machine-uid`, `fs2`, `aes-gcm`, `argon2`, `keyring` — tasks needing them add them to Cargo.toml.
- `src/main.rs`: placeholder (prints hello)
- `src/lib.rs`: placeholder (OctxError with Generic variant, run() returning string, CliArgs stub)
- `src/`: no other modules exist yet
- `arms/`: does not exist yet
- `TASKS/`: current directory
- No workspace setup yet