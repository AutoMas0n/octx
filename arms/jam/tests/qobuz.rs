//! The Qobuz sink, driven end to end against a mock Qobuz.
//!
//! The mock keeps playlist state so a second run can be observed as a no-op,
//! and records every mutating call so the tests can assert both what changed
//! and in what order. The arm itself is the real binary throughout.

mod support;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use support::{Mock, Response, Store, document, jam, no_requests};

const RECS_ROUTE: &str = "/1/cf/recommendation/user/automason/recording";
const RADIO_ROUTE: &str = "/1/explore/lb-radio";
const PLAYER_ROUTE: &str = "/player/";

/// A radio response in the shape ListenBrainz returns.
fn radio_body() -> String {
    json!({
        "payload": {
            "feedback": "Using seed artist Queen",
            "jspf": { "playlist": { "track": [{
                "title": "Misfire",
                "creator": "Queen",
                "album": "Sheer Heart Attack",
                "duration": "114000",
                "identifier": ["https://musicbrainz.org/recording/1123e365-7b81-4a1f-b876-877c4bc0ca1e"]
            }] } }
        }
    })
    .to_string()
}

/// A recommendation response naming these recordings.
fn recs_body(mbids: &[&str]) -> String {
    json!({
        "payload": {
            "count": mbids.len(),
            "model_id": "0fe09b1a",
            "last_updated": 1_760_000_000,
            "total_mbid_count": 100,
            "mbids": mbids.iter().map(|mbid| json!({
                "recording_mbid": mbid,
                "score": 0.5,
                "latest_listened_at": 1_750_000_000
            })).collect::<Vec<_>>()
        }
    })
    .to_string()
}

/// A player response for whatever MBIDs the request carried.
fn player_body(mbids: &str) -> String {
    let tracks: Vec<Value> = mbids
        .split(',')
        .filter(|mbid| !mbid.is_empty())
        .map(|mbid| {
            json!({
                "title": format!("Track {mbid}"),
                "creator": "An Artist",
                "album": "An Album",
                "duration": "356000",
                "identifier": [format!("https://musicbrainz.org/recording/{mbid}")]
            })
        })
        .collect();
    json!({ "playlist": { "playlist": { "track": tracks } } }).to_string()
}

/// A ListenBrainz that recommends `mbids` and expands whatever it is asked to.
fn listenbrainz(recs: String) -> Mock {
    Mock::start(move |request| match request.route() {
        RECS_ROUTE => Response::json(recs.clone()),
        RADIO_ROUTE => Response::json(radio_body()),
        PLAYER_ROUTE => Response::json(player_body(
            &request.query("recording_mbids").unwrap_or_default(),
        )),
        _ => Response::status(404, r#"{"error":"no such route"}"#),
    })
}

/// One catalog entry the fake Qobuz can resolve.
struct Catalog {
    artist: String,
    title: String,
    id: String,
}

/// One entry the fake Qobuz holds in a playlist.
#[derive(Clone)]
struct Entry {
    track_id: String,
    entry_id: String,
}

/// The mutable state behind the fake Qobuz.
#[derive(Default)]
struct State {
    playlists: Vec<(String, String)>,
    /// Playlist id -> its entries, in playlist order.
    entries: HashMap<String, Vec<Entry>>,
    next_id: u64,
    next_entry_id: u64,
    catalog: Vec<Catalog>,
    adds: Vec<Vec<String>>,
    removes: Vec<Vec<String>>,
}

impl State {
    /// Put a playlist holding these track ids into the fake.
    fn seed_playlist(&mut self, id: &str, name: &str, track_ids: &[&str]) {
        self.playlists.push((id.to_string(), name.to_string()));
        let mut entries = Vec::new();
        for track_id in track_ids {
            entries.push(self.new_entry(track_id));
        }
        self.entries.insert(id.to_string(), entries);
    }

    /// A fresh entry for one track.
    fn new_entry(&mut self, track_id: &str) -> Entry {
        self.next_entry_id += 1;
        Entry {
            track_id: track_id.to_string(),
            entry_id: format!("e{}", self.next_entry_id),
        }
    }
}

/// A handle on the fake Qobuz's state, for assertions.
struct Fake {
    state: Arc<Mutex<State>>,
    rate_limits: Arc<AtomicUsize>,
    fail_title: Arc<Mutex<Option<String>>>,
}

impl Fake {
    /// The track ids in a playlist, by name.
    fn tracks(&self, name: &str) -> Vec<String> {
        let state = self.state.lock().expect("state lock");
        let id = state
            .playlists
            .iter()
            .find(|(_, playlist_name)| playlist_name == name)
            .map(|(id, _)| id.clone());
        id.and_then(|id| state.entries.get(&id).cloned())
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.track_id)
            .collect()
    }

    /// The track ids each `addTracks` call carried, in call order.
    fn adds(&self) -> Vec<Vec<String>> {
        self.state.lock().expect("state lock").adds.clone()
    }

    /// The track ids each `deleteTracks` call carried, in call order.
    fn removes(&self) -> Vec<Vec<String>> {
        self.state.lock().expect("state lock").removes.clone()
    }
}

