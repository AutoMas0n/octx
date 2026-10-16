//! Known coding agents octx publishes skills to.
//!
//! octx links the agents a user actually runs, so an installed arm's skill is
//! visible without a manual `octx link`. Detection is presence-based: an agent
//! is linked only when its configuration directory already exists, so octx never
//! creates a directory for a tool the user does not have.

/// A coding agent whose skills directory octx knows.
pub struct KnownAgent {
    /// Link name, as shown by `octx link list`.
    pub name: &'static str,
    /// Home-relative directory whose existence means the agent is installed.
    pub home: &'static str,
    /// Home-relative skills directory octx publishes into.
    pub skills: &'static str,
}

/// The agents octx detects and links by default.
///
/// Adding an agent is one entry here, plus a test that its paths are right.
pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent {
        name: "claude",
        home: ".claude",
        skills: ".claude/skills",
    },
    KnownAgent {
        name: "pi",
        home: ".pi",
        skills: ".pi/agent/skills",
    },
];

/// The `(name, skills directory)` of every known agent present on this machine.
///
/// Presence means the agent's configuration directory exists. A missing agent is
/// skipped, never created.
pub fn detected() -> Vec<(String, String)> {
    let Some(home) = crate::paths::home_dir() else {
        return Vec::new();
    };

    KNOWN_AGENTS
        .iter()
        .filter(|agent| home.join(agent.home).is_dir())
        .map(|agent| {
            (
                agent.name.to_string(),
                home.join(agent.skills).to_string_lossy().into_owned(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A home directory for one test: a temporary root plus its override.
    /// Thread-local overrides start fresh, so nothing to restore.
    struct TestHome(PathBuf);

    impl TestHome {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "octx-home-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create test home");
            crate::paths::set_test_home_dir(Some(path.clone()));
            Self(path)
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
            crate::paths::set_test_home_dir(None);
        }
    }

    #[test]
    fn detected_finds_a_present_agent() {
        let home = TestHome::new("present");
        std::fs::create_dir_all(home.0.join(".pi")).unwrap();

        let found = detected();
        let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["pi"], "only pi is present");

        let expected = home.0.join(".pi/agent/skills");
        assert_eq!(
            found[0].1,
            expected.to_string_lossy(),
            "pi's skills directory should be the publish target"
        );
    }

    #[test]
    fn detected_ignores_an_absent_agent_and_creates_nothing() {
        let home = TestHome::new("absent");

        assert!(detected().is_empty(), "no agent directories exist");
        assert!(
            !home.0.join(".claude").exists(),
            "detection must not create an absent agent's directory"
        );
    }

    #[test]
    fn known_agents_cover_claude_and_pi() {
        let names: Vec<&str> = KNOWN_AGENTS.iter().map(|a| a.name).collect();
        assert!(names.contains(&"claude"), "claude must be in the table");
        assert!(names.contains(&"pi"), "pi must be in the table");

        let claude = KNOWN_AGENTS.iter().find(|a| a.name == "claude").unwrap();
        assert_eq!((claude.home, claude.skills), (".claude", ".claude/skills"));
        let pi = KNOWN_AGENTS.iter().find(|a| a.name == "pi").unwrap();
        assert_eq!((pi.home, pi.skills), (".pi", ".pi/agent/skills"));
    }

    #[test]
    fn no_home_override_detects_nothing() {
        // In tests home_dir() is None unless a test sets it, so no test
        // ever reaches the developer's real agent directories.
        assert!(detected().is_empty());
    }
}
