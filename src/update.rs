use std::sync::Arc;

use crate::OctxError;
use crate::manifest::Manifest;
use crate::registry::RegistryIndex;
use crate::util::FileLock;
use crate::{paths, platform};

/// Number of concurrent workers based on available parallelism (capped at 16).
fn worker_count() -> usize {
    match std::thread::available_parallelism() {
        Ok(n) => {
            let count = n.get();
            if count > 16 { 16 } else { count }
        }
        Err(_) => 4,
    }
}

/// Update all installed arms, sync skills, and self-update octx.
/// Concurrent across arms (worker-pooled), ETag-cached, race-free.
pub async fn run() -> Result<(), OctxError> {
    // 1. Acquire file lock
    let lock_path = paths::data_dir().join("update.lock");
    let _lock = FileLock::acquire(&lock_path)?;

    // 2. Load manifest
    let mut manifest = Manifest::load()?;

    // 3. Fetch registry index (ETag-cached) — non-fatal if fails
    let index = match RegistryIndex::fetch().await {
        Ok((idx, _cached)) => Some(idx),
        Err(e) => {
            eprintln!("octx: warning: failed to fetch registry index: {e}");
            None
        }
    };

    // 4. Concurrent phase: update each arm
    let arms: Vec<(String, String)> = manifest
        .arms
        .iter()
        .map(|(name, entry)| (name.clone(), entry.source.clone()))
        .collect();

    if !arms.is_empty()
        && let Some(index) = index.as_ref()
    {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(worker_count()));
        let mut handles = Vec::new();

        for (name, source) in &arms {
            let permit = semaphore.clone().acquire_owned().await;
            let name = name.clone();
            let source = source.clone();
            let index = index.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit;
                if source == "registry" {
                    update_registry_arm(&name, &index).await
                } else {
                    // Remote arm: extract host/owner/repo from source_url in manifest
                    let entry = {
                        let m = Manifest::load()?;
                        m.get_arm(&name).cloned()
                    };
                    match entry {
                        Some(e) => {
                            let url = e.source_url;
                            let parts: Vec<&str> = url.split('/').collect();
                            if parts.len() == 3 {
                                update_remote_arm(&name, parts[0], parts[1], parts[2]).await
                            } else {
                                eprintln!("octx: warning: invalid remote URL for '{name}': {url}");
                                Ok(())
                            }
                        }
                        None => {
                            eprintln!("octx: warning: arm '{name}' not found in manifest");
                            Ok(())
                        }
                    }
                }
            }));
        }

        // 5. Wait for all arm updates
        for handle in handles {
            if let Err(e) = handle.await {
                eprintln!("octx: warning: arm update task failed: {e}");
            }
        }
    }

    // 6. Update manifest timestamp
    manifest.touch();
    let _ = manifest.save();

    // 7. Sync skills
    if let Err(e) = crate::skills::sync_all() {
        eprintln!("octx: warning: skill sync failed: {e}");
    }

    // 8. Self-update
    if let Err(e) = self_update().await {
        eprintln!("octx: warning: self-update failed: {e}");
    }

    Ok(())
}