/// A fake Qobuz that resolves `catalog` and keeps playlist state.
fn qobuz(catalog: Vec<(&str, &str, &str)>) -> (Mock, Fake) {
    let state = Arc::new(Mutex::new(State {
        catalog: catalog
            .into_iter()
            .map(|(artist, title, id)| Catalog {
                artist: artist.to_string(),
                title: title.to_string(),
                id: id.to_string(),
            })
            .collect(),
        next_id: 1,
        ..State::default()
    }));
    let rate_limits = Arc::new(AtomicUsize::new(0));
    let fail_title: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let handler_state = Arc::clone(&state);
    let handler_limits = Arc::clone(&rate_limits);
    let handler_fail = Arc::clone(&fail_title);

    let mock = Mock::start(move |request| {
        if handler_limits.load(Ordering::SeqCst) > 0 {
            handler_limits.fetch_sub(1, Ordering::SeqCst);
            return Response::status(429, "{}");
        }
        let mut state = handler_state.lock().expect("state lock");
        match request.route() {
            "/playlist/getUserPlaylists" => {
                let items: Vec<Value> = state
                    .playlists
                    .iter()
                    .map(|(id, name)| json!({ "id": id, "name": name, "tracks_count": 0 }))
                    .collect();
                Response::json(json!({ "playlists": { "items": items } }).to_string())
            }
            "/playlist/get" => {
                let id = request.query("playlist_id").unwrap_or_default();
                let entries = state.entries.get(&id).cloned().unwrap_or_default();
                // Qobuz names the track `id` and the entry `playlist_track_id`.
                let items: Vec<Value> = entries
                    .iter()
                    .map(|entry| {
                        json!({
                            "id": entry.track_id,
                            "playlist_track_id": entry.entry_id,
                        })
                    })
                    .collect();
                Response::json(
                    json!({ "tracks": { "items": items, "total": entries.len() } }).to_string(),
                )
            }
            "/playlist/create" => {
                let id = state.next_id.to_string();
                state.next_id += 1;
                let name = request.query("name").unwrap_or_default();
                state.playlists.push((id.clone(), name));
                state.entries.insert(id.clone(), Vec::new());
                Response::json(json!({ "id": id }).to_string())
            }
            "/playlist/addTracks" => {
                let id = request.query("playlist_id").unwrap_or_default();
                let ids: Vec<String> = split_ids(request.query("track_ids").unwrap_or_default());
                state.adds.push(ids.clone());
                let mut fresh = Vec::with_capacity(ids.len());
                for track_id in &ids {
                    fresh.push(state.new_entry(track_id));
                }
                state.entries.entry(id).or_default().extend(fresh);
                Response::json("{}")
            }
            "/playlist/deleteTracks" => {
                let id = request.query("playlist_id").unwrap_or_default();
                // Deletion names the playlist's own entries, never track ids.
                let ids: Vec<String> =
                    split_ids(request.query("playlist_track_ids").unwrap_or_default());
                state.removes.push(ids.clone());
                if let Some(entries) = state.entries.get_mut(&id) {
                    entries.retain(|entry| !ids.contains(&entry.entry_id));
                }
                Response::json("{}")
            }
            "/track/search" => {
                let query = request.query("query").unwrap_or_default();
                if let Some(title) = handler_fail.lock().expect("fail lock").as_ref()
                    && query.contains(title.as_str())
                {
                    return Response::status(500, r#"{"message":"boom"}"#);
                }
                let hit = state
                    .catalog
                    .iter()
                    .find(|entry| query.contains(&entry.title) && query.contains(&entry.artist));
                let items: Vec<Value> = hit
                    .map(|entry| vec![json!({ "id": entry.id })])
                    .unwrap_or_default();
                Response::json(json!({ "tracks": { "items": items } }).to_string())
            }
            _ => Response::status(404, r#"{"message":"no such route"}"#),
        }
    });

    (
        mock,
        Fake {
            state,
            rate_limits,
            fail_title,
        },
    )
}

fn split_ids(value: String) -> Vec<String> {
    value
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// A document whose account carries a Qobuz token.
fn qobuz_account() -> String {
    document(
        r#""jesse": {
            "listenbrainz": {"user": "automason", "token": "lb-jesse-token"},
            "qobuz": {"user_auth_token": "qz-jesse-token"}
        }"#,
        None,
    )
}

fn endpoints(lb: &Mock, qobuz: &Mock) -> Vec<(&'static str, String)> {
    vec![
        ("OCTX_JAM_LB_API", lb.base.clone()),
        ("OCTX_JAM_LB_PLAYER", format!("{}{PLAYER_ROUTE}", lb.base)),
        ("OCTX_JAM_QOBUZ_API", qobuz.base.clone()),
    ]
}

/// A run against a fresh store and the given mocks.
fn run(lb: &Mock, qobuz: &Mock, args: &[&str]) -> support::Output {
    let store = Store::with_document(Some(&qobuz_account()));
    jam(&store, &endpoints(lb, qobuz), args)
}

fn recs_args() -> [&'static str; 3] {
    ["recs", "--output", "qobuz"]
}

// -------------------------------------------------------------- resolution

#[test]
fn a_first_run_creates_and_populates_the_playlist_in_source_order() {
    let lb = listenbrainz(recs_body(&["rec-a", "rec-b"]));
    let (qobuz, fake) = qobuz(vec![
        ("An Artist", "Track rec-a", "101"),
        ("An Artist", "Track rec-b", "102"),
    ]);

    let output = run(&lb, &qobuz, &recs_args());
    output.ok();

    // The default name for `recs`, created because it did not exist.
    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["101", "102"]);
    // The summary reports what happened.
    output.stderr_has("resolved 2");
    output.stderr_has("unresolved 0");
    output.stderr_has("added 2");
    output.stderr_has("removed 0");
}

#[test]
fn a_second_identical_run_makes_no_mutations() {
    let lb = listenbrainz(recs_body(&["rec-a", "rec-b"]));
    let (qobuz, fake) = qobuz(vec![
        ("An Artist", "Track rec-a", "101"),
        ("An Artist", "Track rec-b", "102"),
    ]);
    let store = Store::with_document(Some(&qobuz_account()));
    let endpoints = endpoints(&lb, &qobuz);

    support::jam(&store, &endpoints, &recs_args()).ok();
    let adds_after_first = fake.adds().len();
    let removes_after_first = fake.removes().len();

    support::jam(&store, &endpoints, &recs_args()).ok();

    assert_eq!(fake.adds().len(), adds_after_first, "second run added");
    assert_eq!(
        fake.removes().len(),
        removes_after_first,
        "second run removed"
    );
    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["101", "102"]);
}

