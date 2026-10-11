//! Shared encrypted credential storage for octx and its arms.
//!
//! This crate holds the single implementation of credential storage: key
//! derivation from the machine identity, AES-256-GCM encryption at rest, and
//! retrieval. The head and every arm link it instead of reimplementing crypto.
//!
//! Credentials are handed out as [`Secret`] handles that redact themselves when
//! formatted, are not serializable, and zero their memory on drop. A caller
//! names exactly the host it needs; there is no bulk value export, and the
//! library never sources a credential from the environment or process arguments.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};

/// Length of the random salt prefix.
const SALT_LEN: usize = 32;
/// Length of the AES-GCM nonce.
const NONCE_LEN: usize = 12;

/// Errors from credential storage and retrieval.
#[derive(Debug, thiserror::Error)]
pub enum CredsError {
    /// Underlying I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The credential store could not be read, written, or decrypted.
    #[error("credential error: {0}")]
    Store(String),
    /// The credential store exists but cannot be decrypted.
    ///
    /// The message names the file and the remedy, and states that nothing was
    /// changed — an unreadable store is never discarded on the caller's behalf.
    #[error(
        "cannot read {path}: {reason}\n  the store was encrypted for a different machine, or the file is corrupted\n  nothing was changed — run `octx creds reset` to move it aside, then add credentials again"
    )]
    Unreadable {
        /// Path of the store file that could not be read.
        path: PathBuf,
        /// Why it could not be decrypted.
        reason: String,
    },
    /// The machine identity needed for key derivation is unavailable.
    #[error("machine identity unavailable: {0}")]
    MachineId(String),
    /// No configuration directory could be located.
    #[error("cannot locate the configuration directory")]
    NoConfigDir,
    /// The stored payload could not be (de)serialized.
    #[error("credential data error: {0}")]
    Data(#[from] serde_json::Error),
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, CredsError>;

/// The octx configuration directory (`{config_dir}/octx`), which holds
/// `creds.enc`.
pub fn config_dir() -> Result<PathBuf> {
    dirs::config_dir()
        .map(|dir| dir.join("octx"))
        .ok_or(CredsError::NoConfigDir)
}

/// The machine ID used to derive the encryption key.
///
/// Fallback chain:
/// 1. `/etc/machine-id` — Linux standard
/// 2. `/var/lib/dbus/machine-id` — Linux fallback
/// 3. `getprop ro.build.fingerprint` — Android/Termux (stable per device/ROM)
/// 4. macOS `ioreg IOPlatformUUID`
/// 5. `/proc/sys/kernel/random/boot_id` — container last resort (changes on boot)
pub fn machine_id() -> Result<String> {
    for path in &["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = fs::read_to_string(path) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }

    if let Ok(output) = std::process::Command::new("getprop")
        .arg("ro.build.fingerprint")
        .output()
    {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !out.is_empty() {
            return Ok(out);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("IOPlatformUUID")
                    && let Some(value) = line.split('=').nth(1)
                {
                    let id = value.trim().trim_matches('"').to_string();
                    if !id.is_empty() {
                        return Ok(id);
                    }
                }
            }
        }
    }

    if let Ok(id) = fs::read_to_string("/proc/sys/kernel/random/boot_id") {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    Err(CredsError::MachineId(
        "no known machine-id file or command found on this platform".into(),
    ))
}

/// A retrieved credential.
///
/// The value is redacted in every formatting context, the handle is not
/// serializable, and its memory is zeroed on drop. Plaintext is reachable only
/// through [`Secret::expose`] or the header helper [`Secret::bearer`].
pub struct Secret(SecretString);

impl Secret {
    /// Wrap a plaintext credential value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    /// Explicitly reveal the credential value.
    ///
    /// This is the only way to obtain the plaintext; callers should prefer
    /// [`Secret::bearer`] so the value never lands in a local string.
    #[must_use]
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }

    /// Build an `Authorization` header value (`Bearer <token>`) for an
    /// outbound request, marked sensitive so it redacts when logged.
    pub fn bearer(&self) -> Result<http::HeaderValue> {
        let mut value = http::HeaderValue::from_str(&format!("Bearer {}", self.expose()))
            .map_err(|_| CredsError::Store("credential is not a valid header value".into()))?;
        value.set_sensitive(true);
        Ok(value)
    }

    /// Zero the backing memory immediately. Called automatically on drop.
    pub fn zeroize(&mut self) {
        self.0 = SecretString::from(String::new());
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// An exclusive lock on `creds.lock`, released on drop.
struct Lock(fs::File);

impl Lock {
    fn acquire(path: &Path) -> Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .read(true)
            .open(path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Lock(file))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.sync_all();
    }
}

/// A handle to the encrypted credential store in a specific directory.
///
/// The directory and machine identity are injected so the store is testable
/// and reusable outside the head. Production callers use [`Store::open_default`].
pub struct Store {
    dir: PathBuf,
    machine_id: String,
}

impl Store {
    /// Open the store in the default configuration directory using the
    /// machine's own identity.
    pub fn open_default() -> Result<Self> {
        Ok(Self::open(config_dir()?, machine_id()?))
    }

    /// Open a store rooted at `dir`, encrypted for `machine_id`.
    pub fn open(dir: impl Into<PathBuf>, machine_id: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            machine_id: machine_id.into(),
        }
    }

    fn creds_path(&self) -> PathBuf {
        self.dir.join("creds.enc")
    }

    /// Add or replace the credential for `host`.
    pub fn add(&self, host: &str, token: &str) -> Result<()> {
        let mut creds = self.read()?;
        let mut entry = HashMap::new();
        entry.insert("token".to_string(), token.to_string());
        creds.insert(host.to_string(), entry);
        self.write(&creds)
    }

    /// Remove the credential for `host`.
    pub fn remove(&self, host: &str) -> Result<()> {
        let mut creds = self.read()?;
        if creds.remove(host).is_none() {
            return Err(CredsError::Store(format!(
                "no credential stored for host '{host}'"
            )));
        }
        self.write(&creds)
    }

    /// List the stored host names, sorted. Values are never returned.
    pub fn hosts(&self) -> Result<Vec<String>> {
        let creds = self.read()?;
        let mut names: Vec<String> = creds.keys().cloned().collect();
        names.sort();
        Ok(names)
    }

    /// Retrieve the credential for one host.
    pub fn get(&self, host: &str) -> Result<Secret> {
        let creds = self.read()?;
        let entry = creds.get(host).ok_or_else(|| {
            CredsError::Store(format!(
                "no credential stored for host '{host}' — add it with `octx creds add {host}`"
            ))
        })?;
        let token = entry.get("token").ok_or_else(|| {
            CredsError::Store(format!("credential for host '{host}' has no token field"))
        })?;
        Ok(Secret::new(token.clone()))
    }

    /// Path of the encrypted store file (`{dir}/creds.enc`).
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.creds_path()
    }

    /// Move the existing store aside so a new one can be created.
    ///
    /// The file is renamed, never deleted, to `creds.enc.bak` or the next free
    /// `creds.enc.bak.N`. This is the escape hatch for a store that cannot be
    /// decrypted (for example one copied from another machine), so it does not
    /// read or decrypt the file first. Returns the backup path, or `None` when
    /// there is no store to move aside.
    pub fn reset(&self) -> Result<Option<PathBuf>> {
        let path = self.creds_path();
        if !path.exists() {
            return Ok(None);
        }
        fs::create_dir_all(&self.dir)?;
        let _lock = Lock::acquire(&self.dir.join("creds.lock"))?;
        let backup = self.free_backup_path();
        fs::rename(&path, &backup)?;
        Ok(Some(backup))
    }

    /// First unused backup name (`creds.enc.bak`, then `.bak.1`, `.bak.2`, …).
    fn free_backup_path(&self) -> PathBuf {
        let base = self.dir.join("creds.enc.bak");
        if !base.exists() {
            return base;
        }
        for n in 1..1000 {
            let candidate = self.dir.join(format!("creds.enc.bak.{n}"));
            if !candidate.exists() {
                return candidate;
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        self.dir.join(format!("creds.enc.bak.{stamp}"))
    }

    /// Read and decrypt the whole store; an absent or empty file is empty.
    fn read(&self) -> Result<HashMap<String, HashMap<String, String>>> {
        let path = self.creds_path();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let data = fs::read(&path)?;
        if data.is_empty() {
            return Ok(HashMap::new());
        }
        if data.len() < SALT_LEN + NONCE_LEN {
            return Err(CredsError::Unreadable {
                path,
                reason: format!(
                    "file too short ({} bytes; expected at least {})",
                    data.len(),
                    SALT_LEN + NONCE_LEN
                ),
            });
        }

        let salt = &data[..SALT_LEN];
        let nonce_bytes = &data[SALT_LEN..SALT_LEN + NONCE_LEN];
        let ciphertext = &data[SALT_LEN + NONCE_LEN..];

        let key = derive_key(&self.machine_id, salt)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| CredsError::Unreadable {
                path,
                reason: "failed to decrypt (wrong machine or corrupted file)".into(),
            })?;

        Ok(serde_json::from_slice(&plaintext)?)
    }

    /// Encrypt and write the whole store, holding `creds.lock`.
    fn write(&self, creds: &HashMap<String, HashMap<String, String>>) -> Result<()> {
        let json = serde_json::to_vec(creds)?;

        let mut salt = vec![0u8; SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let key = derive_key(&self.machine_id, &salt)?;

        let mut nonce_bytes = vec![0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), json.as_ref())
            .map_err(|_| CredsError::Store("encryption failed".into()))?;

        let mut output = Vec::with_capacity(salt.len() + nonce_bytes.len() + ciphertext.len());
        output.extend_from_slice(&salt);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        fs::create_dir_all(&self.dir)?;
        let _lock = Lock::acquire(&self.dir.join("creds.lock"))?;
        fs::write(self.creds_path(), &output)?;
        Ok(())
    }
}

/// Derive the 256-bit AES key from the machine identity and a random salt.
fn derive_key(machine_id: &str, salt: &[u8]) -> Result<Vec<u8>> {
    let mut key = vec![0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(machine_id.as_bytes(), salt, &mut key)
        .map_err(|e| CredsError::Store(format!("key derivation failed: {e}")))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &Path, machine: &str) -> Store {
        Store::open(dir, machine)
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    #[test]
    fn round_trip_add_and_get() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");

        store.add("github", "ghp_abc123").expect("add");
        assert_eq!(store.get("github").expect("get").expose(), "ghp_abc123");
        assert_eq!(store.hosts().expect("hosts"), vec!["github".to_string()]);
    }

    #[test]
    fn remove_deletes_entry() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        store.add("github", "ghp_abc").expect("add");
        store.remove("github").expect("remove");
        assert!(store.get("github").is_err());
        assert!(store.hosts().expect("hosts").is_empty());
    }

    #[test]
    fn remove_unknown_host_names_it_and_fails() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        let err = store.remove("missing").expect_err("should fail");
        assert!(err.to_string().contains("missing"), "err: {err}");
    }

    #[test]
    fn wrong_machine_cannot_decrypt() {
        let dir = tempdir();
        store(dir.path(), "machine-a")
            .add("github", "ghp_abc")
            .expect("add with machine A");

        let other = store(dir.path(), "machine-b");
        let err = other.get("github").expect_err("wrong machine must fail");
        let text = err.to_string();
        assert!(text.contains("decrypt"), "err: {text}");
        assert!(!text.contains("ghp_abc"), "value must not leak: {text}");
    }

    #[test]
    fn unreadable_store_names_the_file_and_the_remedy() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        let junk = vec![7u8; SALT_LEN + NONCE_LEN + 16];
        fs::write(dir.path().join("creds.enc"), &junk).expect("write junk");

        let err = store.hosts().expect_err("junk must fail");
        let text = err.to_string();
        assert!(text.contains("creds.enc"), "must name the file: {text}");
        assert!(
            text.contains("octx creds reset"),
            "must name the remedy: {text}"
        );
        assert!(text.contains("nothing was changed"), "err: {text}");
        assert!(dir.path().join("creds.enc").exists(), "must not discard");
    }

    #[test]
    fn truncated_store_reports_a_readable_error() {
        let dir = tempdir();
        fs::write(dir.path().join("creds.enc"), [1u8, 2, 3]).expect("write short file");
        let err = store(dir.path(), "machine-a")
            .hosts()
            .expect_err("short file must fail");
        let text = err.to_string();
        assert!(text.contains("too short"), "err: {text}");
        assert!(text.contains("octx creds reset"), "err: {text}");
    }

    #[test]
    fn reset_moves_the_store_aside_without_reading_it() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        let junk = vec![9u8; SALT_LEN + NONCE_LEN + 16];
        fs::write(dir.path().join("creds.enc"), &junk).expect("write junk");

        let backup = store
            .reset()
            .expect("reset must not need to decrypt")
            .expect("a store existed");
        assert!(!dir.path().join("creds.enc").exists());
        assert_eq!(fs::read(&backup).expect("read backup"), junk);
        assert!(
            backup.to_string_lossy().contains("creds.enc.bak"),
            "backup: {backup:?}"
        );
    }

    #[test]
    fn reset_on_absent_store_reports_nothing_to_do() {
        let dir = tempdir();
        let outcome = store(dir.path(), "machine-a").reset().expect("reset");
        assert!(outcome.is_none());
    }

    #[test]
    fn reset_twice_keeps_every_backup() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        store.add("github", "first").expect("add");
        let one = store.reset().expect("reset").expect("store existed");
        store.add("github", "second").expect("add again");
        let two = store.reset().expect("reset").expect("store existed");

        assert_ne!(one, two, "backups must not overwrite each other");
        assert!(one.exists() && two.exists());
    }

    #[test]
    fn store_is_usable_again_after_reset() {
        let dir = tempdir();
        let first = store(dir.path(), "machine-a");
        first.add("github", "ghp_old").expect("add");
        first.reset().expect("reset").expect("store existed");

        let fresh = store(dir.path(), "machine-a");
        assert!(fresh.hosts().expect("hosts").is_empty());
        fresh.add("qobuz", "tok").expect("add after reset");
        assert_eq!(fresh.get("qobuz").expect("get").expose(), "tok");
    }

    #[test]
    fn empty_file_is_an_empty_store() {
        let dir = tempdir();
        fs::write(dir.path().join("creds.enc"), b"").expect("write empty");
        let store = store(dir.path(), "machine-a");
        assert!(store.hosts().expect("hosts").is_empty());
        assert!(store.get("anything").is_err());
    }

    #[test]
    fn missing_file_is_an_empty_store() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        assert!(store.hosts().expect("hosts").is_empty());
    }

    #[test]
    fn stored_file_contains_no_plaintext() {
        let dir = tempdir();
        store(dir.path(), "machine-a")
            .add("test", "secret_value")
            .expect("add");
        let data = fs::read(dir.path().join("creds.enc")).expect("read");
        assert!(data.len() > SALT_LEN + NONCE_LEN);
        assert!(
            !data
                .windows(b"secret_value".len())
                .any(|w| w == b"secret_value"),
            "plaintext must not appear in the store"
        );
    }

    #[test]
    fn unknown_host_error_names_the_host() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");
        let err = store.get("nope").expect_err("must fail");
        let text = err.to_string();
        assert!(text.contains("nope"), "err: {text}");
        assert!(text.contains("octx creds add nope"), "err: {text}");
    }

    #[test]
    fn secret_formatting_is_redacted() {
        let secret = Secret::new("supersecret");
        assert!(!format!("{secret}").contains("supersecret"));
        assert!(!format!("{secret:?}").contains("supersecret"));
    }

    #[test]
    fn secret_zeroize_clears_the_value() {
        let mut secret = Secret::new("supersecret");
        assert_eq!(secret.expose(), "supersecret");
        secret.zeroize();
        assert!(secret.expose().is_empty());
    }

    #[test]
    fn bearer_header_is_built_without_a_caller_string() {
        let secret = Secret::new("tok123");
        let header = secret.bearer().expect("header");
        assert_eq!(header.to_str().expect("str"), "Bearer tok123");
        assert!(header.is_sensitive(), "header should be marked sensitive");
    }

    #[test]
    fn library_ignores_credentials_from_the_environment() {
        let dir = tempdir();
        let store = store(dir.path(), "machine-a");

        // SAFETY: single-threaded test; the variable is only read by this test.
        unsafe {
            std::env::set_var("OCTX_TOKEN_GHOST", "env-secret");
            std::env::set_var("GHOST_TOKEN", "env-secret");
            std::env::set_var("GHOST_API_KEY", "env-secret");
        }

        assert!(store.hosts().expect("hosts").is_empty());
        assert!(store.get("ghost").is_err());

        unsafe {
            std::env::remove_var("OCTX_TOKEN_GHOST");
            std::env::remove_var("GHOST_TOKEN");
            std::env::remove_var("GHOST_API_KEY");
        }
    }

    #[test]
    fn machine_id_is_available_on_this_system() {
        let id = machine_id().expect("machine_id should resolve");
        assert!(!id.is_empty());
        assert!(!id.contains('\n'));
    }
}
