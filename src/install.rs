use std::fs;
use std::path::Path;

use reqwest::{Client, Response, StatusCode};

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
        project_installed_skill(name)?;
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

/// Path of the cached body that belongs to an ETag file.
///
/// `aidhd.etag` -> `aidhd.cache`, `aidhd.skill.etag` -> `aidhd.skill.cache`.
fn cache_path_for(etag_path: &Path) -> std::path::PathBuf {
    etag_path.with_extension("cache")
}

/// Download bytes from a URL with optional Authorization header and ETag caching.
/// Returns (bytes, was_cached).
///
/// The body is cached next to the ETag, so a `304 Not Modified` is served from
/// disk instead of failing. An ETag left behind by an older version has no body
/// beside it, so the request is repeated unconditionally rather than erroring:
/// upgrading octx must never break the first install.
pub async fn fetch(url: &str, etag_path: &Path) -> Result<(Vec<u8>, bool), OctxError> {
    let client = Client::builder().user_agent("octx/0.1.0").build()?;
    let cache_path = cache_path_for(etag_path);

    let cached_etag = fs::read_to_string(etag_path)
        .ok()
        .map(|s| s.trim().to_string());

    if let Some(ref etag_val) = cached_etag {
        let response = client
            .get(url)
            .header("If-None-Match", etag_val)
            .send()
            .await?;

        if response.status() == StatusCode::NOT_MODIFIED {
            if let Ok(bytes) = fs::read(&cache_path) {
                return Ok((bytes, true));
            }
            // No cached body (ETag written by an older octx): fall through to an
            // unconditional request.
        } else {
            return store_response(response, url, etag_path, &cache_path).await;
        }
    }

    let response = client.get(url).send().await?;
    store_response(response, url, etag_path, &cache_path).await
}

/// Persist a successful response body and its ETag, atomically for the body so
/// a crash cannot leave a half-written cache that a later 304 would serve.
async fn store_response(
    response: Response,
    url: &str,
    etag_path: &Path,
    cache_path: &Path,
) -> Result<(Vec<u8>, bool), OctxError> {
    let status = response.status();
    if !status.is_success() {
        return Err(OctxError::Http(format!(
            "fetch returned {status} for {url}"
        )));
    }

    let new_etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok().map(|s| s.trim().to_string()));
    let bytes = response.bytes().await?.to_vec();

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = cache_path.with_extension("cache.tmp");
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, cache_path)?;

    match new_etag {
        Some(etag_val) => fs::write(etag_path, etag_val)?,
        // Without an ETag the next request must be unconditional, or a stale
        // ETag would keep asking the server for a body we can no longer match.
        None => {
            let _ = fs::remove_file(etag_path);
        }
    }

    Ok((bytes, false))
}

/// Project an installed arm's skill into every registered agent directory.
///
/// Called after `install_skill` writes the canonical copy, so the skill is
/// visible to agents as soon as the arm is installed.
fn project_installed_skill(name: &str) -> Result<(), OctxError> {
    let source = paths::skills_dir().join(format!("{name}.md"));
    if !source.exists() {
        return Ok(());
    }

    for (_agent, dir) in crate::skills::link_list() {
        crate::skills::project_skill(name, &source, Path::new(&dir))?;
    }

    Ok(())
}

