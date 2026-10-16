use std::path::Path;

use crate::OctxError;

/// Project skill files from {config_dir}/skills/ into every directory octx
/// publishes to — registered links and detected agents alike — as
/// `<arm>-octx/SKILL.md`. Creates symlinks on Unix, hardlinks on Windows.
pub fn sync_all() -> Result<(), OctxError> {
    let skills_dir = crate::paths::skills_dir();
    if !skills_dir.exists() {
        return Ok(());
    }

    for (_agent, agent_dir) in link_list() {
        let target_dir = Path::new(&agent_dir);
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
                let _ = project_skill(name, &path, target_dir);
            }
        }
    }

    Ok(())
}

/// The discoverable skill name for an arm: the arm name suffixed with `-octx`,
/// so octx-provided skills are identifiable wherever an agent lists them. The
/// arm name itself stays the identity key; only the skill name carries the
/// suffix. The projection directory is derived from this, so it always agrees
/// with the `name` in the skill's frontmatter.
pub fn skill_name(arm: &str) -> String {
    format!("{arm}-octx")
}

/// Project a single arm's skill into an agent directory as `<arm>-octx/SKILL.md`,
/// the portable form an agent's skill loader reads. Removes projection forms an
/// earlier octx version wrote for this arm: a flat `<arm>.md`, and an
/// intermediate `<arm>/SKILL.md` directory whose `SKILL.md` resolves to `source`.
pub fn project_skill(arm: &str, source: &Path, agent_dir: &Path) -> Result<(), OctxError> {
    let dir = agent_dir.join(skill_name(arm));
    crate::util::ensure_dir(&dir)?;
    let target = dir.join("SKILL.md");

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

    remove_legacy_projections(arm, source, agent_dir);

    Ok(())
}

/// Remove projection forms an earlier octx version wrote for this arm: the flat
/// `<arm>.md`, and the intermediate `<arm>/SKILL.md` directory. The directory is
/// only removed when its `SKILL.md` resolves to `source`, so a user's own skill
/// directory that happens to share the arm's name is left untouched.
fn remove_legacy_projections(arm: &str, source: &Path, agent_dir: &Path) {
    let _ = std::fs::remove_file(agent_dir.join(format!("{arm}.md")));

    let ours = match (
        std::fs::canonicalize(agent_dir.join(arm).join("SKILL.md")),
        std::fs::canonicalize(source),
    ) {
        (Ok(link), Ok(src)) => link == src,
        _ => false,
    };
    if ours {
        let _ = std::fs::remove_dir_all(agent_dir.join(arm));
    }
}

/// Register a new agent link directory. Creates the directory if it doesn't
/// exist, adds it to config, and projects every installed arm's skill into it.
pub fn link_add(agent_name: &str, path: &str) -> Result<(), OctxError> {
    let target_dir = Path::new(path);
    crate::util::ensure_dir(target_dir)?;

    let mut cfg = crate::config::Config::load()?;
    cfg.add_link(agent_name, path);
    cfg.save()?;

    // Project every already-installed arm's skill into the new directory.
    sync_all()
}

/// Remove an arm's `<arm>-octx/` projection from an agent directory, along with
/// a flat `<arm>.md` left behind by an earlier octx version. The intermediate
/// `<arm>/` directory is left alone: without the canonical file it cannot be
/// confirmed as ours rather than a user's own skill directory.
pub fn unproject_skill(arm: &str, agent_dir: &Path) -> Result<(), OctxError> {
    let _ = std::fs::remove_dir_all(agent_dir.join(skill_name(arm)));
    let _ = std::fs::remove_file(agent_dir.join(format!("{arm}.md")));
    Ok(())
}

/// Remove a registered agent link. Does NOT delete the directory.
pub fn link_remove(agent_name: &str) -> Result<(), OctxError> {
    let mut cfg = crate::config::Config::load()?;
    cfg.remove_link(agent_name);
    cfg.save()
}

