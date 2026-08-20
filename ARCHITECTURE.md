# Architecture Plan: octx — Octopus CLI

## The Vision

```
octx/                          ← Monorepo: head + all arms
├── Cargo.toml                ← Root package (the head) + workspace root
├── src/                      ← Head source
├── arms/                     ← Each arm is a workspace member
│   ├── fmt/
│   ├── parse/
│   └── deploy/
└── ...
```

At runtime:

```
                         ┌──────────┐
                         │   octx   │  ← Head: CLI entrypoint, orchestration, lifecycle
                         │  (head)  │     <2 MB compiled (stripped, LTO, no debug)
                         └────┬─────┘
              ┌───────────────┼───────────────────┐
              │               │                   │
         ┌────┴────┐   ┌─────┴─────┐       ┌─────┴─────┐
         │  fmt    │   │  parse    │  ...   │  deploy   │  ← Arms: standalone binaries
         │ (arm)   │   │ (arm)     │       │ (arm)     │     stored in {data_dir}/octx/bin/<name>
         └─────────┘   └───────────┘       └───────────┘
```

Arms come from two sources:
1. **Registry** — the octx monorepo's built-in arms (tagged releases)
2. **Remote** — any GitHub repo that publishes release binaries (like `go install`)

---

## 1. Core Principles

| Principle | Rationale |
|-----------|-----------|
| **Async runtime (Tokio)** | Decision. Stripped debug + LTO keeps binary under 2MB. |
| **Static link musl** | Single binary, any Linux. Avoid glibc version hell. |
| **Arms as standalone binaries** | Subprocess dispatch robust. No .so across Rust versions. |
| **JIT pull, not pre-bundle** | Core ships once. Arms fetched on first use. |
| **opt-level = "z" for arms** | Size-optimized compilation. Smaller downloads, less disk. |
| **No registry server needed** | Static JSON manifest on GitHub Releases. |
| **Arms self-document** | Each arm has its own `--help`. Head delegates fully. |
| **Cross-platform by default** | `dirs` crate maps XDG (Linux), ~/Library (macOS), Known Folders (Windows). |
| **Remote install** | `octx install github.com/user/repo` downloads pre-built binaries. |
| **No version pinning** | Always latest. `octx update` refreshes everything. YAGNI. |
| **Vendor-agnostic remote** | v1 parses GitHub URLs. Architecture supports any host. |
| **Security by default** | GitHub CodeQL + Dependabot + `cargo audit` in CI. |
| **Skills-first design** | Every arm ships with an AI skill file. `octx update` syncs skills to any agent vendor via symlinks/hardlinks. Frictionless configuration. |
| **Self-updating** | `octx update` downloads the latest head binary and atomically replaces itself. No cargo, no rustup, no package manager — ever. |

---

## 2. Workspace Structure

```
octx/
├── Cargo.toml                # [package] octx (head) + [workspace]
├── src/                      # Head source
│   ├── main.rs
│   ├── lib.rs
│   ├── cli.rs
│   ├── dispatch.rs
│   ├── install.rs
│   ├── init.rs
│   ├── creds.rs
│   ├── config.rs
│   ├── registry.rs
│   ├── platform.rs
│   ├── manifest.rs
│   ├── update.rs
│   ├── error.rs
│   └── util.rs
├── arms/                     # Workspace members
│   ├── fmt/
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   ├── parse/
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── deploy/
│       ├── Cargo.toml
│       └── src/main.rs
├── arms/fmt/skill.md          # AI skill file for the fmt arm
├── registry-index.json       # Deployed with each release
├── install.sh                # One-liner installer script
└── .github/workflows/
    └── release.yml
```

Root `Cargo.toml`:
```toml
[package]
name = "octx"
version = "0.1.0"
edition = "2024"

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

Each arm's `Cargo.toml`:
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

---

## 3. Directory Layout (cross-platform via `dirs`)

```rust
use dirs::{data_dir, config_dir};

