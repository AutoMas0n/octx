# Task 13: cli.rs — Command-line interface (all subcommands)

## Contract

Wire all subcommands using clap derive. This is the integration point.

```rust
#[derive(clap::Parser)]
#[command(name = "octx", about = "Octopus CLI — your tooling, one head, many arms")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Run an arm (shorthand). Alias: exec
    #[command(trailing_var_arg(true))]
    X { arm: String, args: Vec<String> },

    /// Run an arm (explicit)
    #[command(trailing_var_arg(true))]
    Exec { arm: String, args: Vec<String> },

    /// Install an arm from the registry or a remote GitHub repo
    Install {
        name: String,
        #[arg(long)]
        bin: Option<String>,
    },

    /// Uninstall an arm
    Uninstall { name: String },

    /// Update all installed arms, sync skills, and update octx itself
    Update,

    /// List installed arms
    #[command(aliases = &["list", "l"])]
    Ls,

    /// Search the registry for arms
    Search { query: String },

    /// Shell integration (adds bin dir to PATH)
    Init,

    /// Manage credentials
    #[command(subcommand)]
    Creds(CredsCommand),

    /// Manage agent skill directory links
    #[command(subcommand)]
    Link(LinkCommand),
}

#[derive(clap::Subcommand)]
pub enum CredsCommand {
    /// Add a credential token for a host
    Add { host: String, #[arg(long)] token: Option<String> },
    /// Get a credential token for a host
    Get { host: String, #[arg(long)] raw: bool },
    /// Remove a credential token
    Remove { host: String },
    /// List all stored credential hosts
    List,
}

#[derive(clap::Subcommand)]
pub enum LinkCommand {
    /// Register an agent skills directory
    Add { path: String, #[arg(long)] unlink: bool },
    /// List all registered links
    List,
}

/// Parse CLI args and dispatch to the appropriate handler.
pub async fn run() -> Result<(), OctxError>
```

## Dependencies

- All prior modules: dispatch, install, registry, creds, skills, init, update, config, error, paths

## Tests (RED)

```
- test_cli_x_subcommand_parses_correctly  (structopt parsing test)
- test_cli_install_subcommand_parses_name
- test_cli_creds_add_parses_host_and_token
- test_cli_link_add_parses_path
- test_empty_args_prints_help
```

## Implementation notes

- `run()` is the main dispatcher:
  - `Command::X { arm, args }` → `dispatch::run_arm(&arm, &args).await`
  - `Command::Exec { arm, args }` → same as X
  - `Command::Install { name, bin }` → if name contains `/`, `install::from_remote(&name, bin.as_deref()).await`, else `install::from_registry(&name).await`
  - `Command::Uninstall { name }` → remove binary from bin_dir, remove skill from skills_dir, remove from manifest
  - `Command::Update` → `update::run().await`
  - `Command::Ls` → load manifest, print each arm with version + source
  - `Command::Search { query }` → fetch registry index, `index.search(&query)`, print results
  - `Command::Init` → `init::install_path_hook()`
  - `Command::Creds(sub)` → dispatch to creds module functions
  - `Command::Link(sub)` → dispatch to skills module functions
- `trailing_var_arg(true)` on X and Exec — means `octx x fmt --flag value` passes `["--flag", "value"]` to `args`
- `--help` on X/Exec shows octx's subcommand help, not the arm's help. Arm help is `octx x fmt --help`
- X and Exec are identical in behavior — keep both for user preference
- Add as aliases in clap: `Ls` has `[command(aliases = &["list", "l"])]`
- Error handling: wrap each handler with `if let Err(e) = handler { eprintln!("error: {e}"); std::process::exit(e.exit_code()); }`

## Output

- Creates: `src/cli.rs`
- Modifies: `src/lib.rs` (add `pub mod cli;`, update `pub async fn run()` to call `cli::run().await`)
## Definition of Done
- [ ] `cargo check` compiles without errors
- [ ] `cargo test` — all CLI tests pass
- [ ] `cargo clippy -- -D warnings` — no warnings
- [ ] `cargo fmt --check` — code is formatted
- [ ] octx --help shows all subcommands
- [ ] octx x --help shows x subcommand help
- [ ] octx creds add/remove/list/get are subcommands
- [ ] octx link add/list parse correctly
- [ ] octx ls, octx search, octx init are top-level commands
- [ ] Each command dispatches to the correct module handler
- [ ] run() is async and returns Result<(), OctxError>
