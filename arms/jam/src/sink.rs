//! Delivery: the documents, out.
//!
//! A sink receives a document and does not care which source produced it. The
//! `json` sinks are the no-network inspector on both sides of the arm.

use anyhow::{Context, Result};
use std::io::Write;

use crate::document::{ListenDocument, PlaylistDocument};
use crate::http::Http;
use crate::lb;

/// Write a playlist document to stdout.
pub fn write_playlist(document: &PlaylistDocument) -> Result<()> {
    write_json(document, "the playlist document")
}

/// Write a listen document to stdout.
pub fn write_listens(document: &ListenDocument) -> Result<()> {
    write_json(document, "the listen document")
}

/// Submit a listen document to ListenBrainz and report what it sent.
///
/// The endpoint answers `{"status":"ok"}` however many listens were new, so the
/// report counts what went out and what the input lost — never a count of
/// newly created listens, which the arm cannot observe.
pub async fn submit(
    http: &Http,
    token: &str,
    document: &ListenDocument,
    rejected: usize,
) -> Result<()> {
    lb::submit(http, token, &document.listens).await?;
    let submitted = document.listens.len();
    eprintln!(
        "jam: submitted {submitted} listen{} to ListenBrainz, {rejected} input line{} rejected",
        if submitted == 1 { "" } else { "s" },
        if rejected == 1 { "" } else { "s" }
    );
    Ok(())
}

/// Serialize one document to stdout.
fn write_json<T: serde::Serialize>(document: &T, what: &str) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    serde_json::to_writer_pretty(&mut out, document)
        .with_context(|| format!("could not write {what}"))?;
    writeln!(out).context("could not finish writing the document")?;
    out.flush().context("could not flush the document")?;
    Ok(())
}