// Data directory (binaries, cache, state)
// Linux:   ~/.local/share/octx/
// macOS:   ~/Library/Application Support/octx/
// Windows: C:\Users\<user>\AppData\Local\octx\
let octx_data = data_dir().unwrap().join("octx");

// Config directory (creds, config.toml)
// Linux:   ~/.config/octx/
// macOS:   ~/Library/Application Support/octx/
// Windows: C:\Users\<user>\AppData\Roaming\octx\
let octx_config = config_dir().unwrap().join("octx");
```

```
{data_dir}/octx/
├── bin/                     # Flat: one file per arm
│   ├── fmt
│   ├── parse
│   └── deploy
├── registry-index.json      # Cached registry manifest
└── installed-manifest.json  # What's installed, versions, sources
```

```
{config_dir}/octx/
├── config.toml              # Registry URL, agent links, etc.
├── creds.enc                # Encrypted credentials (chmod 600)
└── skills/                  # Skill files, one per installed arm
    ├── fmt.md
    ├── parse.md
    └── deploy.md
```

---

## 4. The Head (`octx` — root package)

### 4.1 Commands

```
octx                         # → top-level help
octx x <arm> [args...]       # → shorthand: dispatch to arm (JIT installs if missing)  ← PRIMARY
octx exec <arm> [args...]    # → explicit: same as `octx x`
octx install <arm>           # → install from registry
octx install github.com/user/repo  # → install from remote
octx uninstall <arm>
octx update                  # → update all installed arms + octx itself + sync skills
octx ls                      # → list installed arms (aliases: list, l)
octx search <query>          # → search registry
octx init                    # → shell integration (PATH, one-time)
octx creds add <host>        # → store auth token (e.g. github)
octx creds remove <host>
octx creds list
octx link <path>              # → register an agent skills directory (e.g. ~/.claude/skills)
octx link <path> --unlink    # → remove a registered link
octx links                   # → list all registered agent links
```

### 4.2 Internal modules

```
src/
├── main.rs                  # Thin: parse args, dispatch to lib
├── lib.rs                   # Re-exports
├── cli.rs                   # Clap definition (all subcommands)
├── dispatch.rs              # Arm discovery + execution
│   ├── find_arm(name)       # Check {data_dir}/octx/bin/<name>, prompt JIT if missing
│   └── execute(arm, args)   # Spawn subprocess, passthrough stdin/stdout/stderr, propagate exit code
├── install.rs               # Install from registry OR remote
│   ├── from_registry(name)  # Look up in registry-index.json, fetch binary + skill, verify
│   ├── from_remote(url)     # Parse URL, predict release asset URL, fetch + verify
│   ├── fetch(url, checksum) # HTTP GET, stream to temp file, verify sha256
│   ├── install_binary(path) # Copy to {data_dir}/octx/bin/<name>, chmod +x
│   └── install_skill(name)  # Download skill.md to {config_dir}/octx/skills/<name>.md
├── registry.rs              # Registry index operations
│   ├── fetch_index()        # Download from default URL or config override
│   ├── search(query)        # Search locally cached index
│   └── resolve(name)        # Get download URL for arm + current platform
├── manifest.rs              # Local installed-manifest.json read/write
├── update.rs                # Update all arms: re-download from same source
├── init.rs                  # Shell integration
│   ├── detect_shell()       # From $SHELL env var
│   ├── rc_file_path()       # .bashrc / .zshrc / config.fish / $PROFILE
│   └── install_path_hook()  # Append `{data_dir}/octx/bin` to shell rc
├── creds.rs                 # Auth token storage
│   ├── add(host, token)     # Encrypt + store in {config_dir}/octx/creds.enc
│   ├── get(host)            # Retrieve token
│   └── remove(host)
├── skills.rs                # Skill files + agent link management
│   ├── install_skill(name)  # Copy skill.md to {config_dir}/octx/skills/<name>.md
│   ├── sync_all()           # Symlink/hardlink skills to all linked agent dirs
│   ├── link_add(path)       # Register agent directory, create if missing
│   ├── link_remove(path)
│   └── link_list()
├── config.rs                # Config file read/write
│   ├── load()               # Parse {config_dir}/octx/config.toml, merge defaults
│   └── save()               # Write config (links, registry URL, etc.)
├── platform.rs              # Target triple detection
│   └── detect()             # uname -m → triple mapping table
├── error.rs                 # OctxError enum (thiserror)
└── util.rs                  # sha256, path helpers, temp file cleanup
```

### 4.3 Resource budget

| Metric | Target | Why |
|--------|--------|-----|
| Binary size | < 2 MB (stripped, LTO) | Fits on a 16MB /boot partition |
| RAM at rest | < 5 MB | Leaves 500MB+ for arms on Pi Zero |
| Cold start | < 100ms | Even on single-core ARMv6 |

### 4.4 Dependencies

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
anyhow = "1"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
dirs = "6"
machine-uid = "0.2"
fs2 = "0.4"
aes-gcm = "0.10"
argon2 = "0.5"
```

