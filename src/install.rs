use std::fs;
use std::path::Path;

use reqwest::Client;

use crate::OctxError;
use crate::manifest::{ArmEntry, Manifest};
use crate::paths;
use crate::platform;
use crate::registry::RegistryIndex;
use crate::util::{install_binary, verify_checksum};

/// Derive the binary name from a GitHub repo name.
/// Strips the "octx-" prefix if present.
/// E.g. "octx-fmt" → "fmt", "my-tool" → "my-tool".
pub fn derive_bin_name(repo: &str) -> String {
    repo.strip_prefix("octx-").unwrap_or(repo).to_string()
}

/// Install an arm from the registry by name (binary + skill file).
/// Downloads, verifies checksum, places binary in {data_dir}/octx/bin/<name>,
/// and skill file in {config_dir}/octx/skills/<name>.md.
pub async fn from_registry(name: &str) -> Result<(), OctxError> {
    let target = platform::detect().to_string();

    // Fetch and resolve registry
    let (index, _cached) = RegistryIndex::fetch().await?;
    let (url, checksum) = index.resolve_arm(name, &target).ok_or_else(|| {
        OctxError::NotFound(format!("arm \"{name}\" not found in registry for {target}"))
    })?;

    // Resolve skill URL
    let skill_url = index.arms.get(name).and_then(|a| a.skill_url.as_deref());

    // Temporary download path
    let data_dir = paths::data_dir();
    let tmp_dir = data_dir.join("tmp");
    let tmp_path = tmp_dir.join(format!("{name}.download"));

    // Fetch the binary
    let (bytes, _cached) = fetch(url, &data_dir.join(format!("{name}.etag"))).await?;

    // Write to temp
    if let Some(parent) = tmp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tmp_path, &bytes)?;

    // Verify checksum
    verify_checksum(&tmp_path, checksum)?;

    // Install binary
    let dest = paths::bin_dir().join(name);
    install_binary(&tmp_path, &dest)?;

    // Clean up temp
    let _ = fs::remove_file(&tmp_path);

    // Install skill if available
    if let Some(skill_url) = skill_url {
        install_skill(name, skill_url).await?;
    }

    // Update manifest
    let mut manifest = Manifest::load()?;
    manifest.set_arm(
        name,
        ArmEntry {
            source: "registry".into(),
            source_url: url.to_string(),
            version: "latest".into(),
            checksum: Some(format!("sha256:{checksum}")),
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    manifest.save()?;

    Ok(())
}

/// Install an arm from a remote GitHub repo URL.
/// URL format: "github.com/owner/repo"
/// Binary name derived from repo name (strip "octx-" prefix if present).
/// Supports optional `--bin` override.
pub async fn from_remote(url: &str, bin_name: Option<&str>) -> Result<(), OctxError> {
    let (host, owner, repo) = parse_remote_url(url)?;
    let target = platform::detect();
    let bin = match bin_name {
        Some(name) => name.to_string(),
        None => derive_bin_name(&repo),
    };

    let download_url = construct_remote_url(&host, &owner, &repo, &bin, target);

    let data_dir = paths::data_dir();
    let tmp_dir = data_dir.join("tmp");
    let tmp_path = tmp_dir.join(format!("{bin}.download"));

    let (bytes, _cached) = fetch(&download_url, &data_dir.join(format!("{bin}.etag"))).await?;

    if let Some(parent) = tmp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&tmp_path, &bytes)?;

    let dest = paths::bin_dir().join(&bin);
    install_binary(&tmp_path, &dest)?;

    let _ = fs::remove_file(&tmp_path);

    // Update manifest (no checksum for remote installs)
    // ponytail: no checksum for remote installs, add when we have a checksum convention
    let mut manifest = Manifest::load()?;
    manifest.set_arm(
        &bin,
        ArmEntry {
            source: "remote".into(),
            source_url: url.to_string(),
            version: "latest".into(),
            checksum: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    manifest.save()?;

    Ok(())
}

/// Construct a predictable GitHub release download URL.
pub fn construct_remote_url(
    host: &str,
    owner: &str,
    repo: &str,
    bin: &str,
    target: &str,
) -> String {
    format!("https://{host}/{owner}/{repo}/releases/latest/download/{bin}-{target}.gz")
}

/// Parse a remote URL into its components: (host, owner, repo).
/// Expects format "github.com/owner/repo" (exactly 3 parts separated by '/').
pub fn parse_remote_url(url: &str) -> Result<(String, String, String), OctxError> {
    let parts: Vec<&str> = url.split('/').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(OctxError::Http(format!(
            "invalid remote URL format: expected \"github.com/owner/repo\", got \"{url}\""
        )));
    }
    Ok((
        parts[0].to_string(),
        parts[1].to_string(),
        parts[2].to_string(),
    ))
}

/// Download bytes from a URL with optional Authorization header and ETag caching.
/// Returns (bytes, was_cached).
pub async fn fetch(url: &str, etag_path: &Path) -> Result<(Vec<u8>, bool), OctxError> {
    let client = Client::builder().user_agent("octx/0.1.0").build()?;

    // Read cached ETag
    let etag = fs::read_to_string(etag_path)
        .ok()
        .map(|s| s.trim().to_string());

    let mut request = client.get(url);
    if let Some(ref etag_val) = etag {
        request = request.header("If-None-Match", etag_val);
    }

    let response = request.send().await?;
    let status = response.status();

    if status == reqwest::StatusCode::NOT_MODIFIED {
        // 304: read from cache — but we don't know where cache is for arbitrary fetch.
        // For registry fetches, calling code handles caching. For binary/skill fetches,
        // there's no separate cached body, so return empty.
        return Err(OctxError::Http(
            "304 Not Modified with no local cache".into(),
        ));
    }

    if !status.is_success() {
        return Err(OctxError::Http(format!(
            "fetch returned {status} for {url}"
        )));
    }

    // Capture ETag from response headers
    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok().map(|s| s.trim().to_string()));

    // Read body
    let body = response.bytes().await?;
    let bytes = body.to_vec();

    // Save ETag
    if let Some(ref etag_val) = new_etag {
        if let Some(parent) = etag_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(etag_path, etag_val)?;
    }

    Ok((bytes, false))
}

