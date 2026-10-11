//! Opt-in live smoke tests against the real services.
//!
//! These are ignored by default so the ordinary `cargo test` run stays offline
//! and deterministic. Run them explicitly when real credentials are stored at
//! host `jam`:
//!
//! ```text
//! cargo test -p octx-jam --test live -- --ignored
//! ```
//!
//! Both commands select `--output json`, so they write a document to stdout and
//! never reach Qobuz: this is the "does the live read path work" check, not a
//! sync. The real endpoints and the default credential store are used — no
//! `OCTX_JAM_*` override is set.

use std::process::Command;

/// Run the real binary against the real services.
fn jam(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_jam"))
        .args(args)
        .output()
        .expect("run jam")
}

fn document(output: &std::process::Output, what: &str) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{what} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{what} did not emit a document ({error})"))
}

#[test]
#[ignore = "requires real credentials and network access; run with --ignored"]
fn recs_emits_a_document_without_mutating_any_playlist() {
    let output = jam(&["recs", "--count", "5", "--output", "json"]);
    let value = document(&output, "jam recs");
    assert_eq!(value["kind"], "playlist");
}

#[test]
#[ignore = "requires real credentials and network access; run with --ignored"]
fn radio_emits_a_document_without_mutating_any_playlist() {
    let output = jam(&[
        "radio",
        "--prompt",
        "artist:(queen)",
        "--mode",
        "easy",
        "--output",
        "json",
    ]);
    let value = document(&output, "jam radio");
    assert_eq!(value["kind"], "playlist");
}
