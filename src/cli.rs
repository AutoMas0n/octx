use clap::Parser;

use crate::OctxError;

/// Octopus CLI — your tooling, one head, many arms.
#[derive(Parser, Debug)]
#[command(
    name = "octx",
    about = "Octopus CLI — your tooling, one head, many arms",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Run an arm (shorthand).
    #[command(trailing_var_arg(true))]
    X {
        /// Name of the arm to run
        arm: String,
        /// Arguments to pass to the arm
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Run an arm (explicit).
    #[command(trailing_var_arg(true))]
    Exec {
        /// Name of the arm to run
        arm: String,
        /// Arguments to pass to the arm
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Install an arm from the registry or a remote GitHub repo.
    Install {
        /// Arm name (registry) or "github.com/owner/repo" (remote)
        name: String,
        /// Override binary name (for remote installs)
        #[arg(long)]
        bin: Option<String>,
    },

    /// Uninstall an arm.
    Uninstall {
        /// Name of the arm to uninstall
        name: String,
    },

    /// Update all installed arms, sync skills, and update octx itself.
    Update,

    /// List installed arms.
    #[command(aliases = &["list", "l"])]
    Ls,

    /// Search the registry for arms.
    Search {
        /// Search query (matches name and description)
        query: String,
    },

    /// Shell integration — adds bin dir to PATH.
    Init,

    /// Manage credentials.
    #[command(subcommand)]
    Creds(CredsCommand),

    /// Manage agent skill directory links.
    #[command(subcommand)]
    Link(LinkCommand),
}

#[derive(clap::Subcommand, Debug)]
pub enum CredsCommand {
    /// Add a credential token for a host.
    Add {
        /// Host name (e.g. "github.com")
        host: String,
        /// Token value. If omitted, prompts interactively.
        #[arg(long)]
        token: Option<String>,
    },
    /// Get a credential token for a host.
    Get {
        /// Host name
        host: String,
        /// Print raw token without label
        #[arg(long)]
        raw: bool,
    },
    /// Remove a credential token.
    Remove {
        /// Host name
        host: String,
    },
    /// List all stored credential hosts.
    List,
}

#[derive(clap::Subcommand, Debug)]
pub enum LinkCommand {
    /// Register an agent skills directory, or unregister with --unlink.
    Add {
        /// Path to the agent skills directory
        path: String,
        /// Remove the link instead of adding it
        #[arg(long)]
        unlink: bool,
    },
    /// List all registered links.
    List,
}

/// Parse CLI args and dispatch to the appropriate handler.
pub async fn run() -> Result<(), OctxError> {
    let cli = Cli::parse();

    match cli.command {
        Command::X { arm, args } | Command::Exec { arm, args } => {
            crate::dispatch::run_arm(&arm, &args).await
        }
        Command::Install { name, bin } => {
            if name.contains('/') {
                crate::install::from_remote(&name, bin.as_deref()).await
            } else {
                crate::install::from_registry(&name).await
            }
        }
        Command::Uninstall { name } => uninstall_arm(&name),
        Command::Update => crate::update::run().await,
        Command::Ls => list_arms(),
        Command::Search { query } => search_registry(&query).await,
        Command::Init => crate::init::install_path_hook(),
        Command::Creds(sub) => handle_creds(sub),
        Command::Link(sub) => handle_link(sub),
    }
}

/// Uninstall an arm: remove binary, skill file, and manifest entry.
fn uninstall_arm(name: &str) -> Result<(), OctxError> {
    let bin_path = crate::paths::bin_dir().join(name);
    if bin_path.exists() {
        std::fs::remove_file(&bin_path)?;
    }

    let skill_path = crate::paths::skills_dir().join(format!("{name}.md"));
    if skill_path.exists() {
        std::fs::remove_file(&skill_path)?;
    }

    let mut manifest = crate::manifest::Manifest::load()?;
    if manifest.remove_arm(name) {
        manifest.save()?;
        eprintln!("octx: uninstalled '{name}'");
        Ok(())
    } else {
        Err(OctxError::NotFound(format!(
            "arm '{name}' is not installed"
        )))
    }
}

/// List installed arms from the manifest.
fn list_arms() -> Result<(), OctxError> {
    let manifest = crate::manifest::Manifest::load()?;
    if manifest.arms.is_empty() {
        println!("No arms installed.");
    } else {
        // Sort by name for consistent output
        let mut names: Vec<&String> = manifest.arms.keys().collect();
        names.sort();
        for name in names {
            if let Some(entry) = manifest.arms.get(name) {
                println!("{name} (v{}, {})", entry.version, entry.source);
            }
        }
    }
    Ok(())
}

/// Search the registry for arms matching a query.
async fn search_registry(query: &str) -> Result<(), OctxError> {
    let (index, _cached) = crate::registry::RegistryIndex::fetch().await?;
    let results = index.search(query);
    if results.is_empty() {
        println!("No arms found matching '{query}'");
    } else {
        for (name, desc) in &results {
            println!("{name}: {desc}");
        }
    }
    Ok(())
}

/// Handle credential subcommands.
fn handle_creds(cmd: CredsCommand) -> Result<(), OctxError> {
    match cmd {
        CredsCommand::Add { host, token } => match token {
            Some(tok) => crate::creds::add(&host, &tok),
            None => {
                let cfg = crate::config::Config::load()?;
                if cfg.is_noninteractive() {
                    return Err(OctxError::Creds(
                        "token required in non-interactive mode. Use --token <TOKEN>".into(),
                    ));
                }
                let token = rpassword::prompt_password(format!("Token for {host}: "))
                    .map_err(|e| OctxError::Creds(format!("failed to read token: {e}")))?;
                crate::creds::add(&host, &token)
            }
        },
        CredsCommand::Get { host, raw } => crate::creds::get(&host, raw),
        CredsCommand::Remove { host } => crate::creds::remove(&host),
        CredsCommand::List => crate::creds::list(),
    }
}

/// Handle link subcommands.
fn handle_link(cmd: LinkCommand) -> Result<(), OctxError> {
    match cmd {
        LinkCommand::Add { path, unlink } => {
            let agent_name = derive_agent_name(&path);
            if unlink {
                crate::skills::link_remove(&agent_name)
            } else {
                crate::skills::link_add(&agent_name, &path)
            }
        }
        LinkCommand::List => {
            let links = crate::skills::link_list();
            if links.is_empty() {
                println!("No agent links registered.");
            } else {
                let mut sorted = links;
                sorted.sort_by(|(a, _), (b, _)| a.cmp(b));
                for (name, path) in &sorted {
                    println!("{name}: {path}");
                }
            }
            Ok(())
        }
    }
}

/// Derive an agent name from a skills directory path.
///
/// Scans the path for a hidden directory (starts with `.`) and uses its name
/// without the leading dot. Falls back to the last path component.
///
/// Examples:
/// - `/home/user/.pi/agent/skills` → `pi`
/// - `/home/user/.claude/skills` → `claude`
/// - `/some/dir` → `dir`
fn derive_agent_name(path: &str) -> String {
    let p = std::path::Path::new(path);
    for component in p.components().rev() {
        let raw = component.as_os_str().to_string_lossy();
        if raw.starts_with('.') && raw.len() > 1 {
            return raw.trim_start_matches('.').to_string();
        }
    }
    // Fallback: last component
    p.components()
        .next_back()
        .map(|c| {
            c.as_os_str()
                .to_string_lossy()
                .trim_start_matches('.')
                .to_string()
        })
        .unwrap_or_else(|| "agent".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_cli_x_subcommand_parses_correctly() {
        let args: Vec<&str> = vec!["octx", "x", "fmt", "--flag", "value", "arg"];
        let cli = Cli::try_parse_from(args).expect("should parse x subcommand");
        match cli.command {
            Command::X { arm, args } => {
                assert_eq!(arm, "fmt");
                assert_eq!(args, vec!["--flag", "value", "arg"]);
            }
            other => panic!("expected Command::X, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_install_subcommand_parses_name() {
        let args: Vec<&str> = vec!["octx", "install", "fmt"];
        let cli = Cli::try_parse_from(args).expect("should parse install subcommand");
        match cli.command {
            Command::Install { name, bin } => {
                assert_eq!(name, "fmt");
                assert!(bin.is_none());
            }
            other => panic!("expected Command::Install, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_install_with_bin_flag() {
        let args: Vec<&str> = vec![
            "octx",
            "install",
            "github.com/owner/repo",
            "--bin",
            "my-tool",
        ];
        let cli = Cli::try_parse_from(args).expect("should parse install with --bin");
        match cli.command {
            Command::Install { name, bin } => {
                assert_eq!(name, "github.com/owner/repo");
                assert_eq!(bin.as_deref(), Some("my-tool"));
            }
            other => panic!("expected Command::Install, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_creds_add_parses_host_and_token() {
        let args: Vec<&str> = vec!["octx", "creds", "add", "github.com", "--token", "ghp_abc"];
        let cli = Cli::try_parse_from(args).expect("should parse creds add");
        match cli.command {
            Command::Creds(CredsCommand::Add { host, token }) => {
                assert_eq!(host, "github.com");
                assert_eq!(token.as_deref(), Some("ghp_abc"));
            }
            other => panic!("expected CredsCommand::Add, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_creds_add_without_token() {
        let args: Vec<&str> = vec!["octx", "creds", "add", "github.com"];
        let cli = Cli::try_parse_from(args).expect("should parse creds add without token");
        match cli.command {
            Command::Creds(CredsCommand::Add { host, token }) => {
                assert_eq!(host, "github.com");
                assert!(token.is_none());
            }
            other => panic!("expected CredsCommand::Add, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_link_add_parses_path() {
        let args: Vec<&str> = vec!["octx", "link", "add", "/home/user/.pi/agent/skills"];
        let cli = Cli::try_parse_from(args).expect("should parse link add");
        match cli.command {
            Command::Link(LinkCommand::Add { path, unlink }) => {
                assert_eq!(path, "/home/user/.pi/agent/skills");
                assert!(!unlink);
            }
            other => panic!("expected LinkCommand::Add, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_link_add_with_unlink() {
        let args: Vec<&str> = vec![
            "octx",
            "link",
            "add",
            "/home/user/.pi/agent/skills",
            "--unlink",
        ];
        let cli = Cli::try_parse_from(args).expect("should parse link add --unlink");
        match cli.command {
            Command::Link(LinkCommand::Add { path, unlink }) => {
                assert_eq!(path, "/home/user/.pi/agent/skills");
                assert!(unlink);
            }
            other => panic!("expected LinkCommand::Add with --unlink, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_empty_args_prints_help() {
        // Cli::try_parse_from with no args should error (needs at least subcommand)
        let result = Cli::try_parse_from(vec!["octx"]);
        assert!(result.is_err(), "expected error for no subcommand");
    }

    #[test]
    fn test_cli_exec_subcommand_parses() {
        let args: Vec<&str> = vec!["octx", "exec", "deploy", "--env", "prod"];
        let cli = Cli::try_parse_from(args).expect("should parse exec subcommand");
        match cli.command {
            Command::Exec { arm, args } => {
                assert_eq!(arm, "deploy");
                assert_eq!(args, vec!["--env", "prod"]);
            }
            other => panic!("expected Command::Exec, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_update_parses() {
        let args: Vec<&str> = vec!["octx", "update"];
        let cli = Cli::try_parse_from(args).expect("should parse update");
        assert!(matches!(cli.command, Command::Update));
    }

    #[test]
    fn test_cli_ls_parses() {
        let args: Vec<&str> = vec!["octx", "ls"];
        let cli = Cli::try_parse_from(args).expect("should parse ls");
        assert!(matches!(cli.command, Command::Ls));
    }

    #[test]
    fn test_cli_search_parses() {
        let args: Vec<&str> = vec!["octx", "search", "formatter"];
        let cli = Cli::try_parse_from(args).expect("should parse search");
        match cli.command {
            Command::Search { query } => assert_eq!(query, "formatter"),
            other => panic!("expected Command::Search, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_init_parses() {
        let args: Vec<&str> = vec!["octx", "init"];
        let cli = Cli::try_parse_from(args).expect("should parse init");
        assert!(matches!(cli.command, Command::Init));
    }

    #[test]
    fn test_cli_creds_get_raw_parses() {
        let args: Vec<&str> = vec!["octx", "creds", "get", "github.com", "--raw"];
        let cli = Cli::try_parse_from(args).expect("should parse creds get --raw");
        match cli.command {
            Command::Creds(CredsCommand::Get { host, raw }) => {
                assert_eq!(host, "github.com");
                assert!(raw);
            }
            other => panic!("expected CredsCommand::Get, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_creds_list_parses() {
        let args: Vec<&str> = vec!["octx", "creds", "list"];
        let cli = Cli::try_parse_from(args).expect("should parse creds list");
        assert!(matches!(cli.command, Command::Creds(CredsCommand::List)));
    }

    #[test]
    fn test_cli_uninstall_parses() {
        let args: Vec<&str> = vec!["octx", "uninstall", "fmt"];
        let cli = Cli::try_parse_from(args).expect("should parse uninstall");
        match cli.command {
            Command::Uninstall { name } => assert_eq!(name, "fmt"),
            other => panic!("expected Command::Uninstall, got {other:?}"),
        }
    }

    #[test]
    fn test_cli_link_list_parses() {
        let args: Vec<&str> = vec!["octx", "link", "list"];
        let cli = Cli::try_parse_from(args).expect("should parse link list");
        assert!(matches!(cli.command, Command::Link(LinkCommand::List)));
    }

    #[test]
    fn test_derive_agent_name_from_pi_path() {
        let name = derive_agent_name("/home/user/.pi/agent/skills");
        assert_eq!(name, "pi");
    }

    #[test]
    fn test_derive_agent_name_from_claude_path() {
        let name = derive_agent_name("/home/user/.claude/skills");
        assert_eq!(name, "claude");
    }

    #[test]
    fn test_derive_agent_name_from_generic_path() {
        let name = derive_agent_name("/some/dir");
        assert_eq!(name, "dir");
    }

    #[test]
    fn test_list_arms_empty() {
        let result = list_arms();
        assert!(result.is_ok());
    }

    #[test]
    fn test_uninstall_nonexistent_returns_not_found() {
        let result = uninstall_arm("__octx_test_nonexistent__");
        assert!(result.is_err());
        match result {
            Err(OctxError::NotFound(msg)) => assert!(msg.contains("__octx_test_nonexistent__")),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }
}
