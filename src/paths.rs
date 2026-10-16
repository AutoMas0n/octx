use std::path::PathBuf;

// Thread-local override for config_dir in tests.
// Each test thread gets its own independent override,
// so concurrent tests never step on each other.
#[cfg(test)]
thread_local! {
    static TEST_CONFIG_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
    static TEST_DATA_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
    static TEST_HOME_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Redirect config_dir for testing. Sets the thread-local override.
/// The override persists for the duration of the test — no guard needed
/// because each thread starts with a fresh `None`.
#[cfg(test)]
pub fn set_test_config_dir(path: PathBuf) {
    TEST_CONFIG_DIR.with(|cell| {
        *cell.borrow_mut() = Some(path);
    });
}

/// Redirect data_dir for testing. Prefer [`TestDataDir`], which also cleans up.
#[cfg(test)]
pub fn set_test_data_dir(path: PathBuf) {
    TEST_DATA_DIR.with(|cell| {
        *cell.borrow_mut() = Some(path);
    });
}

/// Point `home_dir()` at a temporary root for one test, or clear it with `None`.
/// Callers should clear on drop, so thread-local state never leaks to the next
/// test to run on the same thread.
#[cfg(test)]
pub fn set_test_home_dir(path: Option<PathBuf>) {
    TEST_HOME_DIR.with(|cell| {
        *cell.borrow_mut() = path;
    });
}

/// A throwaway data directory for tests: points `data_dir()` at a unique
/// temporary directory and restores the real one on drop.
///
/// Any test that reaches `data_dir()` must hold one of these. The real
/// directory holds user state — `installed-manifest.json` among it — so a test
/// without it reads and writes state it does not own, and can delete it.
#[cfg(test)]
pub struct TestDataDir {
    path: PathBuf,
}

#[cfg(test)]
impl TestDataDir {
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "octx-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create test data dir");
        set_test_data_dir(path.clone());
        Self { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
        TEST_DATA_DIR.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}

/// Returns the user's home directory, or `None` when it cannot be determined.
///
/// This is the root agent detection reads to find configuration directories.
/// In the crate's own tests it returns a thread-local override and `None` when
/// unset — never the real home — so a test can never detect, and therefore never
/// write into, a developer's actual agent directories.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_HOME_DIR.with(|cell| cell.borrow().clone())
    }
    #[cfg(not(test))]
    {
        dirs::home_dir()
    }
}

/// Returns the data directory for binaries, cache, and state.
/// Linux:   ~/.local/share/octx/
/// macOS:   ~/Library/Application Support/octx/
/// Windows: C:\Users\<user>\AppData\Local\octx/
///
/// In tests, returns the test override path if one is set.
pub fn data_dir() -> PathBuf {
    #[cfg(test)]
    {
        let result = TEST_DATA_DIR.with(|cell| cell.borrow().clone());
        if let Some(override_path) = result {
            return override_path;
        }
    }
    dirs::data_dir()
        .expect("octx data_dir: home directory not found (are you in a sandbox?)")
        .join("octx")
}

/// Returns the config directory for config.toml, creds.enc, skills.
/// Linux:   ~/.config/octx/
/// macOS:   ~/Library/Application Support/octx/   (same as data on macOS)
/// Windows: C:\Users\<user>\AppData\Roaming\octx/
///
/// In tests, returns the test override path if one is set.
pub fn config_dir() -> PathBuf {
    #[cfg(test)]
    {
        let result = TEST_CONFIG_DIR.with(|cell| cell.borrow().clone());
        if let Some(override_path) = result {
            return override_path;
        }
    }
    dirs::config_dir()
        .expect("octx config_dir: home directory not found (are you in a sandbox?)")
        .join("octx")
}

/// Returns the bin directory where arm binaries are stored: {data_dir}/bin/
pub fn bin_dir() -> PathBuf {
    data_dir().join("bin")
}

/// Returns the skills directory where skill files are stored: {config_dir}/skills/
pub fn skills_dir() -> PathBuf {
    config_dir().join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_dir_returns_path_ending_with_octx() {
        let p = data_dir();
        assert!(
            p.ends_with("octx"),
            "data_dir should end with 'octx': {:?}",
            p
        );
    }

    #[test]
    fn test_config_dir_returns_path_ending_with_octx() {
        let p = config_dir();
        assert!(
            p.ends_with("octx"),
            "config_dir should end with 'octx': {:?}",
            p
        );
    }

    #[test]
    fn test_bin_dir_is_under_data_dir() {
        let d = data_dir();
        let b = bin_dir();
        assert_eq!(b, d.join("bin"), "bin_dir should be data_dir/bin");
    }

    #[test]
    fn test_skills_dir_is_under_config_dir() {
        let c = config_dir();
        let s = skills_dir();
        assert_eq!(
            s,
            c.join("skills"),
            "skills_dir should be config_dir/skills"
        );
    }

    #[test]
    fn test_data_dir_and_config_dir_are_different_paths_on_unix() {
        let d = data_dir();
        let c = config_dir();
        assert_ne!(d, c, "data_dir and config_dir must differ");
    }
}
