//! The discovery memory, driven end to end through the binary.
//!
//! These prove the wiring around `state.rs`: that only a materialized Qobuz
//! delivery is remembered, that a rehearsal or a JSON dump spends nothing, and
//! that the next run excludes what the last one delivered.

mod support;

use std::sync::{Arc, Mutex};

use serde_json::json;
use support::{Mock, Response, Store, document, jam_with_env};

const LASTFM_ROUTE: &str = "/2.0/";

/// An account with a Last.fm user and a Qobuz token — enough for a discovery
/// delivery to Qobuz.
fn account() -> String {
    document(
        r#""jesse": {"lastfm": {"user": "jesse1819"},
                     "qobuz": {"user_auth_token": "qz-jesse-token"}}"#,
        None,
    )
}

/// A Last.fm whose one seed artist and two similar artists each have two
/// tracks, so a small run leaves candidates for the next run to deliver.
fn lastfm() -> Mock {
    Mock::start(|request| {
        if request.route() != LASTFM_ROUTE {
            return Response::status(404, r#"{"error":"no such route"}"#);
        }
        match request.query("method").as_deref() {
            Some("user.getTopArtists") => Response::json(
                r#"{"topartists":{"artist":[{"name":"Seed One"}],"@attr":{"totalPages":"1"}}}"#,
            ),
            Some("artist.getSimilar") => Response::json(
                r#"{"similarartists":{"artist":[{"name":"Sim A"},{"name":"Sim B"}],"@attr":{"totalPages":"1"}}}"#,
            ),
            Some("artist.getTopTracks") => {
                let artist = request.query("artist").unwrap_or_default();
                let tracks: Vec<serde_json::Value> = (1..=2)
                    .map(|n| {
                        json!({
                            "name": format!("{artist} Track {n}"),
                            "artist": {"name": artist}
                        })
                    })
                    .collect();
                Response::json(
                    json!({"toptracks": {"track": tracks, "@attr": {"totalPages": "1"}}})
                        .to_string(),
                )
            }
            Some("user.getrecenttracks") => {
                Response::json(r#"{"recenttracks":{"track":[],"@attr":{"totalPages":"1"}}}"#)
            }
            _ => Response::status(404, r#"{"error":6,"message":"no such method"}"#),
        }
    })
}

/// A Qobuz that accepts a playlist creation and one add, keeping just enough
/// state for the sync's resolution and retention passes.
fn qobuz() -> Mock {
    let playlists: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let next: Arc<Mutex<u64>> = Arc::new(Mutex::new(1));
    let handler_playlists = Arc::clone(&playlists);
    let handler_next = Arc::clone(&next);
    Mock::start(move |request| match request.route() {
        "/playlist/getUserPlaylists" => {
            let items: Vec<serde_json::Value> = handler_playlists
                .lock()
                .expect("lock")
                .iter()
                .map(|(id, name)| json!({"id": id, "name": name, "tracks_count": 0}))
                .collect();
            Response::json(json!({"playlists": {"items": items}}).to_string())
        }
        "/playlist/create" => {
            let mut next = handler_next.lock().expect("lock");
            let id = format!("p{next}");
            *next += 1;
            let name = request.query("name").unwrap_or_default();
            handler_playlists
                .lock()
                .expect("lock")
                .push((id.clone(), name));
            Response::json(json!({"id": id}).to_string())
        }
        "/playlist/get" => Response::json(r#"{"tracks":{"items":[],"total":0}}"#),
        "/track/search" => {
            let query = request.query("query").unwrap_or_default();
            Response::json(
                json!({"tracks": {"items": [{"id": format!("t{}", query.len())}]}}).to_string(),
            )
        }
        "/playlist/addTracks" => Response::json("{}"),
        "/playlist/delete" => Response::json(r#"{"success":true}"#),
        _ => Response::status(404, r#"{"message":"no such route"}"#),
    })
}

/// A Qobuz whose track resolution fails, so a delivery cannot complete.
fn qobuz_failing() -> Mock {
    Mock::start(|request| match request.route() {
        "/playlist/getUserPlaylists" => Response::json(r#"{"playlists":{"items":[]}}"#),
        "/playlist/create" => Response::json(r#"{"id":"p1"}"#),
        "/track/search" => Response::status(500, r#"{"message":"boom"}"#),
        _ => Response::status(404, r#"{"message":"no such route"}"#),
    })
}

/// The endpoints a discovery delivery needs.
fn endpoints(lastfm: &Mock, qobuz: &Mock) -> Vec<(&'static str, String)> {
    vec![
        (
            "OCTX_JAM_LASTFM_API",
            format!("{}{LASTFM_ROUTE}", lastfm.base),
        ),
        ("OCTX_JAM_QOBUZ_API", qobuz.base.clone()),
    ]
}

/// Run a discovery command against a scratch memory directory, returning the
/// output and the memory path it was pointed at.
fn run(
    lastfm: &Mock,
    qobuz: &Mock,
    state_dir: &str,
    args: &[&str],
) -> (support::Output, std::path::PathBuf) {
    let store = Store::with_document(Some(&account()));
    let output = jam_with_env(
        &store,
        &endpoints(lastfm, qobuz),
        &[("OCTX_JAM_STATE_DIR", state_dir)],
        args,
    );
    let memory = std::path::Path::new(state_dir).join("discovery-state.json");
    (output, memory)
}

/// The titles a JSON document holds, in order.
fn titles(output: &support::Output) -> Vec<String> {
    output.json()["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry["title"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_qobuz_delivery_records_but_a_json_run_does_not() {
    let lastfm = lastfm();
    let qobuz = qobuz();
    let dir = tempfile::tempdir().expect("state dir");
    let state = dir.path().to_str().expect("path");

    // A JSON run writes to stdout only; nothing may be remembered.
    let (output, memory) = run(&lastfm, &qobuz, state, &["discover", "--output", "json"]);
    output.ok();
    assert!(!memory.exists(), "a JSON run must not record");

    // A Qobuz delivery lands a playlist, so it is remembered.
    let (output, memory) = run(&lastfm, &qobuz, state, &["discover", "--output", "qobuz"]);
    output.ok();
    assert!(memory.exists(), "a Qobuz run must record");
}

#[test]
fn a_dry_run_records_nothing() {
    let lastfm = lastfm();
    let qobuz = qobuz();
    let dir = tempfile::tempdir().expect("state dir");
    let state = dir.path().to_str().expect("path");

    let (output, memory) = run(
        &lastfm,
        &qobuz,
        state,
        &["discover", "--output", "qobuz", "--dry-run"],
    );
    output.ok();
    assert!(!memory.exists(), "a dry run must not record");
}

#[test]
fn a_second_run_excludes_what_the_first_delivered() {
    let lastfm = lastfm();
    let qobuz = qobuz();
    let dir = tempfile::tempdir().expect("state dir");
    let state = dir.path().to_str().expect("path");

    // The first run delivers the first track from each artist.
    let (output, _) = run(
        &lastfm,
        &qobuz,
        state,
        &["discover", "--output", "qobuz", "--count", "2"],
    );
    output.ok();

    // The next run must draw on tracks the first did not deliver.
    let (output, _) = run(
        &lastfm,
        &qobuz,
        state,
        &["discover", "--output", "json", "--count", "2"],
    );
    output.ok();
    let titles = titles(&output);
    assert!(
        titles.contains(&"Sim B Track 1".to_string())
            && titles.contains(&"Seed One Track 2".to_string()),
        "the second run should rotate to undelivered tracks: {titles:?}"
    );
}

#[test]
fn a_failed_delivery_records_nothing() {
    let lastfm = lastfm();
    let qobuz = qobuz_failing();
    let dir = tempfile::tempdir().expect("state dir");
    let state = dir.path().to_str().expect("path");

    let (output, memory) = run(&lastfm, &qobuz, state, &["discover", "--output", "qobuz"]);
    output.failed();
    assert!(!memory.exists(), "a failed sync must not record");
}

#[test]
fn a_memory_write_failure_does_not_fail_a_delivered_run() {
    let lastfm = lastfm();
    let qobuz = qobuz();
    let dir = tempfile::tempdir().expect("state dir");
    // A file where a directory is expected: the memory cannot be written.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").expect("write blocker");
    let state = blocker.to_str().expect("path");

    let (output, _) = run(&lastfm, &qobuz, state, &["discover", "--output", "qobuz"]);
    output.ok(); // the playlist landed, so the run succeeds despite the memory
}

#[test]
fn a_missing_memory_directory_is_created() {
    let lastfm = lastfm();
    let qobuz = qobuz();
    let dir = tempfile::tempdir().expect("state dir");
    // A directory that does not exist yet.
    let nested = dir.path().join("does").join("not").join("exist");
    let state = nested.to_str().expect("path");

    let (output, memory) = run(&lastfm, &qobuz, state, &["discover", "--output", "qobuz"]);
    output.ok();
    assert!(memory.exists(), "the memory should have been written");
}
