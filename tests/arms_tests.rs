/// Integration tests for the arms workspace members.
///
/// These tests verify that each arm binary compiles and responds
/// correctly to --help, --version, and the check mode.
use std::process::Command;
use std::sync::Once;

/// Build the fmt release binary once.
static BUILD_FMT: Once = Once::new();

fn ensure_fmt_built() {
    BUILD_FMT.call_once(|| {
        let output = Command::new("cargo")
            .args(["build", "-p", "octx-fmt", "--release"])
            .output()
            .expect("failed to run cargo build -p octx-fmt");
        assert!(
            output.status.success(),
            "octx-fmt build failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
}

/// Build path for a workspace member binary under target/release/
fn arm_binary(name: &str) -> String {
    let target = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| format!("{}/target", env!("CARGO_MANIFEST_DIR")));
    format!("{}/release/{name}", target)
}

#[test]
fn test_octx_fmt_compiles() {
    ensure_fmt_built();
}

#[test]
fn test_fmt_help() {
    ensure_fmt_built();
    let output = Command::new(arm_binary("fmt"))
        .arg("--help")
        .output()
        .expect("failed to run fmt --help");
    assert!(output.status.success(), "fmt --help exited non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage"), "help text missing Usage");
    assert!(stdout.contains("check"), "help text missing --check");
}

#[test]
fn test_fmt_version() {
    ensure_fmt_built();
    let output = Command::new(arm_binary("fmt"))
        .arg("--version")
        .output()
        .expect("failed to run fmt --version");
    assert!(output.status.success(), "fmt --version exited non-zero");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"), "version string missing 0.1.0");
}

#[test]
fn test_fmt_check_clean_file() {
    ensure_fmt_built();
    let tmp = std::env::temp_dir().join("octx_test_clean.txt");
    std::fs::write(&tmp, "short line\n").unwrap();

    let output = Command::new(arm_binary("fmt"))
        .args(["--check", &tmp.to_string_lossy()])
        .output()
        .expect("failed to run fmt --check on clean file");
    assert!(
        output.status.success(),
        "fmt --check should exit 0 on clean file"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "should print nothing on clean file");

    std::fs::remove_file(&tmp).ok();
}

#[test]
fn test_fmt_check_violation_file() {
    ensure_fmt_built();
    let tmp = std::env::temp_dir().join("octx_test_long.txt");
    // Write a line > 100 chars
    let long_line = "x".repeat(101);
    std::fs::write(&tmp, &long_line).unwrap();

    let output = Command::new(arm_binary("fmt"))
        .args(["--check", &tmp.to_string_lossy()])
        .output()
        .expect("failed to run fmt --check on violating file");
    assert!(
        !output.status.success(),
        "fmt --check should exit 1 on violations"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.is_empty(), "should print violation info");

    std::fs::remove_file(&tmp).ok();
}

#[test]
fn test_fmt_no_files_error() {
    ensure_fmt_built();
    let output = Command::new(arm_binary("fmt"))
        .output()
        .expect("failed to run fmt with no arguments");
    assert!(
        !output.status.success(),
        "fmt should exit non-zero with no files"
    );
}
