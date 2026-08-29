use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::OctxError;

/// Compute SHA-256 hex digest of a byte slice.
pub fn sha256_digest(data: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// An exclusive file lock that releases on Drop.
pub struct FileLock {
    file: std::fs::File,
    _path: PathBuf,
}

impl FileLock {
    /// Create or open file at path, acquire exclusive lock (blocking).
    pub fn acquire(path: &Path) -> Result<FileLock, OctxError> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(path)?;

        // Acquire exclusive lock (blocking)
        #[allow(unused_qualifications)]
        fs2::FileExt::lock_exclusive(&file)?;

        Ok(FileLock {
            file,
            _path: path.to_path_buf(),
        })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Unlock is automatically handled by fs2 on file close/Drop
        // The file is closed when FileLock is dropped.
        let _ = self.file.sync_all();
    }
}

/// Atomically install a binary: write to temp, set permissions, rename into place.
///
/// source: where the downloaded bytes are
/// dest:   final path in {data_dir}/octx/bin/<name>
pub fn install_binary(source: &Path, dest: &Path) -> Result<(), OctxError> {
    // Create parent directories for dest
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // Read source bytes
    let data = fs::read(source)?;

    // Write to temp file
    let tmp = dest.with_extension("tmp");
    fs::write(&tmp, &data)?;

    // Set permissions to 0o755 on Unix
    #[cfg(unix)]
    {
        let metadata = fs::metadata(&tmp)?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tmp, perms)?;
    }

    // Atomic rename
    fs::rename(&tmp, dest)?;

    Ok(())
}

/// Verify that a file's SHA-256 matches the expected hex string.
pub fn verify_checksum(path: &Path, expected: &str) -> Result<(), OctxError> {
    let data = fs::read(path)?;
    let actual = sha256_digest(&data);

    // Compare lowercased
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
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_sha256_digest_returns_expected_hex() {
        // Known SHA-256 of "hello"
        let result = sha256_digest(b"hello");
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_digest_empty_input() {
        let result = sha256_digest(b"");
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_file_lock_acquires_and_releases() {
        let dir = std::env::temp_dir().join("octx-test-filelock");
        let path = dir.join("test.lock");

        // Clean up any stale lock file
        let _ = fs::remove_dir_all(&dir);

        // Acquire lock — should succeed
        let lock = FileLock::acquire(&path).expect("acquire should succeed");
        assert!(path.exists(), "lock file should exist");

        // Drop lock — releasing
        drop(lock);

        // Acquire again — should succeed (lock was released)
        let _lock2 = FileLock::acquire(&path).expect("re-acquire should succeed");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_lock_from_two_processes_blocks() {
        let dir = std::env::temp_dir().join("octx-test-filelock-block");
        let path = dir.join("concurrent.lock");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Acquire lock in main thread
        let _lock = FileLock::acquire(&path).expect("main thread acquire");

        let path_clone = path.clone();
        let thread_lock = Arc::new(std::sync::Mutex::new(false));
        let acquired = thread_lock.clone();

        // Spawn a thread that tries to acquire the same lock
        let handle = thread::spawn(move || {
            // Attempt to acquire — should block until _lock is dropped
            let result = FileLock::acquire(&path_clone);
            if result.is_ok() {
                let mut lock = acquired.lock().unwrap();
                *lock = true;
            }
        });

        // Give thread time to attempt acquisition
        thread::sleep(std::time::Duration::from_millis(200));

        // Thread should not have acquired the lock (still blocked)
        {
            let lock = thread_lock.lock().unwrap();
            assert!(!*lock, "thread should not have acquired lock yet");
        }

        // Drop the main thread's lock, releasing it
        drop(_lock);

        // Give thread time to acquire
        thread::sleep(std::time::Duration::from_millis(200));

        // Thread should now have acquired the lock
        {
            let lock = thread_lock.lock().unwrap();
            assert!(*lock, "thread should have acquired lock after release");
        }

        handle.join().expect("thread should finish");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_binary_sets_permissions() {
        let dir = std::env::temp_dir().join("octx-test-install-perms");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let source = dir.join("source.bin");
        let dest = dir.join("bin").join("tool");

        // Create source
        fs::write(&source, b"binary content").unwrap();

        // Install
        install_binary(&source, &dest).expect("install should succeed");

        assert!(dest.exists(), "dest binary should exist");
        assert_eq!(
            fs::read(&dest).unwrap(),
            b"binary content",
            "content should match"
        );

        #[cfg(unix)]
        {
            let metadata = fs::metadata(&dest).unwrap();
            let mode = metadata.permissions().mode();
            assert!(
                mode & 0o111 != 0,
                "binary should be executable: mode={:o}",
                mode
            );
        }

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_binary_is_atomic() {
        let dir = std::env::temp_dir().join("octx-test-install-atomic");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let source = dir.join("source.bin");
        let dest = dir.join("dest.bin");

        // Create destination with "old" content (simulating existing installation)
        fs::write(&dest, b"old content").unwrap();

        // Write new content to source
        let new_content = b"new binary content";
        fs::write(&source, new_content).unwrap();

        // Install
        install_binary(&source, &dest).expect("install should succeed");

        // Dest should have new content
        assert_eq!(
            fs::read(&dest).unwrap(),
            new_content,
            "dest should have new content"
        );

        // Temp file should not exist
        let tmp = dest.with_extension("tmp");
        assert!(!tmp.exists(), "temp file should be cleaned up after rename");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_binary_creates_parent_dirs() {
        let dir = std::env::temp_dir().join("octx-test-install-parents");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let source = dir.join("source.bin");
        let dest = dir.join("deeply").join("nested").join("dirs").join("tool");

        fs::write(&source, b"content").unwrap();

        install_binary(&source, &dest).expect("install should create parent dirs");
        assert!(dest.exists(), "dest should exist with created parents");

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_checksum_passes_on_match() {
        let dir = std::env::temp_dir().join("octx-test-checksum-pass");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("test.bin");
        let data = b"hello";
        fs::write(&path, data).unwrap();

        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        verify_checksum(&path, expected).expect("checksum should match");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_verify_checksum_fails_on_mismatch() {
        let dir = std::env::temp_dir().join("octx-test-checksum-fail");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let path = dir.join("test.bin");
        let data = b"hello";
        fs::write(&path, data).unwrap();

        let expected = "0000000000000000000000000000000000000000000000000000000000000000";
        let result = verify_checksum(&path, expected);
        assert!(result.is_err(), "checksum should fail on mismatch");

        match result {
            Err(OctxError::ChecksumMismatch {
                expected: e,
                actual: a,
            }) => {
                assert_eq!(e, expected);
                assert_eq!(
                    a,
                    "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                );
            }
            other => panic!("expected ChecksumMismatch, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_dir_creates_nested_dirs() {
        let dir = std::env::temp_dir().join("octx-test-ensure-dir");
        let _ = fs::remove_dir_all(&dir);

        let nested = dir.join("a").join("b").join("c");
        assert!(!nested.exists(), "should not exist yet");

        ensure_dir(&nested).expect("ensure_dir should create nested dirs");
        assert!(nested.exists(), "nested dir should now exist");
        assert!(nested.is_dir(), "should be a directory");

        // Ensure calling on existing dir is idempotent
        ensure_dir(&nested).expect("ensure_dir should be idempotent");

        let _ = fs::remove_dir_all(&dir);
    }
}
