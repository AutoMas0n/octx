/// Integration tests for the octx CLI.
///
/// These tests verify that CLI argument parsing works correctly
/// by exercising the Cli parser through the public `octx::cli::Cli` API.
use clap::Parser;
use octx::cli::Cli;
use octx::cli::Command;

#[test]
fn test_cli_x_parsing() {
    let args: Vec<&str> = vec!["octx", "x", "fmt", "--my-flag", "value"];
    let cli = Cli::try_parse_from(args).expect("should parse x fmt with args");
    match cli.command {
        Command::X { arm, args } => {
            assert_eq!(arm, "fmt");
            assert_eq!(args, vec!["--my-flag", "value"]);
        }
        other => panic!("expected Command::X, got {other:?}"),
    }
}

#[test]
fn test_cli_ls_parsing() {
    let args = vec!["octx", "ls"];
    let cli = Cli::try_parse_from(args).expect("should parse ls");
    assert!(matches!(cli.command, Command::Ls));
}

#[test]
fn test_cli_update_parsing() {
    let args = vec!["octx", "update"];
    let cli = Cli::try_parse_from(args).expect("should parse update");
    assert!(matches!(cli.command, Command::Update));
}

#[test]
fn test_cli_init_parsing() {
    let args = vec!["octx", "init"];
    let cli = Cli::try_parse_from(args).expect("should parse init");
    assert!(matches!(cli.command, Command::Init));
}