/// Every directory octx publishes skills into: explicitly registered links,
/// plus the skills directory of every detected agent. Deduplicated by path, so
/// a directory that is both registered and detected is published to once.
pub fn link_list() -> Vec<(String, String)> {
    let mut targets: Vec<(String, String)> = match crate::config::Config::load() {
        Ok(cfg) => cfg.links.unwrap_or_default().into_iter().collect(),
        Err(_) => Vec::new(),
    };

    for (name, path) in crate::agents::detected() {
        if !targets.iter().any(|(_, existing)| existing == &path) {
            targets.push((name, path));
        }
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Creates a unique temp config dir for test isolation.
    fn with_isolated_config(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("octx-test-skills").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::paths::set_test_config_dir(dir.clone());
        dir
    }

    #[test]
    fn test_link_add_registers_in_config() {
        let base = with_isolated_config("link-add");

        let agent_dir = base.join("agent");
        let result = link_add("pi", agent_dir.to_str().unwrap());
        assert!(result.is_ok(), "link_add should succeed");

        let cfg = crate::config::Config::load().unwrap();
        let links = cfg.links.unwrap();
        assert!(links.contains_key("pi"));
    }

    #[test]
    fn test_link_add_creates_directory() {
        let base = with_isolated_config("link-add-dir");

        let agent_dir = base.join("agent");
        assert!(!agent_dir.exists(), "dir should not exist before link_add");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();
        assert!(agent_dir.exists(), "dir should exist after link_add");
    }

    #[test]
    #[cfg(unix)]
    fn test_link_add_projects_existing_skills() {
        let base = with_isolated_config("link-add-projects");

        // An arm is already installed when the link is added.
        let skills_dir = crate::paths::skills_dir();
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(skills_dir.join("tool.md"), b"skill content").unwrap();

        let agent_dir = base.join("agent");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();

        let target = agent_dir.join("tool-octx").join("SKILL.md");
        assert!(
            target.exists(),
            "link_add should project every installed skill into the new directory"
        );
    }

    #[test]
    fn test_link_remove_removes_from_config() {
        let base = with_isolated_config("link-remove");

        let agent_dir = base.join("agent");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();
        link_remove("pi").unwrap();

        let cfg = crate::config::Config::load().unwrap();
        let links = cfg.links.unwrap_or_default();
        assert!(!links.contains_key("pi"));
    }

    #[test]
    fn test_link_list_returns_registered_links() {
        let base = with_isolated_config("link-list");

        let agent_dir = base.join("agent");
        link_add("pi", agent_dir.to_str().unwrap()).unwrap();

        let list = link_list();
        assert!(!list.is_empty(), "list should not be empty");
        assert!(list.iter().any(|(name, _)| name == "pi"));
    }

    #[test]
    #[cfg(unix)]
    fn test_project_skill_creates_directory() {
        let base = with_isolated_config("project-skill");

        let source = base.join("tool.md");
        fs::write(&source, b"skill content").unwrap();

        let agent_dir = base.join("agent");
        fs::create_dir_all(&agent_dir).unwrap();

        // A flat projection left behind by an earlier octx version.
        let legacy = agent_dir.join("tool.md");
        fs::write(&legacy, b"stale").unwrap();

        // An intermediate `<arm>/SKILL.md` projection from a development build.
        let intermediate = agent_dir.join("tool").join("SKILL.md");
        fs::create_dir_all(intermediate.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&source, &intermediate).unwrap();

        project_skill("tool", &source, &agent_dir).unwrap();

        let target = agent_dir.join("tool-octx").join("SKILL.md");
        assert!(target.exists(), "projection should be <arm>-octx/SKILL.md");
        assert!(target.is_symlink(), "target should be a symlink on Unix");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "skill content",
            "projected content should match source"
        );
        assert!(!legacy.exists(), "legacy flat <arm>.md should be removed");
        assert!(
            !agent_dir.join("tool").exists(),
            "intermediate <arm>/ projection should be removed"
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_sync_all_projects_into_every_link() {
        let base = with_isolated_config("sync-all-links");

        // A canonical skill in the skills directory.
        let skills_dir = crate::paths::skills_dir();
        fs::create_dir_all(&skills_dir).unwrap();
        fs::write(skills_dir.join("tool.md"), b"skill content").unwrap();

        // Two registered agent dirs, each with a stale flat projection.
        let dir_a = base.join("agent-a");
        let dir_b = base.join("agent-b");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();
        fs::write(dir_a.join("tool.md"), b"stale").unwrap();
        fs::write(dir_b.join("tool.md"), b"stale").unwrap();
        link_add("pi", dir_a.to_str().unwrap()).unwrap();
        link_add("claude", dir_b.to_str().unwrap()).unwrap();

        sync_all().unwrap();

        for dir in [&dir_a, &dir_b] {
            let target = dir.join("tool-octx").join("SKILL.md");
            assert!(target.exists(), "projection missing in {}", dir.display());
            assert_eq!(
                fs::read_to_string(&target).unwrap(),
                "skill content",
                "projected content should match source in {}",
                dir.display()
            );
            assert!(
                !dir.join("tool.md").exists(),
                "legacy flat file should be gone in {}",
                dir.display()
            );
        }
    }

    #[test]
    fn test_sync_all_with_no_links_does_nothing() {
        let _base = with_isolated_config("sync-all");

        // With no config/links, sync_all should return Ok without doing anything
        let result = sync_all();
        assert!(result.is_ok(), "sync_all with no links should succeed");
    }

    #[test]
    fn test_link_list_includes_detected_agents() {
        let base = with_isolated_config("link-list-detected");
        let home = base.join("home");
        fs::create_dir_all(home.join(".pi")).unwrap();
        crate::paths::set_test_home_dir(Some(home));

        let names: Vec<String> = link_list().into_iter().map(|(name, _)| name).collect();
        assert!(
            names.contains(&"pi".to_string()),
            "a detected agent should be listed: {names:?}"
        );

        crate::paths::set_test_home_dir(None);
    }

    #[test]
    fn test_link_list_deduplicates_a_registered_and_detected_path() {
        let base = with_isolated_config("link-list-dedup");
        let home = base.join("home");
        let pi_skills = home.join(".pi/agent/skills");
        fs::create_dir_all(&pi_skills).unwrap();
        crate::paths::set_test_home_dir(Some(home));

        // Register the very directory detection also finds, under the same name.
        link_add("pi", pi_skills.to_str().unwrap()).unwrap();

        let want = pi_skills.to_string_lossy().into_owned();
        let hits = link_list()
            .into_iter()
            .filter(|(_, path)| path == &want)
            .count();
        assert_eq!(
            hits, 1,
            "a path that is both registered and detected should be published to once"
        );

        crate::paths::set_test_home_dir(None);
    }
}