#[test]
fn a_track_no_longer_recommended_is_removed() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);
    // The playlist already holds a track the source no longer recommends.
    {
        let mut state = fake.state.lock().expect("state lock");
        state.seed_playlist("55", "ListenBrainz Discovery", &["999"]);
    }

    let output = run(&lb, &qobuz, &recs_args());
    output.ok();

    assert_eq!(fake.adds(), vec![vec!["101".to_string()]]);
    assert_eq!(fake.removes().len(), 1, "one removal call");
    assert_eq!(fake.removes()[0].len(), 1, "the stale entry is named once");
    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["101"]);
    output.stderr_has("removed 1");
}

#[test]
fn removals_name_playlist_entries_not_tracks() {
    // Qobuz refuses `track_ids` on a deletion, so a removal that carried track
    // ids was rejected and the playlist only ever grew. This is that guard.
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);
    {
        let mut state = fake.state.lock().expect("state lock");
        state.seed_playlist("55", "ListenBrainz Discovery", &["999"]);
    }

    run(&lb, &qobuz, &recs_args()).ok();

    let calls = qobuz.route("/playlist/deleteTracks");
    assert_eq!(calls.len(), 1, "one removal call");
    let call = &calls[0];
    assert!(
        call.query("track_ids").is_none(),
        "track_ids is the wrong parameter for a deletion: {call:?}"
    );
    let ids = call
        .query("playlist_track_ids")
        .expect("a deletion names playlist_track_ids");
    assert!(!ids.is_empty(), "the entry ids must not be empty");
}