Tokio + reqwest (rustls). `dirs` for cross-platform paths. `machine-uid` + `argon2` + `aes-gcm` for encrypted creds. `fs2` for file locking. No openssl-sys.

---

## 5. Registry Index

### 5.1 URL (compile-time constant, config override)

```rust
const DEFAULT_REGISTRY_URL: &str = "https://github.com/AutoMas0n/octx/releases/latest/download/registry-index.json";
```

Override in `{config_dir}/octx/config.toml`:
```toml
registry_url = "https://my-mirror.example.com/registry-index.json"
```

### 5.2 Format

```json
{
  "registry_version": 1,
  "updated": "2025-08-19T00:00:00Z",
  "head": {
    "version": "0.1.0",
    "etag": ""abc123def"",
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

No version targeting — always installs the latest version listed in the index. `ponytail: version pinning is YAGNI, add when someone asks for it.`

---

## 6. Platform Detection

```rust
fn detect() -> &'static str {
    let arch = std::process::Command::new("uname")
        .arg("-m")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok());

    match arch.as_deref() {
        Some("x86_64")           => "x86_64-unknown-linux-musl",
        Some("aarch64")          => "aarch64-unknown-linux-musl",
        Some("armv6l")           => "armv6-unknown-linux-gnueabihf",
        Some("armv7l")           => "armv7-unknown-linux-gnueabihf",
        Some("arm64")            => "aarch64-unknown-linux-musl",  // macOS
        _ => panic!("unsupported architecture: {:?}", arch),
    }
}
```

`ponytail: limited to Linux/macOS triples. Add Windows and Android when platform support is needed.`

---

## 7. Remote Install

### 7.1 URL parsing (vendor-agnostic, v1 only GitHub)

```rust
struct RemoteSource {
    host: String,    // github.com
    owner: String,   // AutoMas0n
    repo: String,    // octx-fmt
    bin: String,     // fmt (derived or explicit)
}
```

Parsing: `github.com/AutoMas0n/octx-fmt` → `host=github.com`, `owner=AutoMas0n`, `repo=octx-fmt`.

Binary name derivation:
- Strip `octx-` prefix from repo name → `fmt`
- `--bin` flag overrides

### 7.2 Release URL construction

For GitHub, the predictable URL pattern:
```
https://github.com/{owner}/{repo}/releases/latest/download/{bin}-{target}.gz
```

No version targeting. `latest` is always fetched. `ponytail: version pinning YAGNI, add when someone asks.`

### 7.3 Checksum

For registry arms: SHA256 from registry-index.json, verified after download.

For remote arms: skip checksum for v1. `ponytail: no checksum for remote installs, add when we have a checksum convention.`

### 7.4 Auth

If the arm repo is private, `octx creds get github` provides a token. The head sets `Authorization: Bearer <token>` on the download request.

---

## 8. `octx creds` — Encrypted, Centrally Reusable Credential Storage

### 8.1 Design goals

1. **Encrypted at rest** — machine-ID derived key, not plaintext
2. **Portable** — works on Pi Zero (headless), containers, desktop, CI (no keyring daemon needed)
3. **Reusable by arms** — any arm reads tokens via `OCTX_TOKEN_<HOST>` env var
4. **Centralized** — one place for all auth tokens, shared across the whole tool suite

### 8.2 Encryption strategy

Machine-ID derived key:

1. Read machine ID:
   - Linux: `/etc/machine-id` (or `/var/lib/dbus/machine-id`)
   - macOS: `IOPlatformUUID` via `ioreg`
   - Windows: `MachineGuid` from registry
2. Generate random 32-byte salt (stored alongside ciphertext)
3. Derive 256-bit key via `argon2` (memory-hard, resists GPU cracking)
4. Encrypt creds payload with `AES-256-GCM` (authenticated encryption)
5. Store at `{config_dir}/octx/creds.enc`

```rust
fn encrypt_creds(creds: &Credentials, machine_id: &str) -> Result<Vec<u8>> {
    let salt = random_bytes(32);
    let key = argon2::hash(machine_id.as_bytes(), &salt, &params);
    let ciphertext = aes_256_gcm::encrypt(&key, &serde_json::to_vec(creds)?);
    Ok([&salt[..], &nonce[..], &ciphertext[..]].concat())
}