/// Download a skill file from a URL and save to {config_dir}/skills/<name>.md
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

    /// Serve one canned response on a loopback port; returns its URL.
    fn serve_once(body: Vec<u8>) -> String {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback port");
        let addr = listener.local_addr().expect("read local addr");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });

        format!("http://{addr}/skill.md")
    }

    /// Serve a sequence of canned HTTP responses, one per connection, and
    /// return the requests received. Lets a test answer `200`, then `304`.
    fn serve_responses(
        responses: Vec<(&'static str, &'static str, Vec<u8>)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback port");
        let addr = listener.local_addr().expect("read local addr");

        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, headers, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 8192];
                let read = stream.read(&mut buf).unwrap_or(0);
                requests.push(String::from_utf8_lossy(&buf[..read]).to_string());

                let head = format!(
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
            requests
        });

        (format!("http://{addr}/bin"), handle)
    }

    /// A data dir the test owns, without touching the real one.
    fn test_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "octx-fetch-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn fetch_serves_a_cached_body_after_not_modified() {
        let dir = test_dir("cached");
        let etag_path = dir.join("thing.etag");
        let (url, server) = serve_responses(vec![
            ("200 OK", "ETag: \"v1\"\r\n", b"payload".to_vec()),
            ("304 Not Modified", "", Vec::new()),
        ]);

        let (bytes, cached) = fetch(&url, &etag_path).await.unwrap();
        assert_eq!(bytes, b"payload");
        assert!(!cached, "first fetch is not cached");
        assert_eq!(fs::read_to_string(&etag_path).unwrap(), "\"v1\"");

        // Second fetch must succeed from the cached body, not error.
        let (bytes, cached) = fetch(&url, &etag_path).await.unwrap();
        assert_eq!(bytes, b"payload");
        assert!(cached, "304 must be served from the cached body");

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests[1].to_lowercase().contains("if-none-match: \"v1\""),
            "the conditional header should be sent: {}",
            requests[1]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fetch_refetches_when_the_etag_has_no_cached_body() {
        // Reproduces an upgrade from a version that wrote only the ETag.
        let dir = test_dir("no-cache");
        let etag_path = dir.join("thing.etag");
        fs::write(&etag_path, "\"stale\"").unwrap();

        let (url, server) = serve_responses(vec![
            ("304 Not Modified", "", Vec::new()),
            ("200 OK", "ETag: \"v2\"\r\n", b"fresh".to_vec()),
        ]);

        let (bytes, cached) = fetch(&url, &etag_path).await.unwrap();
        assert_eq!(bytes, b"fresh", "must repeat the request unconditionally");
        assert!(!cached);
        assert_eq!(fs::read_to_string(&etag_path).unwrap(), "\"v2\"");
        assert_eq!(server.join().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fetch_drops_a_stale_etag_when_the_server_stops_sending_one() {
        let dir = test_dir("no-etag");
        let etag_path = dir.join("thing.etag");
        fs::write(&etag_path, "\"stale\"").unwrap();

        let (url, server) = serve_responses(vec![("200 OK", "", b"body".to_vec())]);

        let (bytes, _) = fetch(&url, &etag_path).await.unwrap();
        assert_eq!(bytes, b"body");
        assert!(
            !etag_path.exists(),
            "a response without an ETag must clear the stale one"
        );
        let _ = std::fs::remove_dir_all(&dir);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn test_install_skill_refetches_canonical_with_frontmatter() {
        let _data = crate::paths::TestDataDir::new("install-skill-refetch");
        let base = std::env::temp_dir().join(format!(
            "octx-skill-refetch-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        crate::paths::set_test_config_dir(base.clone());

        // A canonical skill installed before the upgrade carries no frontmatter.
        let skills_dir = paths::skills_dir();
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(
            skills_dir.join("fixture.md"),
            b"# fixture\n\n## Description\nOld.\n",
        )
        .unwrap();

        // The registry now publishes the skill with frontmatter.
        let published = "---\nname: fixture-octx\ndescription: A fixture arm.\n---\n\n# fixture\n";
        let url = serve_once(published.as_bytes().to_vec());

        install_skill("fixture", &url).await.unwrap();

        // The canonical copy is re-fetched with frontmatter.
        let canonical = std::fs::read_to_string(skills_dir.join("fixture.md")).unwrap();
        assert!(
            canonical.starts_with("---\nname: fixture-octx\n"),
            "canonical skill should carry frontmatter after re-fetch: {canonical}"
        );

        // And it projects to a loadable SKILL.md.
        let agent_dir = base.join("agent");
        std::fs::create_dir_all(&agent_dir).unwrap();
        crate::skills::project_skill("fixture", &skills_dir.join("fixture.md"), &agent_dir)
            .unwrap();
        let projection =
            std::fs::read_to_string(agent_dir.join("fixture-octx").join("SKILL.md")).unwrap();
        assert!(
            projection.starts_with("---\nname: fixture-octx\n"),
            "projected SKILL.md should be loadable: {projection}"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn test_project_installed_skill_projects_into_registered_links() {
        let base = std::env::temp_dir().join(format!(
            "octx-install-project-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        crate::paths::set_test_config_dir(base.clone());

        // The agent directory is registered before any skill exists.
        let agent_dir = base.join("agent");
        crate::skills::link_add("pi", agent_dir.to_str().unwrap()).unwrap();

        // An arm's canonical skill lands after the link is registered.
        let skills_dir = paths::skills_dir();
        std::fs::create_dir_all(&skills_dir).unwrap();
        std::fs::write(skills_dir.join("tool.md"), b"skill content").unwrap();

        project_installed_skill("tool").unwrap();

        let target = agent_dir.join("tool-octx").join("SKILL.md");
        assert!(
            target.exists(),
            "install should project <arm>-octx/SKILL.md into the registered link"
        );
    }

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