#[test]
fn additions_are_applied_before_removals() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);
    {
        let mut state = fake.state.lock().expect("state lock");
        state.seed_playlist("55", "ListenBrainz Discovery", &["999"]);
    }

    run(&lb, &qobuz, &recs_args()).ok();

    let order: Vec<&str> = qobuz
        .requests()
        .into_iter()
        .filter(|request| {
            request.route() == "/playlist/addTracks" || request.route() == "/playlist/deleteTracks"
        })
        .map(|request| {
            if request.route() == "/playlist/addTracks" {
                "add"
            } else {
                "remove"
            }
        })
        .collect();
    assert_eq!(
        order,
        vec!["add", "remove"],
        "additions must precede removals"
    );
}

#[test]
fn a_dry_run_writes_nothing() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);

    let output = run(&lb, &qobuz, &["recs", "--output", "qobuz", "--dry-run"]);
    output.ok();

    assert!(fake.adds().is_empty(), "dry run must not add");
    assert!(fake.removes().is_empty(), "dry run must not remove");
    output.stderr_has("dry run");
}

// ------------------------------------------------------------ target names

#[test]
fn a_missing_named_playlist_is_created() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);

    let output = run(
        &lb,
        &qobuz,
        &["recs", "--output", "qobuz", "--playlist", "Jams"],
    );
    output.ok();

    let creates: Vec<String> = qobuz
        .route("/playlist/create")
        .into_iter()
        .map(|request| request.query("name").unwrap_or_default())
        .collect();
    assert_eq!(creates, vec!["Jams"]);
    assert_eq!(fake.tracks("Jams"), vec!["101"]);
}

#[test]
fn an_existing_named_playlist_is_used_without_creating() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);
    {
        let mut state = fake.state.lock().expect("state lock");
        state.seed_playlist("77", "Jams", &[]);
    }

    run(
        &lb,
        &qobuz,
        &["recs", "--output", "qobuz", "--playlist", "Jams"],
    )
    .ok();

    assert!(qobuz.route("/playlist/create").is_empty());
    let add_ids: Vec<String> = qobuz
        .route("/playlist/addTracks")
        .into_iter()
        .map(|request| request.query("playlist_id").unwrap_or_default())
        .collect();
    assert_eq!(add_ids, vec!["77"]);
}

