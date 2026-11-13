//! Carry user-authored pi configuration between devices through untrusted media.
//!
//! The device's own work is never overwritten: every change is appended to a
//! per-device series in a log, the log travels as one passphrase-encrypted blob,
//! and disagreements are resolved only when producing a view — and always
//! reported.
//!
//! ```text
//! collect -> append -> seal -> [user moves the blob] -> open -> append -> view -> apply
//! ```

pub mod apply;
pub mod blob;
pub mod cli;
pub mod collect;
pub mod consolidate;
pub mod crypto;
pub mod log;
pub mod paths;
pub mod record;

pub use cli::run;
pub use record::{Op, Record};
