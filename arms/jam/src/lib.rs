//! `jam`: ListenBrainz recommendations into a playlist document, and listens
//! back into ListenBrainz.
//!
//! Library code lives here so the binary stays thin and every rule below is
//! testable without a network: the credential document and its account
//! selection, the two normalized documents, the ListenBrainz and Last.fm
//! clients, listen identity, and the sinks.

pub mod cli;
pub mod config;
pub mod credentials;
pub mod discover;
pub mod document;
pub mod http;
pub mod lastfm;
pub mod lb;
pub mod qobuz;
pub mod run;
pub mod scrobble;
pub mod sink;
pub mod sync;
