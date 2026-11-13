//! Passphrase encryption for the wire blob.
//!
//! Argon2 derives a key from the passphrase and a per-blob salt; AES-256-GCM
//! seals the serialized log. Both primitives are already dependencies of
//! `octx-creds`, so this adds no new crypto dependency and no new credential
//! mechanism.
//!
//! Layout: `salt (32) || nonce (12) || ciphertext+tag`.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::Argon2;
use rand::RngCore;

/// Length of the random salt prefix.
pub const SALT_LEN: usize = 32;
/// Length of the AES-GCM nonce.
pub const NONCE_LEN: usize = 12;

/// Errors from sealing or opening the blob.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// The blob is too short to hold a salt and nonce.
    #[error("the blob is not a pi-sync blob: {0}")]
    Malformed(String),
    /// Key derivation failed.
    #[error("cannot derive the key: {0}")]
    Derive(String),
    /// The passphrase does not decrypt the blob, or it was tampered with.
    #[error("wrong passphrase, or the blob is corrupted or was written for another profile")]
    WrongPassphrase,
}

fn derive(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], CryptoError> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| CryptoError::Derive(e.to_string()))?;
    Ok(key)
}

/// Encrypt `plaintext` with `passphrase`, producing a self-contained blob.
pub fn seal(passphrase: &str, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);

    let key = derive(passphrase, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::Derive("encryption failed".into()))?;

    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

/// Decrypt a blob produced by [`seal`].
pub fn open(passphrase: &str, blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if blob.len() <= SALT_LEN + NONCE_LEN {
        return Err(CryptoError::Malformed(format!(
            "{} bytes is shorter than the {} byte header",
            blob.len(),
            SALT_LEN + NONCE_LEN
        )));
    }
    let (salt, rest) = blob.split_at(SALT_LEN);
    let (nonce_bytes, ciphertext) = rest.split_at(NONCE_LEN);

    let key = derive(passphrase, salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::WrongPassphrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_and_open_round_trip() {
        let blob = seal("correct horse", b"payload").unwrap();
        assert_eq!(open("correct horse", &blob).unwrap(), b"payload");
    }

    #[test]
    fn wrong_passphrase_fails_cleanly() {
        let blob = seal("correct horse", b"payload").unwrap();
        let error = open("wrong horse", &blob).unwrap_err();
        assert!(matches!(error, CryptoError::WrongPassphrase), "{error:?}");
    }

    #[test]
    fn short_blob_is_rejected_without_panicking() {
        assert!(matches!(open("x", b"tiny"), Err(CryptoError::Malformed(_))));
    }

    #[test]
    fn ciphertext_carries_no_plaintext() {
        let blob = seal("pw", b"lila-baby 192.168.2.113 defaultModel").unwrap();
        let haystack = String::from_utf8_lossy(&blob);
        assert!(!haystack.contains("lila-baby"));
        assert!(!haystack.contains("192.168"));
        assert!(!haystack.contains("defaultModel"));
    }

    #[test]
    fn two_seals_of_the_same_plaintext_differ() {
        let a = seal("pw", b"same").unwrap();
        let b = seal("pw", b"same").unwrap();
        assert_ne!(a, b, "salt and nonce must be fresh per seal");
        assert_eq!(open("pw", &a).unwrap(), open("pw", &b).unwrap());
    }
}
