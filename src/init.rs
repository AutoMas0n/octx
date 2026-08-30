use crate::error::OctxError;
use crate::paths;
use std::fs;
use std::path::{Path, PathBuf};

/// Detected shell type for PATH integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Unknown(String),
}

/// Detect the user's shell from $SHELL env var and return the type.
pub fn detect_shell() -> ShellType {
    detect_shell_impl(std::env::var("SHELL").ok().as_deref())
}

fn detect_shell_impl(shell_var: Option<&str>) -> ShellType {
    let path = match shell_var {
        Some(p) => p,
        None => return ShellType::Unknown(String::new()),
    };

    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(path);

    match filename {
        "bash" => ShellType::Bash,
        "zsh" => ShellType::Zsh,
        "fish" => ShellType::Fish,
        _ if filename.to_lowercase().contains("pwsh")
            || filename.to_lowercase().contains("powershell") =>
        {
            ShellType::PowerShell
        }
        _ => ShellType::Unknown(path.to_string()),
    }
}

/// Get the path to the user's shell rc file.
pub fn rc_file_path(shell: &ShellType) -> PathBuf {
    let home = dirs::home_dir()
        .expect("octx rc_file_path: home directory not found (are you in a sandbox?)");

    match shell {
        ShellType::Bash => home.join(".bashrc"),
        ShellType::Zsh => home.join(".zshrc"),
        ShellType::Fish => home.join(".config").join("fish").join("config.fish"),
        ShellType::PowerShell => home
            .join("Documents")
            .join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
        ShellType::Unknown(_) => home.join(".bashrc"),
    }
}

fn install_path_line(shell: &ShellType, bin_dir: &Path) -> String {
    let bin_dir_str = bin_dir.to_string_lossy();
    match shell {
        ShellType::Bash | ShellType::Zsh | ShellType::Unknown(_) => {
            format!("export PATH=\"$PATH:{bin_dir_str}\"")
        }
        ShellType::Fish => {
            format!("set -gx PATH $PATH {bin_dir_str}")
        }
        ShellType::PowerShell => {
            format!("$env:PATH = \"$env:PATH:{bin_dir_str}\"")
        }
    }
}

/// Append the bin directory to the shell rc file (idempotent).
pub fn install_path_hook() -> Result<(), OctxError> {
    let shell = detect_shell();
    let rc_path = rc_file_path(&shell);
    install_path_hook_impl(&shell, &rc_path)
}

fn install_path_hook_impl(shell: &ShellType, rc_path: &Path) -> Result<(), OctxError> {
    let bin_dir = paths::bin_dir();
    let line = install_path_line(shell, &bin_dir);

    // Read existing content (or empty if file doesn't exist)
    let content = if rc_path.exists() {
        fs::read_to_string(rc_path)?
    } else {
        String::new()
    };

    // Check if the bin_dir path already appears in any line (idempotency)
    let bin_dir_str = bin_dir.to_string_lossy();
    if content.lines().any(|l| l.contains(bin_dir_str.as_ref())) {
        eprintln!("octx: PATH hook already present in {}", rc_path.display());
        return Ok(());
    }

    // Append the PATH line
    let mut new_content = content;
    if !new_content.is_empty() && !new_content.ends_with('\n') {
        new_content.push('\n');
    }
    new_content.push_str(&line);
    new_content.push('\n');

    // Ensure parent directory exists
    if let Some(parent) = rc_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(rc_path, new_content)?;

    eprintln!("octx: Added PATH hook to {}", rc_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_detect_shell_defaults_to_unknown_when_no_env() {
        let shell = detect_shell_impl(None);
        assert!(matches!(shell, ShellType::Unknown(_)));
    }

    #[test]
    fn test_rc_file_path_bash_returns_dot_bashrc() {
        let path = rc_file_path(&ShellType::Bash);
        assert!(path.ends_with(".bashrc"));
    }

    #[test]
    fn test_rc_file_path_zsh_returns_dot_zshrc() {
        let path = rc_file_path(&ShellType::Zsh);
        assert!(path.ends_with(".zshrc"));
    }

    #[test]
    fn test_rc_file_path_fish_returns_config_fish() {
        let path = rc_file_path(&ShellType::Fish);
        assert!(path.ends_with("config.fish"));
    }

    #[test]
    fn test_install_path_hook_adds_bin_dir_to_rc() {
        let dir = std::env::temp_dir().join("octx-init-test-add");
        let rc_path = dir.join(".bashrc");
        fs::create_dir_all(&dir).unwrap();

        install_path_hook_impl(&ShellType::Bash, &rc_path).unwrap();
        let content = fs::read_to_string(&rc_path).unwrap();
        assert!(
            content.contains("octx/bin"),
            "content should contain octx/bin: {content}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_path_hook_is_idempotent() {
        let dir = std::env::temp_dir().join("octx-init-test-idempotent");
        let rc_path = dir.join(".zshrc");
        fs::create_dir_all(&dir).unwrap();

        // First call
        install_path_hook_impl(&ShellType::Zsh, &rc_path).unwrap();
        let content_first = fs::read_to_string(&rc_path).unwrap();

        // Second call — should not duplicate
        install_path_hook_impl(&ShellType::Zsh, &rc_path).unwrap();
        let content_second = fs::read_to_string(&rc_path).unwrap();

        assert_eq!(content_first, content_second);

        let _ = fs::remove_dir_all(&dir);
    }
}