#[test]
fn a_numeric_target_is_used_as_an_id_without_a_lookup() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, _fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);

    run(
        &lb,
        &qobuz,
        &["recs", "--output", "qobuz", "--playlist", "77"],
    )
    .ok();

    assert!(
        qobuz.route("/playlist/getUserPlaylists").is_empty(),
        "an id needs no name lookup"
    );
    assert!(qobuz.route("/playlist/create").is_empty());
    let add_ids: Vec<String> = qobuz
        .route("/playlist/addTracks")
        .into_iter()
        .map(|request| request.query("playlist_id").unwrap_or_default())
        .collect();
    assert_eq!(add_ids, vec!["77"]);
}

#[test]
fn the_defaults_differ_between_recs_and_radio() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);

    run(&lb, &qobuz, &recs_args()).ok();
    run(
        &lb,
        &qobuz,
        &[
            "radio",
            "--prompt",
            "artist:(queen)",
            "--mode",
            "easy",
            "--output",
            "qobuz",
        ],
    )
    .ok();

    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["101"]);
    assert_eq!(fake.tracks("ListenBrainz Radio"), Vec::<String>::new());
}

// -------------------------------------------------------- failure tolerance

#[test]
fn an_unresolved_entry_is_skipped_and_reported_without_aborting() {
    let lb = listenbrainz(recs_body(&["rec-a", "rec-b"]));
    // Only `rec-a` is in the catalog.
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);

    let output = run(&lb, &qobuz, &recs_args());
    output.ok();

    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["101"]);
    output.stderr_has("unresolved 1");
    output.stderr_has("Track rec-b");
}

#[test]
fn a_failed_search_does_not_abort_the_remaining_entries() {
    let lb = listenbrainz(recs_body(&["rec-a", "rec-b"]));
    let (qobuz, fake) = qobuz(vec![
        ("An Artist", "Track rec-a", "101"),
        ("An Artist", "Track rec-b", "102"),
    ]);
    *fake.fail_title.lock().expect("fail lock") = Some("Track rec-a".to_string());

    let output = run(&lb, &qobuz, &recs_args());
    output.ok();

    // The failing entry is skipped and reported; the other is still added.
    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["102"]);
    output.stderr_has("could not search Qobuz");
    output.stderr_has("unresolved 1");
}

#[test]
fn a_rate_limited_request_is_retried() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let (qobuz, fake) = qobuz(vec![("An Artist", "Track rec-a", "101")]);
    fake.rate_limits.store(1, Ordering::SeqCst);

    let output = run(&lb, &qobuz, &recs_args());
    output.ok();
    assert_eq!(fake.tracks("ListenBrainz Discovery"), vec!["101"]);
}

// ------------------------------------------------------------- empty source

#[test]
fn an_empty_source_never_touches_the_playlist() {
    let lb = Mock::start(|_request| Response::no_content());
    let qobuz = no_requests();
    let store = Store::with_document(Some(&qobuz_account()));

    let output = jam(&store, &endpoints(&lb, &qobuz), &recs_args());
    output.failed();
    assert!(
        output.stderr.contains("no recommendations"),
        "{}",
        output.stderr
    );
    assert_eq!(qobuz.count(), 0, "an empty source must not reach Qobuz");
}

#[test]
fn an_empty_source_writes_no_document() {
    let lb = Mock::start(|_request| Response::no_content());
    let qobuz = no_requests();
    let store = Store::with_document(Some(&qobuz_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert_eq!(output.stdout, "", "nothing may be written to stdout");
}

#[test]
fn the_qobuz_credential_is_required_and_named_before_any_request() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    // The account can read recommendations but carries no Qobuz token.
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason", "token": "lb-jesse-token"}}"#,
        None,
    )));

    let output = jam(&store, &endpoints(&lb, &qobuz), &recs_args());
    output.failed();
    assert!(
        output.stderr.contains("qobuz.user_auth_token"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("octx creds add jam"),
        "{}",
        output.stderr
    );
    assert_eq!(qobuz.count(), 0, "no Qobuz request without a credential");
}