fn decrypt_creds(data: &[u8], machine_id: &str) -> Result<Credentials> {
    let (salt, nonce, ciphertext) = split(data);
    let key = argon2::hash(machine_id.as_bytes(), &salt, &params);
    let plaintext = aes_256_gcm::decrypt(&key, &nonce, &ciphertext)?;
    Ok(serde_json::from_slice(&plaintext)?)
}
```

### 8.3 File layout

```
{config_dir}/octx/
├── creds.enc       # Encrypted blob (chmod 600)
└── creds.lock      # File lock for concurrent access
```

Decrypted content (never written to disk):
```json
{
  "github": { "token": "ghp_abc123..." },
  "gitlab": { "token": "glpat_..." },
  "github.my-org": { "token": "ghp_def456..." }
}
```

### 8.4 Commands

```bash
octx creds add github          # prompts for token, encrypts, writes
octx creds add github --token ghp_abc123  # non-interactive
octx creds remove github
octx creds list                # shows hosts only, never tokens
octx creds get github          # prints token to stdout (for scripts)
octx creds get github --raw    # decrypt and print secret plaintext
```

### 8.5 Arms reusing creds

The head injects tokens as environment variables when spawning an arm:

```rust
// In dispatch.rs::execute()
if let Ok(token) = creds::get(&host) {
    cmd.env(format!("OCTX_TOKEN_{}", host.to_uppercase().replace('.', "_")), &token);
}
```

Arms simply read the env var — no credential logic needed in the arm:

```rust
let token = std::env::var("OCTX_TOKEN_GITHUB").ok();
```

This makes creds **centralized and reusable** across all tools.

### 8.6 Dependencies

Uses `machine-uid` crate for reading the machine ID. Argon2 + AES-256-GCM from `aes-gcm` and `argon2` crates.

```toml
machine-uid = "0.2"
aes-gcm = "0.10"
argon2 = "0.5"

---

## 9. Skills & Agent Integration

Every arm ships with a **skill file** — a markdown document that tells AI agents (Pi, Claude Code, etc.) how to naturally use the tool. Skills live in `{config_dir}/octx/skills/` and are synced to agent directories via `octx update`.

### 9.1 Skill file format

Each arm has a `skill.md` in the monorepo at `arms/<name>/skill.md`. Published alongside the binary in releases.

