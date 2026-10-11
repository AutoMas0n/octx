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
}
