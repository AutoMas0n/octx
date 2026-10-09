//! Pi-specific customizations, isolated from the generic ACP flow.
//!
//! Skills are exposed through an isolated `PI_CODING_AGENT_DIR` rather than
//! mutating the user's global pi configuration.

use std::path::{Path, PathBuf};

/// A temporary pi config directory that is removed on drop.
pub struct PiSetup {
    dir: PathBuf,
}

impl PiSetup {
    /// The `PI_CODING_AGENT_DIR` value to export to the agent subprocess.
    #[must_use]
    pub fn agent_dir(&self) -> String {
        self.dir.display().to_string()
    }
}

impl Drop for PiSetup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Built-in agent registry: agent id -> command and arguments.
#[must_use]
pub fn registry_command(agent: &str) -> Option<(&'static str, Vec<&'static str>)> {
    match agent {
        "pi" => Some(("npx", vec!["-y", "pi-acp"])),
        "claude" => Some(("npx", vec!["-y", "@zed-industries/claude-code-acp"])),
        "codex" => Some(("npx", vec!["-y", "@zed-industries/codex-acp"])),
        "gemini" => Some((
            "npx",
            vec!["-y", "@google/gemini-cli", "--experimental-acp"],
        )),
        _ => None,
    }
}

/// Candidate directories a named skill may live in.
fn skill_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".pi/agent/skills"));
    }
    if let Some(config) = dirs_config() {
        dirs.push(config.join("octx/skills"));
    }
    dirs
}

fn dirs_config() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("XDG_CONFIG_HOME")
        && !explicit.is_empty()
    {
        return Some(PathBuf::from(explicit));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

fn resolve_skill(name: &str) -> Option<PathBuf> {
    let direct = Path::new(name);
    if direct.exists() {
        return Some(direct.to_path_buf());
    }
    skill_search_dirs()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.exists())
}

/// Build an isolated pi config directory containing symlinks to `skills`.
///
/// The user's pi config files (`auth.json`, `provider-keys.json`,
/// `models-store.json`, `settings.json`, ...) are symlinked in too, because pi
/// resolves credentials relative to `PI_CODING_AGENT_DIR` — without them an
/// authenticated pi would look logged out. Symlinks keep the user's global
/// config untouched.
///
/// Skill names that cannot be resolved are warned about and skipped, never
/// fatal. An empty skill list still produces an isolated directory.
pub fn setup_skills(skills: &[String]) -> Result<PiSetup, String> {
    let dir = std::env::temp_dir().join(format!("octx-pi-{}", std::process::id()));
    let skills_dir = dir.join("skills");
    std::fs::create_dir_all(&skills_dir)
        .map_err(|e| format!("failed to create `{}`: {e}", skills_dir.display()))?;

    link_user_config(&dir);

    for name in skills {
        let Some(target) = resolve_skill(name) else {
            eprintln!("warning: skill `{name}` not found; skipping");
            continue;
        };
        let file_name = target.file_name().map_or_else(
            || std::ffi::OsString::from(name),
            std::ffi::OsStr::to_os_string,
        );
        let link = skills_dir.join(&file_name);
        if let Err(e) = make_symlink(&target, &link) {
            eprintln!("warning: could not link skill `{name}`: {e}");
        }
    }

    Ok(PiSetup { dir })
}

/// Symlink the user's pi config files into the isolated directory.
fn link_user_config(isolated: &Path) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let user_dir = PathBuf::from(home).join(".pi/agent");
    let Ok(entries) = std::fs::read_dir(&user_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }
        let link = isolated.join(entry.file_name());
        if link.exists() {
            continue;
        }
        if let Err(e) = make_symlink(&entry.path(), &link) {
            eprintln!(
                "warning: could not link pi config `{}`: {e}",
                entry.file_name().to_string_lossy()
            );
        }
    }
}

fn make_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(not(unix))]
    {
        // Fall back to copying where symlinks are unavailable.
        if target.is_dir() {
            std::fs::create_dir_all(link)?;
            for entry in std::fs::read_dir(target)? {
                let entry = entry?;
                std::fs::copy(entry.path(), link.join(entry.file_name()))?;
            }
            Ok(())
        } else {
            std::fs::copy(target, link).map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_maps_pi_to_pi_acp() {
        let (program, args) = registry_command("pi").expect("pi is registered");
        assert_eq!(program, "npx");
        assert_eq!(args, vec!["-y", "pi-acp"]);
        assert!(registry_command("nonexistent").is_none());
    }

    #[test]
    fn setup_skills_links_resolved_and_skips_missing() {
        let temp = tempfile::tempdir().unwrap();
        let skill = temp.path().join("myskill");
        std::fs::create_dir_all(&skill).unwrap();

        let setup = setup_skills(&[skill.display().to_string(), "not-a-real-skill".to_string()])
            .expect("setup succeeds");
        let link = Path::new(&setup.agent_dir()).join("skills/myskill");
        assert!(link.exists(), "linked skill should exist");
    }
}
