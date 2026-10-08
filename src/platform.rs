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
    let arch = uname("-m")?;
    Ok(triple_for(&arch, is_android())?.to_string())
}

/// Runs `uname <flag>` and returns the trimmed stdout.
fn uname(flag: &str) -> Result<String, OctxError> {
    let output = std::process::Command::new("uname")
        .arg(flag)
        .output()
        .map_err(OctxError::Io)?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Android reports `Linux` from `uname -s`, but it is not a Linux libc: it uses
/// bionic. Only `uname -o` distinguishes it (=> `Android`).
fn is_android() -> bool {
    uname("-o")
        .map(|o| o.eq_ignore_ascii_case("android"))
        .unwrap_or(false)
}

/// Maps an architecture (from `uname -m`) to the release target triple.
///
/// Android gets its own triples: a musl-static binary cannot resolve DNS there
/// (musl reads `/etc/resolv.conf`, which does not exist on Android), so Android
/// builds must link bionic and use Android's own resolver.
fn triple_for(arch: &str, is_android: bool) -> Result<&'static str, OctxError> {
    match (arch, is_android) {
        ("x86_64", false) => Ok("x86_64-unknown-linux-musl"),
        ("aarch64" | "arm64", false) => Ok("aarch64-unknown-linux-musl"),
        ("armv6l", false) => Ok("arm-unknown-linux-gnueabihf"),
        ("armv7l", false) => Ok("armv7-unknown-linux-gnueabihf"),
        ("aarch64" | "arm64", true) => Ok("aarch64-linux-android"),
        ("armv7l", true) => Ok("armv7-linux-androideabi"),
        ("x86_64", true) => Ok("x86_64-linux-android"),
        (other, true) => Err(OctxError::UnsupportedPlatform(format!(
            "uname -m returned \"{other}\" on Android — no known target triple"
        ))),
        (other, false) => Err(OctxError::UnsupportedPlatform(format!(
            "uname -m returned \"{other}\" — no known target triple"
        ))),
    }
}

/// Returns the machine ID for credential encryption.
///
/// Tries a fallback chain:
/// 1. `/etc/machine-id` — Linux standard
/// 2. `/var/lib/dbus/machine-id` — Linux fallback
/// 3. `getprop ro.build.fingerprint` — Android/Termux (stable per device/ROM)
/// 4. macOS `ioreg IOPlatformUUID`
/// 5. `/proc/sys/kernel/random/boot_id` — Docker/container last resort (changes on reboot)
// ponytail: Windows machine ID is a stub. Implement when adding full Windows support.
pub fn machine_id() -> Result<String, OctxError> {
    // 1. Standard Linux machine-id files
    for path in &["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = std::fs::read_to_string(path) {
            let trimmed = id.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(trimmed);
            }
        }
    }

    // 2. Android/Termux: getprop for a stable device/ROM fingerprint
    if let Ok(output) = std::process::Command::new("getprop")
        .arg("ro.build.fingerprint")
        .output()
    {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !out.is_empty() {
            return Ok(out);
        }
    }

    // 3. macOS: parse ioreg output
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

    // 4. Docker/container fallback: boot_id (changes per boot, but
    //    lets ephemeral environments function vs crashing outright).
    // ponytail: boot_id means creds die on container restart. Acceptable for
    //          ephemeral envs where the alternative is a hard error.
    if let Ok(id) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id") {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
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
    fn test_triple_for_android_maps_to_bionic_targets() {
        assert_eq!(
            triple_for("aarch64", true).unwrap(),
            "aarch64-linux-android"
        );
        assert_eq!(triple_for("arm64", true).unwrap(), "aarch64-linux-android");
        assert_eq!(
            triple_for("armv7l", true).unwrap(),
            "armv7-linux-androideabi"
        );
        assert_eq!(triple_for("x86_64", true).unwrap(), "x86_64-linux-android");
        // Android never resolves to a musl triple — those cannot do DNS there.
        assert!(triple_for("aarch64", true).unwrap().ends_with("-android"));
    }

    #[test]
    fn test_triple_for_linux_unchanged() {
        assert_eq!(
            triple_for("x86_64", false).unwrap(),
            "x86_64-unknown-linux-musl"
        );
        assert_eq!(
            triple_for("aarch64", false).unwrap(),
            "aarch64-unknown-linux-musl"
        );
        assert_eq!(
            triple_for("armv6l", false).unwrap(),
            "arm-unknown-linux-gnueabihf"
        );
        assert_eq!(
            triple_for("armv7l", false).unwrap(),
            "armv7-unknown-linux-gnueabihf"
        );
    }

    #[test]
    fn test_triple_for_unknown_arch_is_error() {
        assert!(triple_for("sparc64", false).is_err());
        assert!(triple_for("sparc64", true).is_err());
    }

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

    #[test]
    fn test_getprop_fallback_android() {
        // Only testable when getprop is on the system (Android/Termux).
        // Silently skipped on other platforms via early return.
        let Ok(output) = std::process::Command::new("getprop")
            .arg("ro.build.fingerprint")
            .output()
        else {
            return;
        };
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            !out.is_empty(),
            "getprop ro.build.fingerprint should be non-empty"
        );
        assert!(
            !out.contains('\n'),
            "getprop output should not contain newlines"
        );
    }

    #[test]
    fn test_boot_id_fallback() {
        // Testable on any Linux with procfs (most containers, some CI runners).
        // Silently skipped if the file doesn't exist.
        let Ok(id) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id") else {
            return;
        };
        let trimmed = id.trim().to_string();
        assert!(!trimmed.is_empty(), "boot_id should be non-empty");
        // boot_id is a UUID like "dd2e12c3-abc1-4ef5-8901-abcdef123456"
        assert!(
            trimmed.contains('-'),
            "boot_id should be a UUID (contain hyphens): got {trimmed:?}"
        );
    }
}