```markdown
# fmt

## Description
Opinionated code formatter. Formats source files according to project conventions.

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

### 9.2 `octx link <path>` — Registering agent directories

Links are stored in `{config_dir}/octx/config.toml`:

```toml
[links]
"pi" = "/home/user/.pi/agent/skills"
"claude" = "/home/user/.claude/skills"
```

```
octx link ~/.pi/agent/skills       # Register, create dir if missing
octx link ~/.claude/skills
octx link ~/.claude/skills --unlink  # Remove
octx links                           # List all
```

When a link is registered, `octx update` syncs all skill files to that directory.

### 9.3 Sync mechanism (per-platform)

| Platform | Method | Rationale |
|----------|--------|-----------|
| Linux / macOS | Symlink | `ln -s` — lightweight, automatic updates, no space duplication |
| Windows (user) | Hardlink + Junction | No admin required. Junction for dirs, hardlinks for files. |

```rust
fn sync_skill_to_linked_dir(skill_name: &str, source: &Path, target_dir: &Path) -> Result<()> {
    let target = target_dir.join(format!("{}.md", skill_name));

    #[cfg(unix)]
    {
        // Remove stale link/file, create symlink
        let _ = std::fs::remove_file(&target);
        std::os::unix::fs::symlink(source, &target)?;
    }

    #[cfg(windows)]
    {
        let _ = std::fs::remove_file(&target);
        std::fs::hard_link(source, &target)?;  // No admin needed
    }

    Ok(())
}
```

The `sync_all()` function in `skills.rs`:

```rust
fn sync_all() -> Result<()> {
    let config = Config::load()?;
    for (agent_name, agent_dir) in &config.links {
        ensure_dir_exists(agent_dir)?;
        for entry in std::fs::read_dir(skills_dir())? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".md") {
                let skill_name = name.trim_end_matches(".md");
                sync_skill_to_linked_dir(skill_name, &entry.path(), agent_dir)?;
            }
        }
    }
    Ok(())
}
```

### 9.4 `octx update` integration

Update becomes a two-phase operation:

```
octx update
  ├── Phase 1: Download latest binaries + skill files for every installed arm
  │     (concurrent, ETag-cached, worker-pooled)
  └── Phase 2: Sync skill files to all linked agent directories
        (symlinks/hardlinks, instant)
```

### 9.5 Agent vendor flexibility

The config is the contract. Any agent that reads skills from a directory is supported:

```toml
[links]
"pi" = "/home/user/.pi/agent/skills"
"claude" = "/home/user/.claude/skills"
"custom" = "/home/user/my-agent/skills"
```

Adding a new agent vendor = one `octx link <dir>` command. No code changes.

---

## 10. `octx update` — Fast, Concurrent, Race-Free Update

### 10.1 Design goals

1. **Concurrent** — all arms checked in parallel, not sequentially
2. **Worker pool** — scales to system resources, never overwhelms the machine
3. **ETag caching** — HTTP conditional requests skip unchanged arms entirely
4. **Race-free** — file lock prevents concurrent `update` processes from clobbering each other
5. **Light** — minimal overhead, fast even with many arms
6. **Skills synced** — after downloading, skill files are symlinked to every registered agent directory
7. **Self-update** — octx checks for a newer head binary and atomically replaces itself. Always the last phase so arm/skill updates aren't interrupted.

### 10.2 Self-update mechanism

```rust
async fn self_update() -> Result<()> {
    let current = std::env::current_exe()?;

    // Fetch registry index to get latest head version URL
    let index = Registry::fetch_index_cached().await?;
    let head_entry = index.head().ok_or(OctxError::NoHeadInRegistry)?;

    // ETag check — skip if already current
    let etag_path = data_dir().join("octx.etag");
    if let Ok(etag) = std::fs::read_to_string(&etag_path) {
        if etag == head_entry.etag {
            return Ok(());  // Already at latest
        }
    }

    // Download new binary
    let url = &head_entry.downloads[platform::detect()].url;
    let checksum = &head_entry.downloads[platform::detect()].sha256;
    let tmp = data_dir().join("octx.new");
    install::fetch(url, Some(checksum), &tmp).await?;

    // Atomic replace
    // Unix: rename over running binary (inode stays, process lives)
    // Windows: rename current → .old, rename new → current
    #[cfg(unix)]
    {
        std::fs::rename(&tmp, &current)?;
    }
    #[cfg(windows)]
    {
        let old = current.with_extension("old");
        let _ = std::fs::rename(&current, &old);      // release lock on current
        std::fs::rename(&tmp, &current)?;             // move new into place
        let _ = std::fs::remove_file(&old);
    }

    // Save new ETag
    if let Some(etag) = &head_entry.etag {
        std::fs::write(&etag_path, etag)?;
    }

    eprintln!("  ✓ octx updated to {}", head_entry.version);
    Ok(())
}
```

### 10.3 Worker pool sizing

```rust
use std::thread::available_parallelism;

