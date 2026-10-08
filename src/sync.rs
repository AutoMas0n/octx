//! Storage sync: download, verify, and extract the storage archive.
//!
//! The storage archive carries non-binary assets (harness YAML, scripts,
//! templates) that arms resolve at runtime. `{data_dir}/octx/storage/` is a
//! canonical, read-only mirror of the repo's `storage/` tree — local edits are
//! owned by octx and replaced wholesale on every sync.

use std::path::Path;

use flate2::read::GzDecoder;
use reqwest::Client;
use tar::Archive;

use crate::OctxError;
use crate::paths;
use crate::registry::RegistryIndex;
use crate::util::{FileLock, verify_checksum};

const USER_AGENT: &str = "octx/0.1.0";

/// Sync the storage archive into `{data_dir}/octx/storage/`.
/// With `force`, the archive is re-downloaded and re-extracted regardless of ETag.
///
/// Serialized by the same `update.lock` that `octx update` uses.
pub async fn run(force: bool) -> Result<(), OctxError> {
    let _lock = FileLock::acquire(&paths::data_dir().join("update.lock"))?;
    sync_locked(force).await
}

/// Perform the sync without acquiring `update.lock`.
///
/// `octx update` already holds the lock, and flock is not reentrant — a second
/// acquisition in the same process would block forever — so update calls this.
pub(crate) async fn sync_locked(force: bool) -> Result<(), OctxError> {
    let data_dir = paths::data_dir();

    let (index, _cached) = RegistryIndex::fetch().await?;
    let Some((url, sha256, version)) = index.resolve_storage() else {
        println!("octx: nothing to sync (registry has no storage entry)");
        return Ok(());
    };

    let archive_path = data_dir.join("storage.tar.gz");
    let etag_path = data_dir.join("storage.tar.gz.etag");

    // A 304 means the cached archive already matches the release.
    let Some((bytes, new_etag)) = fetch_archive(url, &etag_path, force).await? else {
        println!("octx: storage is current (v{version})");
        return Ok(());
    };

    write_and_verify(&archive_path, &bytes, sha256)?;
    save_etag(&etag_path, new_etag.as_deref())?;
    extract_archive(&archive_path, &data_dir)?;

    println!("octx: synced storage v{version}");

    Ok(())
}

/// Conditionally fetch the storage archive, sending `If-None-Match` with the
/// cached ETag. Returns `None` on `304 Not Modified`. When `force` is true the
/// conditional header is omitted, so the server always returns the body.
///
/// `install::fetch` cannot be reused here: it errors on 304 because it has no
/// access to the cached body.
async fn fetch_archive(
    url: &str,
    etag_path: &Path,
    force: bool,
) -> Result<Option<(Vec<u8>, Option<String>)>, OctxError> {
    let client = Client::builder().user_agent(USER_AGENT).build()?;

    let mut request = client.get(url);
    if !force && let Some(etag) = read_etag(etag_path) {
        request = request.header("If-None-Match", etag);
    }

    let response = request.send().await?;
    let status = response.status();

    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }

    if !status.is_success() {
        return Err(OctxError::Http(format!(
            "storage fetch returned {status} for {url}"
        )));
    }

    let etag = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string());
    let bytes = response.bytes().await?.to_vec();

    Ok(Some((bytes, etag)))
}

/// Read a cached ETag, treating a missing or empty file as "no ETag".
fn read_etag(path: &Path) -> Option<String> {
    let etag = std::fs::read_to_string(path).ok()?;
    let etag = etag.trim();
    if etag.is_empty() {
        None
    } else {
        Some(etag.to_string())
    }
}

/// Persist the response ETag alongside the cached archive.
fn save_etag(path: &Path, etag: Option<&str>) -> Result<(), OctxError> {
    if let Some(etag) = etag {
        std::fs::write(path, etag)?;
    }
    Ok(())
}

/// Write the archive to disk, then verify its SHA-256 against the registry.
/// On mismatch the partial file is removed so a bad archive is never cached.
fn write_and_verify(path: &Path, bytes: &[u8], expected: &str) -> Result<(), OctxError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;

    if let Err(e) = verify_checksum(path, expected) {
        let _ = std::fs::remove_file(path);
        return Err(e);
    }

    Ok(())
}

/// Extract the archive into `{data_dir}/octx/storage/`, replacing any existing
/// content. Unpacking happens into a temp sibling first, so a failure leaves the
/// previous storage directory untouched.
fn extract_archive(archive_path: &Path, data_dir: &Path) -> Result<(), OctxError> {
    let tmp = data_dir.join(".storage.tmp");
    let dest = data_dir.join("storage");

    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;

    let file = std::fs::File::open(archive_path)?;
    let mut archive = Archive::new(GzDecoder::new(file));

    if let Err(e) = archive.unpack(&tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e.into());
    }

    swap_dirs(&tmp, &dest)
}

/// Replace `dest` with `tmp`, restoring the old directory if the swap fails.
fn swap_dirs(tmp: &Path, dest: &Path) -> Result<(), OctxError> {
    let old = dest.with_file_name(".storage.old");

    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }

    let had_old = dest.exists();
    if had_old {
        std::fs::rename(dest, &old)?;
    }

    match std::fs::rename(tmp, dest) {
        Ok(()) => {
            if had_old {
                let _ = std::fs::remove_dir_all(&old);
            }
            Ok(())
        }
        Err(e) => {
            if had_old {
                let _ = std::fs::rename(&old, dest);
            }
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory `.tar.gz` from (path, content) pairs.
    fn make_archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut builder = tar::Builder::new(gz);
        for (path, content) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, content.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("octx-sync-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_extract_archive_mirrors_tree_and_replaces_content() {
        let dir = temp_dir("extract");
        let archive_path = dir.join("storage.tar.gz");
        std::fs::write(
            &archive_path,
            make_archive(&[
                ("harnesses/develop-arm/harness.yaml", "name: develop-arm"),
                (".gitkeep", ""),
            ]),
        )
        .unwrap();

        extract_archive(&archive_path, &dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("storage/harnesses/develop-arm/harness.yaml"))
                .unwrap(),
            "name: develop-arm"
        );
        assert!(dir.join("storage/.gitkeep").exists());

        // A second extraction replaces stale content wholesale (read-only mirror).
        let stale = dir.join("storage/stale.txt");
        std::fs::write(&stale, "old").unwrap();
        extract_archive(&archive_path, &dir).unwrap();
        assert!(
            !stale.exists(),
            "stale content should be dropped on re-extract"
        );
        assert!(!dir.join(".storage.tmp").exists(), "tmp dir should be gone");
        assert!(!dir.join(".storage.old").exists(), "old dir should be gone");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
