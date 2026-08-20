use std::sync::OnceLock;

use crate::OctxError;

/// Detects the current platform's target triple for downloading the correct binary.
///
/// Returns strings like `"x86_64-unknown-linux-musl"`, `"aarch64-unknown-linux-musl"`, etc.
/// Cached via `OnceLock` — `uname -m` runs at most once per process lifetime.
pub fn detect() -> &'static str {
    static TRIPLE: OnceLock<String> = OnceLock::new();
    TRIPLE.get_or_init(|| {
        detect_inner().expect(
            "octx: unsupported platform — run \"uname -m\" and open an issue with the output",
        )
    })
}

fn detect_inner() -> Result<String, OctxError> {
    let output = std::process::Command::new("uname")
        .arg("-m")
        .output()
        .map_err(OctxError::Io)?;

    let arch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let triple = match arch.as_str() {
        "x86_64" => "x86_64-unknown-linux-musl",
        "aarch64" => "aarch64-unknown-linux-musl",
        "armv6l" => "armv6-unknown-linux-gnueabihf",
        "armv7l" => "armv7-unknown-linux-gnueabihf",
        "arm64" => "aarch64-unknown-linux-musl",
        other => {
            return Err(OctxError::UnsupportedPlatform(format!(
                "uname -m returned \"{other}\" — no known target triple"
            )));
        }
    };
    Ok(triple.to_string())
}

/// Returns the machine ID for credential encryption.
///
/// On Linux reads `/etc/machine-id` (falls back to `/var/lib/dbus/machine-id`).
/// On macOS parses `ioreg -rd1 -c IOPlatformExpertDevice` for `IOPlatformUUID`.
/// On Windows returns `Err(UnsupportedPlatform)`.
// ponytail: Windows machine ID is a stub. Implement when adding full Windows support.
pub fn machine_id() -> Result<String, OctxError> {
    // Linux: /etc/machine-id or /var/lib/dbus/machine-id
    for path in &["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = std::fs::read_to_string(path) {
            let trimmed = id.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(trimmed);
            }
        }
    }

    // macOS: parse ioreg output
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let out = String::from_utf8_lossy(&output.stdout);
            for line in out.lines() {
                if line.trim().contains("IOPlatformUUID") {
                    if let Some(val) = line.split('=').nth(1) {
                        let id = val.trim().trim_matches('"').to_string();
                        if !id.is_empty() {
                            return Ok(id);
                        }
                    }
                }
            }
        }
    }

    Err(OctxError::UnsupportedPlatform(
        "machine-id: no known machine-id file or command found on this platform".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_non_empty_string() {
        let triple = detect();
        assert!(
            !triple.is_empty(),
            "detect() should return a non-empty triple"
        );
    }

    #[test]
    fn test_detect_returns_expected_format() {
        let triple = detect();
        // On Linux the triple should contain "linux"; on macOS the triple is still
        // linux-format since we map macOS arches to linux-musl triples.
        #[cfg(target_os = "linux")]
        assert!(
            triple.contains("linux"),
            "on Linux, triple should contain 'linux': got {triple}"
        );
        // On any platform, triple should have at least two hyphens
        assert!(
            triple.chars().filter(|&c| c == '-').count() >= 2,
            "triple should contain at least 2 hyphens: got {triple}"
        );
    }

    #[test]
    #[ignore = "requires /etc/machine-id or /var/lib/dbus/machine-id to exist"]
    fn test_machine_id_returns_some_string() {
        let id = machine_id().expect("machine_id() should succeed on this system");
        assert!(
            !id.is_empty(),
            "machine_id() should return a non-empty string"
        );
        assert!(
            !id.contains('\n'),
            "machine_id() should not contain newlines"
        );
    }
}