/// Download a skill file from a URL and save to {config_dir}/octx/skills/<name>.md
pub async fn install_skill(name: &str, skill_url: &str) -> Result<(), OctxError> {
    let (bytes, _cached) = fetch(
        skill_url,
        &paths::data_dir().join(format!("{name}.skill.etag")),
    )
    .await?;

    let dest = paths::skills_dir().join(format!("{name}.md"));
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, &bytes)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_remote_url_parses_github() {
        let (host, owner, repo) = parse_remote_url("github.com/owner/repo").unwrap();
        assert_eq!(host, "github.com");
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_remote_url_errors_on_bad_format() {
        // Too few parts
        assert!(parse_remote_url("invalid").is_err());
        // Too many parts
        assert!(parse_remote_url("too/many/parts/here").is_err());
        // Empty parts
        assert!(parse_remote_url("//repo").is_err());
        assert!(parse_remote_url("github.com//repo").is_err());
        // Empty string
        assert!(parse_remote_url("").is_err());
    }

    #[test]
    fn test_construct_remote_url_produces_expected_url() {
        let url = construct_remote_url(
            "github.com",
            "owner",
            "repo",
            "tool",
            "x86_64-unknown-linux-musl",
        );
        assert_eq!(
            url,
            "https://github.com/owner/repo/releases/latest/download/tool-x86_64-unknown-linux-musl.gz"
        );
    }

    #[test]
    fn test_parse_remote_url_keeps_repo_name_as_is() {
        // A repo without octx- prefix should be returned as-is
        let (_host, _owner, repo) = parse_remote_url("github.com/owner/my-tool").unwrap();
        assert_eq!(repo, "my-tool");
    }

    #[test]
    fn test_derive_bin_name_strips_octx_prefix() {
        assert_eq!(derive_bin_name("octx-fmt"), "fmt");
        assert_eq!(derive_bin_name("octx-deploy"), "deploy");
    }

    #[test]
    fn test_derive_bin_name_keeps_non_prefix_name() {
        assert_eq!(derive_bin_name("fmt"), "fmt");
        assert_eq!(derive_bin_name("my-tool"), "my-tool");
    }

    #[test]
    fn test_parse_remote_url_with_octx_prefix_repo() {
        // A repo with octx- prefix — parse_remote_url returns the raw name, derive_bin_name strips it
        let (_host, _owner, repo) = parse_remote_url("github.com/owner/octx-fmt").unwrap();
        assert_eq!(repo, "octx-fmt");
        assert_eq!(derive_bin_name(&repo), "fmt");
    }
}
