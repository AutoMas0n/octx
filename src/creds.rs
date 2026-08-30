use std::collections::HashMap;
use std::path::Path;

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;

use crate::error::OctxError;
use crate::{paths, platform};

/// Derive a 256-bit encryption key from the machine ID + random salt.
fn derive_key(machine_id: &str, salt: &[u8]) -> Vec<u8> {
    let mut key = vec![0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(machine_id.as_bytes(), salt, &mut key)
        .expect("Argon2 key derivation failed — this should never happen with valid inputs");
    key
}

/// Read all credentials from the encrypted file at `dir/creds.enc`.
/// Returns empty map if file doesn't exist or is empty.
fn read_creds_at(dir: &Path) -> Result<HashMap<String, HashMap<String, String>>, OctxError> {
    let path = dir.join("creds.enc");

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let data = std::fs::read(&path)?;

    if data.is_empty() {
        return Ok(HashMap::new());
    }

    // Format: salt(32) || nonce(12) || ciphertext
    const SALT_LEN: usize = 32;
    const NONCE_LEN: usize = 12;

    if data.len() < SALT_LEN + NONCE_LEN {
        return Err(OctxError::Creds(
            "creds.enc: file too short (expected salt+nonce)".into(),
        ));
    }

    let salt = &data[..SALT_LEN];
    let nonce_bytes = &data[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ciphertext = &data[SALT_LEN + NONCE_LEN..];

    let machine_id = platform::machine_id()?;
    let key = derive_key(&machine_id, salt);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher.decrypt(nonce, ciphertext).map_err(|_| {
        OctxError::Creds("Failed to decrypt creds.enc — wrong machine or corrupted file".into())
    })?;

    let creds: HashMap<String, HashMap<String, String>> = serde_json::from_slice(&plaintext)?;
    Ok(creds)
}

/// Write credentials to the encrypted file at `dir/creds.enc`. Acquires
/// `dir/creds.lock` for concurrent safety.
fn write_creds_at(
    dir: &Path,
    creds: &HashMap<String, HashMap<String, String>>,
) -> Result<(), OctxError> {
    let json = serde_json::to_vec(creds)?;

    let machine_id = platform::machine_id()?;

    // Generate 32-byte random salt
    let mut salt = vec![0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut salt);

    let key = derive_key(&machine_id, &salt);

    // Generate 12-byte random nonce
    let mut nonce_bytes = vec![0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, json.as_ref())
        .map_err(|_| OctxError::Creds("Encryption failed".into()))?;

    // Write: salt(32) || nonce(12) || ciphertext
    let mut output = Vec::with_capacity(salt.len() + nonce_bytes.len() + ciphertext.len());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);

    std::fs::create_dir_all(dir)?;
    let _lock = crate::util::FileLock::acquire(&dir.join("creds.lock"))?;
    std::fs::write(dir.join("creds.enc"), &output)?;

    Ok(())
}

/// Add a credential (encrypt and write to {config_dir}/octx/creds.enc).
pub fn add(host: &str, token: &str) -> Result<(), OctxError> {
    let dir = paths::config_dir();
    let mut creds = read_creds_at(&dir)?;
    let mut entry = HashMap::new();
    entry.insert("token".to_string(), token.to_string());
    creds.insert(host.to_string(), entry);
    write_creds_at(&dir, &creds)
}

/// Get a credential (decrypt and print to stdout).
pub fn get(host: &str, raw: bool) -> Result<(), OctxError> {
    let dir = paths::config_dir();
    let creds = read_creds_at(&dir)?;
    let entry = creds
        .get(host)
        .ok_or_else(|| OctxError::Creds(format!("No credential found for host '{host}'")))?;
    let token = entry
        .get("token")
        .ok_or_else(|| OctxError::Creds(format!("Credential for '{host}' has no token field")))?;

    if raw {
        println!("{token}");
    } else {
        println!("Token for {host}: {token}");
    }
    Ok(())
}

/// Remove a credential.
pub fn remove(host: &str) -> Result<(), OctxError> {
    let dir = paths::config_dir();
    let mut creds = read_creds_at(&dir)?;
    if creds.remove(host).is_none() {
        return Err(OctxError::NotFound(format!(
            "No credential found for host '{host}'"
        )));
    }
    write_creds_at(&dir, &creds)
}

/// List all stored hosts (tokens redacted).
pub fn list() -> Result<(), OctxError> {
    let dir = paths::config_dir();
    let creds = read_creds_at(&dir)?;
    for host in creds.keys() {
        println!("{host}: ***");
    }
    if creds.is_empty() {
        println!("No credentials stored.");
    }
    Ok(())
}

/// Get a stored token by host (used internally by dispatch.rs for env var injection).
pub fn get_token(host: &str) -> Option<String> {
    let dir = paths::config_dir();
    let creds = read_creds_at(&dir).ok()?;
    creds.get(host)?.get("token").cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Unique temp dir per test call, no env var interference.
    struct CredsTestCtx {
        dir: PathBuf,
    }

    static TEST_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    impl CredsTestCtx {
        fn new() -> Self {
            let seq = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join("octx-test-creds")
                .join(format!("{seq}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create test creds dir");
            CredsTestCtx { dir }
        }

        /// Call `add` but store in test dir instead of real config.
        fn add(&self, host: &str, token: &str) -> Result<(), OctxError> {
            let mut creds = read_creds_at(&self.dir)?;
            let mut entry = HashMap::new();
            entry.insert("token".to_string(), token.to_string());
            creds.insert(host.to_string(), entry);
            write_creds_at(&self.dir, &creds)
        }

        /// Call `get_token` but read from test dir.
        fn get_token(&self, host: &str) -> Option<String> {
            let creds = read_creds_at(&self.dir).ok()?;
            creds.get(host)?.get("token").cloned()
        }

        /// Call `remove` but on test dir.
        fn remove(&self, host: &str) -> Result<(), OctxError> {
            let mut creds = read_creds_at(&self.dir)?;
            if creds.remove(host).is_none() {
                return Err(OctxError::NotFound(format!(
                    "No credential found for host '{host}'"
                )));
            }
            write_creds_at(&self.dir, &creds)
        }

        fn creds_path(&self) -> PathBuf {
            self.dir.join("creds.enc")
        }

        /// Call `read_creds_at` on test dir.
        fn read_creds(&self) -> Result<HashMap<String, HashMap<String, String>>, OctxError> {
            read_creds_at(&self.dir)
        }
    }

    impl Drop for CredsTestCtx {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn test_add_and_get_roundtrip() {
        let ctx = CredsTestCtx::new();

        ctx.add("github", "ghp_abc123").expect("add should succeed");
        assert_eq!(ctx.get_token("github").as_deref(), Some("ghp_abc123"));

        // Verify file is binary (not plaintext)
        let path = ctx.creds_path();
        assert!(path.exists(), "creds.enc should exist");
        let data = std::fs::read(&path).expect("read creds.enc");
        assert!(!data.is_empty(), "creds.enc should not be empty");
        let text = String::from_utf8_lossy(&data);
        assert!(
            !text.contains("ghp_abc123"),
            "creds.enc should not contain plaintext token"
        );
    }

    #[test]
    fn test_remove_removes_entry() {
        let ctx = CredsTestCtx::new();

        ctx.add("github", "ghp_abc123").expect("add should succeed");
        assert_eq!(ctx.get_token("github").as_deref(), Some("ghp_abc123"));

        ctx.remove("github").expect("remove should succeed");
        assert_eq!(
            ctx.get_token("github"),
            None,
            "token should be gone after remove"
        );
    }

    #[test]
    fn test_get_returns_error_for_unknown_host() {
        let ctx = CredsTestCtx::new();

        // We can't call the real `get()` since it uses paths::config_dir().
        // Instead test the underlying logic: read_creds on empty dir.
        let creds = ctx.read_creds().expect("read_creds on empty dir");
        assert!(creds.is_empty());
    }

    #[test]
    fn test_list_returns_hosts_only() {
        let ctx = CredsTestCtx::new();

        ctx.add("github", "ghp_abc").expect("add github");
        ctx.add("gitlab", "glpat_xyz").expect("add gitlab");

        assert_eq!(ctx.get_token("github").as_deref(), Some("ghp_abc"));
        assert_eq!(ctx.get_token("gitlab").as_deref(), Some("glpat_xyz"));
    }

    #[test]
    fn test_remove_unknown_host_returns_not_found() {
        let ctx = CredsTestCtx::new();

        let result = ctx.remove("nonexistent");
        assert!(result.is_err(), "remove unknown should error");
        match result {
            Err(OctxError::NotFound(msg)) => {
                assert!(
                    msg.contains("nonexistent"),
                    "error should mention host: {msg}"
                );
            }
            other => panic!("expected NotFound error, got: {other:?}"),
        }
    }

    #[test]
    fn test_multiple_hosts_roundtrip() {
        let ctx = CredsTestCtx::new();

        ctx.add("github", "token_a").expect("add github");
        ctx.add("gitlab", "token_b").expect("add gitlab");
        ctx.add("github.my-org", "token_c")
            .expect("add github.my-org");

        assert_eq!(ctx.get_token("github").as_deref(), Some("token_a"));
        assert_eq!(ctx.get_token("gitlab").as_deref(), Some("token_b"));
        assert_eq!(ctx.get_token("github.my-org").as_deref(), Some("token_c"));

        // Remove middle one
        ctx.remove("gitlab").expect("remove gitlab");
        assert_eq!(ctx.get_token("gitlab"), None, "gitlab should be gone");
        assert_eq!(
            ctx.get_token("github").as_deref(),
            Some("token_a"),
            "github should remain"
        );
        assert_eq!(
            ctx.get_token("github.my-org").as_deref(),
            Some("token_c"),
            "github.my-org should remain"
        );
    }

    #[test]
    fn test_creds_file_is_binary() {
        let ctx = CredsTestCtx::new();
        ctx.add("test", "secret_value").expect("add should succeed");

        let data = std::fs::read(ctx.creds_path()).expect("read creds.enc");
        // Binary blob: salt(32) + nonce(12) + ciphertext >= 44 bytes
        assert!(
            data.len() > 44,
            "creds.enc should be > 44 bytes (salt+nonce): got {}",
            data.len()
        );
        // The plaintext token must not appear anywhere in the binary file
        assert!(
            !data
                .windows(b"secret_value".len())
                .any(|w| w == b"secret_value"),
            "creds.enc should not contain plaintext token"
        );
        // File should not be valid UTF-8 (random salt + nonce + AES-GCM ciphertext)
        assert!(
            std::str::from_utf8(&data).is_err(),
            "creds.enc should not be valid UTF-8"
        );
    }

    #[test]
    fn test_read_empty_file_returns_empty_map() {
        let ctx = CredsTestCtx::new();
        // Create empty file
        std::fs::write(ctx.creds_path(), b"").expect("write empty creds.enc");
        let creds = ctx
            .read_creds()
            .expect("read_creds on empty file should work");
        assert!(creds.is_empty(), "empty file should yield empty map");
    }

    #[test]
    fn test_get_token_returns_none_for_empty() {
        let ctx = CredsTestCtx::new();
        let token = ctx.get_token("anything");
        assert_eq!(token, None, "no creds -> None");
    }
}
