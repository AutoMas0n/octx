//! Credential CLI glue.
//!
//! All storage, key derivation, and encryption live in the shared
//! [`octx_creds`] library; this module only adapts it to the head's error type
//! and the `octx creds` command output.

use crate::error::OctxError;
use octx_creds::{CredsError, Store};

/// Open the shared credential store using the machine's own identity.
fn store() -> Result<Store, OctxError> {
    Store::open_default().map_err(map_err)
}

/// Map a library error into the head's error type.
fn map_err(error: CredsError) -> OctxError {
    match error {
        CredsError::Io(io) => OctxError::Io(io),
        other => OctxError::Creds(other.to_string()),
    }
}

/// Add a credential (encrypt and write to `{config_dir}/octx/creds.enc`).
///
/// # Errors
/// Fails when the store cannot be read, encrypted, or written.
pub fn add(host: &str, token: &str) -> Result<(), OctxError> {
    store()?.add(host, token).map_err(map_err)
}

/// Print a credential for `host` to stdout.
///
/// # Errors
/// Fails when no credential is stored for `host`.
pub fn get(host: &str, raw: bool) -> Result<(), OctxError> {
    let secret = store()?.get(host).map_err(map_err)?;
    if raw {
        println!("{}", secret.expose());
    } else {
        println!("Token for {host}: {}", secret.expose());
    }
    Ok(())
}

/// Remove a credential.
///
/// # Errors
/// Fails when no credential is stored for `host`.
pub fn remove(host: &str) -> Result<(), OctxError> {
    store()?.remove(host).map_err(map_err)
}

/// List all stored hosts (tokens redacted).
///
/// # Errors
/// Fails when the store cannot be read.
pub fn list() -> Result<(), OctxError> {
    let hosts = store()?.hosts().map_err(map_err)?;
    if hosts.is_empty() {
        println!("No credentials stored.");
    } else {
        for host in hosts {
            println!("{host}: ***");
        }
    }
    Ok(())
}

/// Move an unreadable store aside so a new one can be created.
///
/// The file is renamed, never deleted, so a store that cannot be decrypted
/// (for example one copied from another machine) can be recovered manually.
///
/// # Errors
/// Fails when the store file cannot be moved.
pub fn reset() -> Result<(), OctxError> {
    let store = store()?;
    match store.reset().map_err(map_err)? {
        Some(backup) => println!(
            "Moved the credential store aside to: {}\nAdd credentials again with `octx creds add <HOST>`.",
            backup.display()
        ),
        None => println!(
            "No credential store to reset at: {}",
            store.path().display()
        ),
    }
    Ok(())
}