fn worker_count() -> usize {
    // Default: number of logical CPUs
    // On Pi Zero (1 core): 1 worker
    // On desktop (8 cores): 8 workers
    // Capped at 16 to avoid connection pool exhaustion
    available_parallelism()
        .map(|n| n.get().min(16))
        .unwrap_or(4) // fallback if unavailable
}
```

The Tokio `tokio::sync::Semaphore` gates concurrent downloads:

```rust
async fn update_all() -> Result<()> {
    let manifest = Manifest::load()?;
    if manifest.arms.is_empty() {
        return Ok(());
    }

    let lock_path = data_dir().join("update.lock");
    let _lock = util::acquire_file_lock(&lock_path)?;

    let registry_index = Registry::fetch_index_cached().await?;
    let semaphore = Arc::new(Semaphore::new(worker_count()));

    let tasks: Vec<_> = manifest.arms.into_iter().map(|(name, entry)| {
        let sem = Arc::clone(&semaphore);
        let reg = registry_index.clone();
        async move {
            let _permit = sem.acquire().await?; // blocks if workers are saturated
            match entry.source {
                Source::Registry => update_registry_arm(&name, &reg).await,
                Source::Remote { host, owner, repo } => update_remote_arm(&name, &host, &owner, &repo).await,
            }
        }
    }).collect();

    try_join_all(tasks).await?;
    Manifest::touch()?;

    // Phase 2: Sync skill files to all linked agent directories
    // Phase 3: Self-update (last, so if it fails, arms and skills are still current)
    self_update().await?;
    skills::sync_all()?;

    Ok(())
}
```

### 10.4 ETag caching for all HTTP requests

Every outgoing HTTP request stores the `ETag` response header alongside the cached file:

```
{data_dir}/octx/
├── bin/fmt                    # Arm binary
├── bin/fmt.etag               # Last ETag for this binary
├── registry-index.json        # Cached registry index
├── registry-index.json.etag   # Last ETag for the index
└── installed-manifest.json    # Known-good state
```

On fetch:

```rust
async fn fetch_with_cache(url: &str, cache_path: &Path) -> Result<Vec<u8>> {
    let etag_path = cache_path.with_extension("etag");
    let previous_etag = std::fs::read_to_string(&etag_path).ok();

    let mut request = client.get(url);
    if let Some(etag) = &previous_etag {
        request = request.header("If-None-Match", etag);
    }

    let response = request.send().await?;

    if response.status() == StatusCode::NOT_MODIFIED {
        return Ok(std::fs::read(cache_path)?);
    }

    let bytes = response.bytes().await?;
    let new_etag = response.headers().get("etag").and_then(|v| v.to_str().ok());

    std::fs::write(cache_path, &bytes)?;
    if let Some(etag) = new_etag {
        std::fs::write(&etag_path, etag)?;
    }

    Ok(bytes.to_vec())
}
```

This means:
- `octx update` called twice in a row: first call downloads, second call is a no-op (304)
- `octx update` after a new release: only the changed arms re-download
- Registry index refresh: lightweight, just headers round-trip

### 10.5 Race condition scenarios

| Scenario | How it's handled |
|----------|-----------------|
| Two `octx update` at once | File lock (`fs2`). Second one waits, then reads fresh state. |
| `octx x fmt` triggers JIT install during `octx update` | File lock is per-operation. JIT install acquires its own lock. |
| `octx install fmt` while `octx update` is running | Lock serializes. Update runs first, then install sees new state. |
| Network drops mid-download | Temp file is written, then atomically renamed. Partial download is discarded. |

### 10.6 Atomic install

```rust
fn install_binary(source: &Path, dest: &Path) -> Result<()> {
    let tmp = dest.with_extension("tmp");
    std::fs::rename(source, &tmp)?;
    #[cfg(unix)]
    std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    std::fs::rename(&tmp, dest)?;  // Atomic on same filesystem
    Ok(())
}
```

No selective update for v1. `octx update` updates everything. `ponytail: selective update YAGNI, add when someone has a 200ms update they want to skip.`

---

## 11. `octx x` / `octx exec` — Dispatching

### 11.1 Help interception

`octx x --help` is ambiguous: is it asking for `x`'s help or the arm's help?

Clap handles `trailing_var_arg(true)` for `x` and `exec`. The head intercepts:
- `-h` / `--help` at the `x`/`exec` subcommand level → show arm's `--help` by running `{bin} --help`
- `-V` / `--version` → show arm's `--version` by running `{bin} --version`
- Everything else → pass through to the arm

Actually simpler: `octx x --help` shows octx's help for the `x` subcommand (which is just "usage: octx x <arm> [args]"). To get arm help, user runs `octx x fmt --help` which passes `--help` to `fmt`. This is the natural clap behavior with `trailing_var_arg(true)`.

### 11.2 Exit code propagation

```rust
let status = Command::new(bin_path)
    .args(&arm_args)
    .stdin(std::process::Stdio::inherit())
    .stdout(std::process::Stdio::inherit())
    .stderr(std::process::Stdio::inherit())
    .status()?;
