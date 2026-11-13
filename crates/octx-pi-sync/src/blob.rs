//! Reading and writing the encrypted blob.
//!
//! The passphrase comes from the octx credential store under the host
//! `pi-sync`. Nothing here touches the network: getting the blob to its path is
//! the user's transport.

use std::fs;
use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use octx_creds::Store;

use crate::crypto;

/// The credential host whose value is the blob passphrase.
pub const PASSPHRASE_HOST: &str = "pi-sync";

/// Errors from blob I/O.
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// Reading or writing the blob failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The blob path.
        path: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The credential store could not supply a passphrase.
    #[error(
        "cannot read the pi-sync passphrase: {0}\n  add it once per device with: octx creds add pi-sync"
    )]
    Passphrase(String),
    /// Encryption or decryption failed.
    ///
    /// The crypto error is flattened into the message rather than kept as a
    /// `source`, so the reported chain states the cause once.
    #[error("the blob could not be decrypted: {0}")]
    Crypto(String),
}

fn io(path: &Path, source: std::io::Error) -> BlobError {
    BlobError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Read the passphrase from the octx credential store.
pub fn passphrase() -> Result<String, BlobError> {
    let store = Store::open_default().map_err(|e| BlobError::Passphrase(e.to_string()))?;
    let secret = store
        .get(PASSPHRASE_HOST)
        .map_err(|e| BlobError::Passphrase(e.to_string()))?;
    Ok(secret.expose().to_string())
}

/// Seal `plaintext` and write it to `path`, creating the parent directory.
pub fn write(path: &Path, passphrase: &str, plaintext: &[u8]) -> Result<(), BlobError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io(path, e))?;
    }
    let blob = crypto::seal(passphrase, plaintext).map_err(|e| BlobError::Crypto(e.to_string()))?;

    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    // The blob can carry a device's network topology; keep it owner-only even
    // though it is encrypted.
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(path).map_err(|e| io(path, e))?;
    file.write_all(&blob).map_err(|e| io(path, e))
}

/// Read and decrypt the blob at `path`.
pub fn read(path: &Path, passphrase: &str) -> Result<Vec<u8>, BlobError> {
    let blob = fs::read(path).map_err(|e| io(path, e))?;
    crypto::open(passphrase, &blob).map_err(|e| BlobError::Crypto(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/blob");
        write(&path, "pw", b"records").unwrap();
        assert_eq!(read(&path, "pw").unwrap(), b"records");
    }

    #[cfg(unix)]
    #[test]
    fn the_blob_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob");
        write(&path, "pw", b"secret network map").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "mode was {:o}", mode & 0o777);
    }

    #[test]
    fn missing_blob_names_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope");
        let error = read(&path, "pw").unwrap_err().to_string();
        assert!(error.contains("nope"), "{error}");
    }
}
