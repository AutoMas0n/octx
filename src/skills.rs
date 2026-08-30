use std::path::Path;

use crate::OctxError;

/// Copy skill files from {config_dir}/octx/skills/ to all linked agent directories.
/// Creates symlinks on Unix, hardlinks on Windows.
pub fn sync_all() -> Result<(), OctxError> {
    // Load config to get agent links
    let cfg = crate::config::Config::load()?;
    let links = match cfg.links {
        Some(links) => links,
        None => return Ok(()),
    };

    let skills_dir = crate::paths::skills_dir();
    if !skills_dir.exists() {
        return Ok(());
    }

    for agent_dir in links.values() {
        let target_dir = Path::new(agent_dir);
        crate::util::ensure_dir(target_dir)?;

        // Read all skill files in the skills directory
        let entries = match std::fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
            {
                let _ = sync_skill_to_dir(name, &path, target_dir);
            }
        }
    }

    Ok(())
}

/// Symlink/hardlink a single skill file to a target agent directory.
fn sync_skill_to_dir(skill_name: &str, source: &Path, target_dir: &Path) -> Result<(), OctxError> {
    let target = target_dir.join(format!("{skill_name}.md"));

    // Remove existing file/symlink at target
    let _ = std::fs::remove_file(&target);

    // Create symlink on Unix, hardlink on Windows
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, &target)?;
    }

    #[cfg(not(unix))]
    {
        std::fs::hard_link(source, &target)?;
    }

    Ok(())
}

/// Register a new agent link directory. Creates the directory if it doesn't exist.
/// Adds to config and saves.
pub fn link_add(agent_name: &str, path: &str) -> Result<(), OctxError> {
    let target_dir = Path::new(path);
    crate::util::ensure_dir(target_dir)?;

    let mut cfg = crate::config::Config::load()?;
    cfg.add_link(agent_name, path);
    cfg.save()
}

/// Remove a registered agent link. Does NOT delete the directory.
pub fn link_remove(agent_name: &str) -> Result<(), OctxError> {
    let mut cfg = crate::config::Config::load()?;
    cfg.remove_link(agent_name);
    cfg.save()
}

/// List all registered agent links.
pub fn link_list() -> Vec<(String, String)> {
    let cfg = match crate::config::Config::load() {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    match cfg.links {
        Some(links) => links.into_iter().collect(),
        None => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_link_add_registers_in_config() {
        let dir = std::env::temp_dir().join("octx-test-skills-link-add");
        let _ = fs::remove_dir_all(&dir);

        let agent_dir = dir.join("agent");
        let result = link_add("pi", agent_dir.to_str().unwrap());
        assert!(result.is_ok(), "link_add should succeed");

        let cfg = crate::config::Config::load().unwrap();
        let links = cfg.links.unwrap();
        assert!(links.contains_key("pi"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_link_add_creates_directory() {
        let dir = std::env::temp_dir().join("octx-test-skills-link-add-dir");
        let _ = fs::remove_dir_all(&dir);

        let agent_dir = dir.join("agent");
        assert!(!agent_dir.exists(), "dir should not exist before link_add");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();
        assert!(agent_dir.exists(), "dir should exist after link_add");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_link_remove_removes_from_config() {
        let dir = std::env::temp_dir().join("octx-test-skills-link-remove");
        let _ = fs::remove_dir_all(&dir);

        let agent_dir = dir.join("agent");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();
        link_remove("pi").unwrap();

        let cfg = crate::config::Config::load().unwrap();
        let links = cfg.links.unwrap_or_default();
        assert!(!links.contains_key("pi"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_link_list_returns_registered_links() {
        let dir = std::env::temp_dir().join("octx-test-skills-link-list");
        let _ = fs::remove_dir_all(&dir);

        let agent_dir = dir.join("agent");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();

        let list = link_list();
        assert!(!list.is_empty(), "list should not be empty");
        assert!(list.iter().any(|(name, _)| name == "pi"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_sync_skill_to_dir_creates_symlink() {
        let dir = std::env::temp_dir().join("octx-test-skills-symlink");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let source = dir.join("tool.md");
        fs::write(&source, b"skill content").unwrap();

        let target_dir = dir.join("agent");
        fs::create_dir_all(&target_dir).unwrap();

        sync_skill_to_dir("tool", &source, &target_dir).unwrap();

        let target = target_dir.join("tool.md");
        assert!(target.exists(), "target should exist after sync");
        assert!(target.is_symlink(), "target should be a symlink on Unix");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "skill content",
            "symlink target content should match source via symlink"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sync_all_with_no_links_does_nothing() {
        // With no config/links, sync_all should return Ok without doing anything
        let result = sync_all();
        assert!(result.is_ok(), "sync_all with no links should succeed");
    }
}