std::process::exit(status.code().unwrap_or(1));
```

### 11.3 JIT install — frictionless auto-install + run

If the arm is not found, `octx x` and `octx exec` install it silently and run it immediately:

```
$ octx x fmt file.rs
octx: installing 'fmt' (latest) from registry...
  ✓ fmt v0.1.0 downloaded and installed
[fmt output]
```

No prompt. No user interruption. The tool behaves as if it was already installed.

**How it works:**

```rust
fn prepare_and_run(name: &str, args: &[String]) -> Result<()> {
    let bin_path = data_dir().join("bin").join(name);

    if !bin_path.exists() {
        // JIT install binary + skill
        eprintln!("octx: installing '{}' (latest) from registry...", name);
        install::from_registry(name)?;  // also calls install_skill internally
        eprintln!("  ✓ {} installed", name);
    }

    // Run
    let status = Command::new(&bin_path)
        .args(args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()?;

    std::process::exit(status.code().unwrap_or(1));
}
```

**Non-interactive (`OCTX_NONINTERACTIVE=1`):** already handled — same behavior, no prompt either way.

**Originally from registry, then from remote on second invocation:**
- First `octx x fmt` → installs from registry, runs
- If `octx install github.com/other/fmt` overwrites it later → `octx x fmt` runs the overwritten binary
- No cache-aside logic needed — just run whatever's in `{data_dir}/octx/bin/<name>`

---

## 12. One-Liner Install

```bash
curl -fsSL https://octx.sh/install.sh | bash
```

The `install.sh` script:
1. Detects OS + arch
2. Downloads `octx-{target}.gz` from GitHub latest release
3. Extracts to `/usr/local/bin/octx` (or `~/.local/bin/octx` if no sudo)
4. Runs `octx init` to add `{data_dir}/octx/bin` to PATH
5. Prompts: "Add GitHub token? (Y/n)" → `octx creds add github`
6. Prints: "Done. Run `octx update` anytime to keep octx and all tools current."

The script is checked into the monorepo at `install.sh` and deployed as a GitHub Pages site at `octx.sh`.

---

## 13. Security Scanning

Free tools in CI:

| Tool | What it catches | Setup |
|------|----------------|-------|
| **GitHub CodeQL** | Code vulnerabilities, injection, XSS | Built into GitHub. Add `.github/workflows/codeql.yml`. |
| **Dependabot** | Outdated deps with known CVEs | Built into GitHub. Enable via repo settings. |
| **`cargo audit`** | Rust dependency vulnerabilities | `cargo install cargo-audit` + CI step. |
| **`cargo deny`** | License compliance, duplicate deps | Optional. `cargo install cargo-deny`. |

`cargo audit` is the most important — catches CVEs in the dependency tree. Add to CI:

```yaml
- name: Audit dependencies
  run: cargo audit
```

---

## 14. Config File

```toml
# {config_dir}/octx/config.toml
registry_url = "https://github.com/AutoMas0n/octx/releases/latest/download/registry-index.json"
noninteractive = false     # auto-yes for JIT installs

[links]
# Agent vendor directories to sync skills to
# "pi" = "/home/user/.pi/agent/skills"
# "claude" = "/home/user/.claude/skills"
```

Light. One file, optional managed by `octx link`. All fields have sensible defaults.

---

## 15. Collision & Update Semantics

| Scenario | Behavior |
|----------|----------|
| `octx install fmt` + `octx install github.com/other/fmt` | Second install overwrites `{data_dir}/octx/bin/fmt`. Manifest tracks last source. |
| `octx update` | Re-downloads the head (if newer), every arm, and syncs skills. |
| `octx uninstall fmt` | Removes binary + skill file + manifest entry. |
| `octx install fmt` (already installed) | Re-downloads latest. Same as `octx update` for that arm. |

---

## 16. `installed-manifest.json` Schema

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

---

## 17. Implementation Phases

| Phase | What | Outcome |
|-------|------|---------|
| **0. Core skeleton** | Workspace, all clap subcommands, error types, dirs | `octx --help` shows all commands |
| **1. Arm discovery** | Find arm in `{data_dir}/octx/bin/`, dispatch | `octx x fmt <file>` works if binary exists |
| **2. JIT install (registry)** | `install::from_registry()`, fetch, checksum, drop | `octx x fmt <file>` prompts + installs + runs |
| **3. Lifecycle** | `octx install`, `uninstall`, `update`, `ls` | Full package management |
| **4. Remote install** | `octx install github.com/user/repo`, predict URL | `go install`-style remote tool delivery |
| **5. Skills + agent links** | `octx link`, `skills.rs`, skill.md in arms | `octx link ~/.pi/agent/skills` registers and syncs |
| **6. Shell init + creds** | `octx init` + `octx creds` + `install.sh` | One-command PATH setup, auth storage |
| **7. Registry** | `registry-index.json`, `octx search` | Remote registry lookup |
| **8. Update (concurrent + skills + self-update)** | `update.rs` with ETag, worker pool, skill sync | `octx update` downloads everything + replaces octx itself + syncs skills in ~1s |
| **9. First arm (dogfood)** | Build `fmt` as a real arm with `skill.md` | End-to-end: install, update, skills synced to agent |
| **10. Pi Zero test** | Cross-compile, deploy, benchmark | Confirmed ≤5MB RSS, ≤100ms cold start |

---

## 18. Questions resolved

| # | Q | A |
|---|----|----|
| 1 | Registry URL | Hardcoded constant, override in `config.toml` |
| 2 | Version syntax | No versions. Always latest. YAGNI. |
| 3 | Non-GitHub remotes | v1 GitHub only. URL parsing is vendor-agnostic. |
| 4 | Overwrite collisions | Last install wins. Manifest tracks source. |
| 5 | Security scanning | CodeQL + Dependabot + `cargo audit` |
| 6 | Config file | `{config_dir}/octx/config.toml`, light |
| 7 | Private repos | `octx creds` stores tokens, used on download |
| 8 | One-liner install | `curl -fsSL https://octx.sh/install.sh \| bash` |
| 9 | `octx x --help` | Passes through to arm. Clap `trailing_var_arg(true)`. |
| 10 | Offline behavior | Cached arms work offline. Only install/update need network. |