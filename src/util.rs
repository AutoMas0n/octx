use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::OctxError;

/// Compute SHA-256 hex digest of a byte slice.
pub fn sha256_digest(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// An exclusive file lock released on Drop.
pub struct FileLock {
    file: std::fs::File,
    #[allow(dead_code)]
    path: PathBuf,
}

impl FileLock {
    /// Create or open file at path, acquire exclusive lock (blocking).
    pub fn acquire(path: &Path) -> Result<FileLock, OctxError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(OctxError::Io)?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(path)
            .map_err(OctxError::Io)?;
        file.lock_exclusive().map_err(OctxError::Io)?;
        Ok(FileLock {
            file,
            path: path.to_path_buf(),
        })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Atomically install a binary: write to temp file, set permissions, rename into place.
pub fn install_binary(source: &Path, dest: &Path) -> Result<(), OctxError> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(OctxError::Io)?;
    }

    let tmp = dest.with_extension("tmp");
    fs::copy(source, &tmp).map_err(OctxError::Io)?;

    // Set executable permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perm = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&tmp, perm).map_err(OctxError::Io)?;
    }

    fs::rename(&tmp, dest).map_err(OctxError::Io)?;
    Ok(())
}

/// Verify that a file's SHA-256 matches the expected hex string.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<(), OctxError> {
    let mut file = fs::File::open(path).map_err(OctxError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(OctxError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected.to_lowercase() {
        return Err(OctxError::ChecksumMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// Create directory and all parents if they don't exist.
pub fn ensure_dir(path: &Path) -> Result<(), OctxError> {
    fs::create_dir_all(path).map_err(OctxError::Io)
}

// Helper to use fs2 traits
use fs2::FileExt;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_sha256_digest_returns_expected_hex() {
        // SHA-256 of empty string
        let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(sha256_digest(b""), empty_hash);

        // Known value
        let hello_hash = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert_eq!(sha256_digest(b"hello"), hello_hash);
    }

    #[test]
    fn test_file_lock_acquires_and_releases() {
        let dir = std::env::temp_dir().join("octx-test-lock");
        let lock_path = dir.join("test.lock");
        let _ = fs::remove_file(&lock_path);

        {
            let _lock = FileLock::acquire(&lock_path).expect("acquire lock should succeed");
            // Lock is held, try acquiring again in another thread
            let lock_path2 = lock_path.clone();
            let handle = std::thread::spawn(move || {
                // This should block until the first lock is released
                let _lock2 = FileLock::acquire(&lock_path2)
                    .expect("second acquire should eventually succeed");
            });
            // Give the thread time to block on the lock
            std::thread::sleep(std::time::Duration::from_millis(100));
            // Lock drops here, releasing it
            drop(_lock);
            // Thread should now complete
            handle.join().expect("thread should finish");
        }

        // Clean up
        let _ = fs::remove_file(&lock_path);
        let _ = fs::remove_dir(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_install_binary_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join("octx-test-install");
        let _ = fs::remove_dir_all(&dir);

        // Create a source file
        let src = dir.join("source.bin");
        fs::create_dir_all(&dir).unwrap();
        let mut f = fs::File::create(&src).unwrap();
        f.write_all(b"binary data").unwrap();
        drop(f);

        let dest = dir.join("bin").join("my-arm");
        install_binary(&src, &dest).expect("install_binary should succeed");

        let meta = fs::metadata(&dest).unwrap();
        let mode = meta.permissions().mode();
        // 0o755 or 0o100755 (some systems add file type bits)
        assert!(
            mode & 0o111 != 0,
            "binary should have executable bits set: mode={:#o}",
            mode
        );

        // Clean up
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_checksum_passes_on_match() {
        let dir = std::env::temp_dir().join("octx-test-checksum");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file_path = dir.join("test.bin");
        fs::write(&file_path, b"hello world").unwrap();
        let expected = sha256_digest(b"hello world");

        verify_checksum(&file_path, &expected).expect("checksum should match");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_checksum_fails_on_mismatch() {
        let dir = std::env::temp_dir().join("octx-test-checksum-fail");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file_path = dir.join("test.bin");
        fs::write(&file_path, b"hello world").unwrap();

        let result = verify_checksum(
            &file_path,
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(result.is_err(), "checksum mismatch should produce an error");

        if let Err(OctxError::ChecksumMismatch { expected, actual }) = result {
            assert_eq!(
                expected,
                "0000000000000000000000000000000000000000000000000000000000000000"
            );
            assert_eq!(actual, sha256_digest(b"hello world"));
        } else {
            panic!("expected ChecksumMismatch error");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_dir_creates_nested_dirs() {
        let dir = std::env::temp_dir()
            .join("octx-test-ensure")
            .join("a")
            .join("b")
            .join("c");
        let _ = fs::remove_dir_all(&dir);

        assert!(!dir.exists());
        ensure_dir(&dir).expect("ensure_dir should create nested dirs");
        assert!(dir.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_binary_is_atomic() {
        let dir = std::env::temp_dir().join("octx-test-atomic");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let src = dir.join("source.bin");
        fs::write(&src, b"final content").unwrap();

        let dest = dir.join("my-arm");
        install_binary(&src, &dest).expect("install_binary should succeed");

        // Verify dest has the correct content (not partial)
        let content = fs::read_to_string(&dest).unwrap();
        assert_eq!(content, "final content");

        // Verify temp file is gone
        assert!(!dest.with_extension("tmp").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_binary_source_file() {
        let dir = std::env::temp_dir().join("octx-test-install-src");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let src = dir.join("source.bin");
        fs::write(&src, b"hello from source").unwrap();
        let dest = dir.join("deployed");

        install_binary(&src, &dest).expect("install_binary should succeed");
        let content = fs::read_to_string(&dest).unwrap();
        assert_eq!(content, "hello from source");

        let _ = fs::remove_dir_all(&dir);
    }
}