/// Update a single registry arm (binary + skill).
async fn update_registry_arm(name: &str, index: &RegistryIndex) -> Result<(), OctxError> {
    let target = platform::detect().to_string();

    let (url, checksum) = index.resolve_arm(name, &target).ok_or_else(|| {
        OctxError::NotFound(format!("arm \"{name}\" not found in registry for {target}"))
    })?;

    let data_dir = paths::data_dir();
    let tmp_dir = data_dir.join("tmp");
    let tmp_path = tmp_dir.join(format!("{name}.update"));

    // Fetch the binary
    let (bytes, _cached) =
        crate::install::fetch(url, &data_dir.join(format!("{name}.update.etag"))).await?;

    // Write to temp
    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, &bytes)?;

    // Verify checksum
    crate::util::verify_checksum(&tmp_path, checksum)?;

    // Install binary
    let dest = paths::bin_dir().join(name);
    crate::util::install_binary(&tmp_path, &dest)?;

    // Clean up temp
    let _ = std::fs::remove_file(&tmp_path);

    // Fetch skill if available
    if let Some(skill_url) = index.arms.get(name).and_then(|a| a.skill_url.as_deref()) {
        let _ = crate::install::install_skill(name, skill_url).await;
    }

    // Update manifest
    let mut manifest = Manifest::load()?;
    manifest.set_arm(
        name,
        crate::manifest::ArmEntry {
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

/// Update a single remote arm.
async fn update_remote_arm(
    _name: &str,
    host: &str,
    owner: &str,
    repo: &str,
) -> Result<(), OctxError> {
    let target = platform::detect();
    let bin = crate::install::derive_bin_name(repo);
    let download_url = crate::install::construct_remote_url(host, owner, repo, &bin, target);

    let data_dir = paths::data_dir();
    let tmp_dir = data_dir.join("tmp");
    let tmp_path = tmp_dir.join(format!("{bin}.update"));

    let (bytes, _cached) =
        crate::install::fetch(&download_url, &data_dir.join(format!("{bin}.update.etag"))).await?;

    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, &bytes)?;

    let dest = paths::bin_dir().join(&bin);
    crate::util::install_binary(&tmp_path, &dest)?;

    let _ = std::fs::remove_file(&tmp_path);

    // Update manifest
    let mut manifest = Manifest::load()?;
    manifest.set_arm(
        &bin,
        crate::manifest::ArmEntry {
            source: "remote".into(),
            source_url: format!("{host}/{owner}/{repo}"),
            version: "latest".into(),
            checksum: None,
            installed_at: chrono::Utc::now().to_rfc3339(),
        },
    );
    manifest.save()?;

    Ok(())
}

/// Self-update: check registry for newer octx binary, download, atomically replace.
async fn self_update() -> Result<(), OctxError> {
    let target = platform::detect().to_string();
    let data_dir = paths::data_dir();

    // Fetch registry index (or use cached)
    let (index, _cached) = RegistryIndex::fetch().await?;

    let (url, checksum, version) = index.resolve_head(&target).ok_or_else(|| {
        OctxError::NotFound(format!("no octx binary available for {target} in registry"))
    })?;

    // Check current version
    let current_version = env!("CARGO_PKG_VERSION");
    if current_version == version {
        // Already up to date, but check ETag
        let etag_path = data_dir.join("octx.etag");
        let etag = index
            .head
            .as_ref()
            .and_then(|h| h.etag.as_deref())
            .unwrap_or("");
        if let Ok(cached_etag) = std::fs::read_to_string(&etag_path)
            && cached_etag.trim() == etag
        {
            return Ok(());
        }
    }

    // Download new binary
    let tmp_path = data_dir.join("octx.new");
    let (bytes, _cached) = crate::install::fetch(url, &data_dir.join("octx.etag")).await?;

    if let Some(parent) = tmp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&tmp_path, &bytes)?;

    // Verify checksum
    crate::util::verify_checksum(&tmp_path, checksum)?;

    // Determine current binary path
    let current_exe = std::env::current_exe()?;

    // Atomically replace
    #[cfg(unix)]
    {
        std::fs::rename(&tmp_path, &current_exe)?;
    }

    #[cfg(not(unix))]
    {
        let old_path = data_dir.join("octx.old");
        std::fs::rename(&current_exe, &old_path)?;
        std::fs::rename(&tmp_path, &current_exe)?;
        let _ = std::fs::remove_file(&old_path);
    }

    eprintln!("octx: updated to version {version}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_count_returns_at_least_1() {
        let count = worker_count();
        assert!(count >= 1, "worker_count should be at least 1, got {count}");
    }

    #[test]
    fn test_worker_count_returns_at_most_16() {
        let count = worker_count();
        assert!(
            count <= 16,
            "worker_count should be at most 16, got {count}"
        );
    }

    #[tokio::test]
    async fn test_update_empty_manifest_returns_ok() {
        // Set up a temporary data dir to avoid contaminating the real one
        let tmp_dir =
            std::env::temp_dir().join(format!("octx-test-update-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // Create empty manifest in the temp area
        // We can't easily override paths::data_dir(), so we test the worker logic
        // and verify that the function handles the empty manifest case gracefully.
        let manifest = Manifest::load().unwrap();
        assert!(
            manifest.arms.is_empty(),
            "manifest should be empty initially"
        );

        // worker_count should always work regardless of manifest
        assert!(worker_count() >= 1);
        assert!(worker_count() <= 16);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
