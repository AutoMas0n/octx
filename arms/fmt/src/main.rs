use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "fmt", version, about = "Opinionated code formatter")]
struct Args {
    /// Files to check
    files: Vec<String>,
    /// Check mode: report violations without modifying files
    #[arg(long)]
    check: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.files.is_empty() {
        anyhow::bail!("no files specified");
    }
    let mut has_violations = false;
    for path in &args.files {
        if check_line_lengths(path, 100)? {
            has_violations = true;
        }
    }
    if has_violations {
        std::process::exit(1);
    }
    Ok(())
}

/// Check line lengths in a file. Returns true if any line exceeds max_len.
/// Prints violating lines to stdout.
fn check_line_lengths(path: impl AsRef<Path>, max_len: usize) -> Result<bool> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut has_violations = false;
    for (i, line) in content.lines().enumerate() {
        if line.len() > max_len {
            println!(
                "{}:{}:{}: line too long ({} chars, max {})",
                path.display(),
                i + 1,
                max_len + 1,
                line.len(),
                max_len
            );
            has_violations = true;
        }
    }
    Ok(has_violations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_check_line_lengths_clean() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "short line").unwrap();
        let path = f.path().to_path_buf();
        assert!(!check_line_lengths(&path, 100).unwrap());
    }

    #[test]
    fn test_check_line_lengths_violation() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{}", "x".repeat(101)).unwrap();
        let path = f.path().to_path_buf();
        assert!(check_line_lengths(&path, 100).unwrap());
    }

    #[test]
    fn test_check_line_lengths_multiple_violations() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{}", "x".repeat(101)).unwrap();
        writeln!(f, "ok").unwrap();
        writeln!(f, "{}", "y".repeat(101)).unwrap();
        let path = f.path().to_path_buf();
        assert!(check_line_lengths(&path, 100).unwrap());
    }

    #[test]
    fn test_check_line_lengths_empty_file() {
        let f = tempfile::NamedTempFile::new().unwrap();
        let path = f.path().to_path_buf();
        assert!(!check_line_lengths(&path, 100).unwrap());
    }

    #[test]
    fn test_check_line_lengths_nonexistent() {
        let result = check_line_lengths("/nonexistent/file.txt", 100);
        assert!(result.is_err());
    }
}
