/// Integration tests for the octx CLI.
///
/// These tests verify that CLI argument parsing works correctly
/// by exercising the Cli parser through the public `octx::cli::Cli` API.
use clap::Parser;
use octx::cli::Cli;
use octx::cli::Command;
use std::path::{Path, PathBuf};
use std::process::Command as ProcCommand;

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

/// A per-test scratch config directory so the real user config is untouched.
fn scratch_config_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("octx-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run the real `octx` binary with a scratch `XDG_CONFIG_HOME`.
fn octx(args: &[&str], config_home: &Path) -> std::process::Output {
    ProcCommand::new(env!("CARGO_BIN_EXE_octx"))
        .args(args)
        .env("XDG_CONFIG_HOME", config_home)
        .output()
        .expect("octx should run")
}

#[test]
fn creds_cli_roundtrip_with_redacted_listing() {
    let dir = scratch_config_dir("creds");
    let secret = "ghp_cli_roundtrip_secret";

    let out = octx(&["creds", "add", "github", "--token", secret], &dir);
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // list shows the host but never the value
    let out = octx(&["creds", "list"], &dir);
    let list = String::from_utf8_lossy(&out.stdout);
    assert!(list.contains("github: ***"), "list: {list}");
    assert!(
        !list.contains(secret),
        "list must not reveal the token: {list}"
    );

    // get --raw returns the value for scripts
    let out = octx(&["creds", "get", "github", "--raw"], &dir);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), secret);

    // the stored file is encrypted at rest
    let stored = std::fs::read(dir.join("octx/creds.enc")).expect("creds.enc should exist");
    assert!(
        !stored.windows(secret.len()).any(|w| w == secret.as_bytes()),
        "creds.enc must not contain the plaintext token"
    );

    // remove empties the store
    let out = octx(&["creds", "remove", "github"], &dir);
    assert!(out.status.success());
    let out = octx(&["creds", "list"], &dir);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("No credentials stored."),
        "store should be empty after remove"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn creds_reset_recovers_from_an_unreadable_store() {
    let dir = scratch_config_dir("creds-reset");
    let store_dir = dir.join("octx");
    std::fs::create_dir_all(&store_dir).expect("create store dir");
    // Bytes long enough to hold salt+nonce but not encrypted for this machine.
    let foreign = vec![7u8; 60];
    std::fs::write(store_dir.join("creds.enc"), &foreign).expect("write foreign store");

    // The failure is actionable: it names the file, the remedy, and that
    // nothing was changed.
    let out = octx(&["creds", "list"], &dir);
    assert!(!out.status.success(), "an unreadable store must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("creds.enc"), "stderr: {stderr}");
    assert!(stderr.contains("octx creds reset"), "stderr: {stderr}");
    assert!(stderr.contains("nothing was changed"), "stderr: {stderr}");
    assert!(
        store_dir.join("creds.enc").exists(),
        "reporting must not discard the store"
    );

    // ... and `reset` moves it aside rather than deleting it.
    let out = octx(&["creds", "reset"], &dir);
    assert!(
        out.status.success(),
        "reset failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!store_dir.join("creds.enc").exists());
    let backup = std::fs::read_dir(&store_dir)
        .expect("read store dir")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| name.starts_with("creds.enc.bak"))
        .expect("reset must leave a backup");
    assert_eq!(
        std::fs::read(store_dir.join(&backup)).expect("read backup"),
        foreign,
        "the backup must keep the original bytes"
    );

    // ... after which credentials work again.
    let out = octx(&["creds", "add", "qobuz", "--token", "tok"], &dir);
    assert!(
        out.status.success(),
        "add after reset failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = octx(&["creds", "list"], &dir);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("qobuz: ***"),
        "list: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn creds_reset_without_a_store_reports_the_path() {
    let dir = scratch_config_dir("creds-reset-none");
    let out = octx(&["creds", "reset"], &dir);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("No credential store to reset at"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("creds.enc"), "stdout: {stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}
