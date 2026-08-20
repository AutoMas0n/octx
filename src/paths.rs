use std::path::PathBuf;

/// Returns the data directory for binaries, cache, and state.
/// Linux:   ~/.local/share/octx/
/// macOS:   ~/Library/Application Support/octx/
/// Windows: C:\Users\<user>\AppData\Local\octx/
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .expect("octx data_dir: home directory not found (are you in a sandbox?)")
        .join("octx")
}

/// Returns the config directory for config.toml, creds.enc, skills.
/// Linux:   ~/.config/octx/
/// macOS:   ~/Library/Application Support/octx/   (same as data on macOS)
/// Windows: C:\Users\<user>\AppData\Roaming\octx/
pub fn config_dir() -> PathBuf {
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
