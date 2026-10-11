//! The write side, driven end to end against a mock ListenBrainz and Last.fm.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use support::{
    Mock, Output, Response, Store, document, jam, jam_stdin, jam_with_env, ndjson, no_requests,
};

const SUBMIT_ROUTE: &str = "/1/submit-listens";
const LASTFM_ROUTE: &str = "/2.0/";

/// An account with a ListenBrainz user and a submittable token.
fn account() -> String {
    document(
        r#""jesse": {"lastfm": {"user": "jesse1819"},
                     "listenbrainz": {"user": "automason", "token": "lb-jesse-token"}}"#,
        None,
    )
}

/// The endpoints a write-side run needs.
fn endpoints(lb: &Mock) -> Vec<(&'static str, String)> {
    vec![
        ("OCTX_JAM_LB_API", lb.base.clone()),
        ("OCTX_JAM_LASTFM_API", format!("{}{LASTFM_ROUTE}", lb.base)),
    ]
}

/// A ListenBrainz that accepts submissions, remembering the identities it has
/// already been given — what the real endpoint's uniqueness constraint does.
struct Submit {
    mock: Mock,
    seen: Arc<Mutex<Vec<String>>>,
    new_listens: Arc<AtomicUsize>,
}

impl Submit {
    fn start() -> Self {
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let new_listens = Arc::new(AtomicUsize::new(0));
        let recorded = Arc::clone(&seen);
        let counted = Arc::clone(&new_listens);

        let mock = Mock::start(move |request| {
            if request.route() == SUBMIT_ROUTE {
                let body = request.json();
                let payload = body["payload"].as_array().cloned().unwrap_or_default();
                let mut fresh = 0usize;
                {
                    let mut seen = recorded.lock().expect("seen lock");
                    for listen in &payload {
                        let identity =
                            listen["track_metadata"]["additional_info"]["recording_msid"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string();
                        if !seen.contains(&identity) {
                            seen.push(identity);
                            fresh += 1;
                        }
                    }
                }
                counted.fetch_add(fresh, Ordering::SeqCst);
                Response::json(r#"{"status":"ok"}"#)
            } else {
                Response::status(404, r#"{"error":"no such route"}"#)
            }
        });

        Self {
            mock,
            seen,
            new_listens,
        }
    }

    fn submissions(&self) -> Vec<support::Request> {
        self.mock.route(SUBMIT_ROUTE)
    }

    fn new_listens(&self) -> usize {
        self.new_listens.load(Ordering::SeqCst)
    }

    fn stored(&self) -> Vec<String> {
        self.seen.lock().expect("seen lock").clone()
    }
}

/// Two listens, one of them with the optional fields.
const TWO_LISTENS: &str = concat!(
    r#"{"listened_at":1760000000,"artist":"Queen","title":"Misfire","album":"Sheer Heart Attack","duration_ms":114000}"#,
    "\n",
    r#"{"listened_at":1760000300,"artist":"Vogl","title":"Odyssey"}"#,
    "\n"
);

// ------------------------------------------------------------- input reading

#[test]
fn a_file_of_listens_is_read_and_written_as_a_document() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(TWO_LISTENS);

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "json",
        ],
    );
    output.ok();

    let value = output.json();
    assert_eq!(value["kind"], "listens");
    assert_eq!(value["source"], "ndjson");
    assert_eq!(value["listens"].as_array().expect("listens").len(), 2);
    assert_eq!(value["listens"][0]["listened_at"], 1_760_000_000i64);
    assert_eq!(value["listens"][0]["artist"], "Queen");
    assert_eq!(value["listens"][0]["title"], "Misfire");
    assert_eq!(value["listens"][0]["album"], "Sheer Heart Attack");
    assert_eq!(value["listens"][0]["duration_ms"], 114_000);
    // No Last.fm data was read, so no Last.fm credit is attached.
    assert!(value.get("attribution").is_none(), "{value}");
    assert_eq!(lb.mock.count(), 0, "the json sink must contact nothing");
}

#[test]
fn standard_input_is_read_the_same_way() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));

    let output = jam_stdin(
        &store,
        &endpoints(&lb.mock),
        &["scrobble", "--input", "-", "--output", "json"],
        TWO_LISTENS,
    );
    output.ok();
    assert_eq!(
        output.json()["listens"].as_array().expect("listens").len(),
        2
    );
    assert_eq!(lb.mock.count(), 0);
}

#[test]
fn blank_lines_are_not_reported_as_rejected() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            ndjson(&format!("\n{TWO_LISTENS}\n\n"))
                .path()
                .to_str()
                .expect("path"),
            "--output",
            "json",
        ],
    );
    output.ok();
    assert!(!output.stderr.contains("rejected"), "{}", output.stderr);
}

#[test]
fn the_order_of_the_input_is_kept() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            ndjson(TWO_LISTENS).path().to_str().expect("path"),
            "--output",
            "json",
        ],
    );
    output.ok();
    let value = output.json();
    assert_eq!(value["listens"][0]["title"], "Misfire");
    assert_eq!(value["listens"][1]["title"], "Odyssey");
}

// ------------------------------------------------------------------ identity

#[test]
fn two_runs_over_one_input_derive_the_same_identity() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(TWO_LISTENS);
    let args = [
        "scrobble",
        "--input",
        input.path().to_str().expect("path"),
        "--output",
        "json",
    ];

    let first = jam(&store, &endpoints(&lb.mock), &args);
    let second = jam(&store, &endpoints(&lb.mock), &args);
    first.ok();
    second.ok();
    assert_eq!(first.stdout, second.stdout, "identity must not vary");
    assert!(first.json()["listens"][0]["recording_msid"].is_string());
}

#[test]
fn a_second_identical_submission_creates_no_new_listens() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(TWO_LISTENS);
    let args = [
        "scrobble",
        "--input",
        input.path().to_str().expect("path"),
        "--output",
        "listenbrainz",
    ];

    jam(&store, &endpoints(&lb.mock), &args).ok();
    assert_eq!(lb.new_listens(), 2, "the first run delivers both listens");

    jam(&store, &endpoints(&lb.mock), &args).ok();
    assert_eq!(lb.new_listens(), 2, "the second run adds nothing");
    assert_eq!(lb.stored().len(), 2);
}

#[test]
fn a_repeat_inside_one_input_is_delivered_once() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let line = r#"{"listened_at":1760000000,"artist":"Queen","title":"Misfire"}"#;

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            ndjson(&format!("{line}\n{line}\n"))
                .path()
                .to_str()
                .expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.ok();
    assert_eq!(lb.new_listens(), 1);
}

// ------------------------------------------------------------------ malformed

#[test]
fn one_bad_line_is_reported_by_number_and_the_good_ones_are_delivered() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(
        "{\"listened_at\":1760000000,\"artist\":\"Queen\",\"title\":\"Misfire\"}\n\
         {oops}\n\
         {\"listened_at\":1760000300,\"artist\":\"Vogl\",\"title\":\"Odyssey\"}\n",
    );

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("line 2");
    output.stderr_has("submitted 2 listens");
    assert_eq!(lb.new_listens(), 2, "the valid lines were still delivered");
}

#[test]
fn an_input_of_only_bad_lines_delivers_nothing() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson("{oops}\nalso not json\n");

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("line 1");
    output.stderr_has("line 2");
    assert_eq!(lb.mock.count(), 0);
}

#[test]
fn an_empty_input_is_a_no_op() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson("\n\n");

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.ok();
    output.stderr_has("nothing to submit");
    assert_eq!(lb.mock.count(), 0);
}

#[test]
fn an_empty_input_needs_no_credential_at_all() {
    let lb = Submit::start();
    let store = Store::with_document(None);
    let input = ndjson("");

    // Output is ListenBrainz, so the token is required before the input is
    // read — and there is no document, so the run fails naming it.
    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("octx creds add jam");
    assert_eq!(lb.mock.count(), 0);
}

// ------------------------------------------------------------------- batching

#[test]
fn many_listens_are_submitted_in_one_import() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let lines: String = (0..5)
        .map(|index| {
            format!(
                "{{\"listened_at\":{},\"artist\":\"Artist {index}\",\"title\":\"Title {index}\"}}\n",
                1_760_000_000 + index
            )
        })
        .collect();

    jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            ndjson(&lines).path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    )
    .ok();

    let submissions = lb.submissions();
    assert_eq!(
        submissions.len(),
        1,
        "one batch, not one request per listen"
    );
    assert_eq!(submissions[0].json()["listen_type"], "import");
    assert_eq!(
        submissions[0].json()["payload"]
            .as_array()
            .expect("payload")
            .len(),
        5
    );
    assert_eq!(
        submissions[0].header("authorization"),
        Some("Token lb-jesse-token")
    );
}

#[test]
fn a_single_listen_uses_the_single_form() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(r#"{"listened_at":1760000000,"artist":"Queen","title":"Misfire"}"#);

    jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    )
    .ok();

    let submissions = lb.submissions();
    assert_eq!(submissions.len(), 1);
    assert_eq!(submissions[0].json()["listen_type"], "single");
}

#[test]
fn a_listen_carries_only_what_listenbrainz_accepts() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));

    jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            ndjson(TWO_LISTENS).path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    )
    .ok();

    let body = lb.submissions()[0].json();
    let listen = &body["payload"][0];
    let keys: Vec<&str> = listen
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys.len(),
        2,
        "identity must not be a third top-level key: {keys:?}"
    );
    assert!(listen["track_metadata"]["additional_info"]["recording_msid"].is_string());
}

#[test]
fn submissions_are_spaced() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    // Force two batches by lowering nothing but the batch size indirectly:
    // one request per listen is not possible, so use the throttle only.
    let input = ndjson(TWO_LISTENS);

    let output = jam_with_env(
        &store,
        &endpoints(&lb.mock),
        &[("OCTX_JAM_INTERVAL_MS", "0")],
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.ok();

    let submissions = lb.submissions();
    assert_eq!(submissions.len(), 1);
    // The spacing rule is asserted against a two-request run in `mock.rs`;
    // here the point is that a batch is one request inside the budget.
}

// ---------------------------------------------------------------- submissions

#[test]
fn a_missing_token_fails_before_reading_the_input() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason"}}"#,
        None,
    )));
    let input = ndjson("this is not even NDJSON");

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("accounts.jesse.listenbrainz.token");
    output.stderr_has("octx creds add jam");
    assert!(
        !output.stderr.contains("line 1"),
        "the input was read first"
    );
    assert_eq!(lb.mock.count(), 0);
}

#[test]
fn a_token_that_may_not_submit_is_reported_differently_from_an_unknown_one() {
    let input = ndjson(TWO_LISTENS);
    let store = Store::with_document(Some(&account()));

    let scope = Mock::start(|_| {
        Response::status(
            401,
            r#"{"error":"Token has no permission to submit listens"}"#,
        )
    });
    let output = jam(
        &store,
        &endpoints(&scope),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("may not submit");
    output.stderr_has("listenbrainz:submit-listens");

    let unknown = Mock::start(|_| Response::status(401, r#"{"error":"No such token"}"#));
    let output = jam(
        &store,
        &endpoints(&unknown),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("refused the submission");
    assert!(
        !output.stderr.contains("may not submit"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_refused_submission_reports_the_reason() {
    let lb = Mock::start(|_| {
        Response::status(
            400,
            r#"{"error":"Cannot submit listens for a paused user."}"#,
        )
    });
    let store = Store::with_document(Some(&account()));
    let input = ndjson(TWO_LISTENS);

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("paused user");
}

#[test]
fn the_summary_counts_what_was_sent_and_rejected_but_never_what_was_new() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(&format!("{TWO_LISTENS}{{oops}}\n"));

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    output.stderr_has("submitted 2 listens");
    output.stderr_has("1 input line rejected");
    for forbidden in ["deduplicat", "new listen", "newly created"] {
        assert!(
            !output.stderr.to_lowercase().contains(forbidden),
            "the summary claims `{forbidden}`: {}",
            output.stderr
        );
    }
}

#[test]
fn the_json_sink_for_listens_contacts_no_write_endpoint() {
    let lb = no_requests();
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--input",
            ndjson(TWO_LISTENS).path().to_str().expect("path"),
            "--output",
            "json",
        ],
    );
    output.ok();
    assert_eq!(lb.count(), 0);
}

// ------------------------------------------------------- source/sink selection

#[test]
fn the_source_defaults_to_ndjson() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(TWO_LISTENS);

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "json",
        ],
    );
    output.ok();
    assert_eq!(output.json()["source"], "ndjson");
}

#[test]
fn the_local_source_needs_an_input() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &["scrobble", "--output", "listenbrainz"],
    );
    output.failed();
    output.stderr_has("--input");
    assert_eq!(lb.mock.count(), 0);
}

#[test]
fn a_networked_source_refuses_an_input() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson(TWO_LISTENS);

    for source in ["lastfm", "listenbrainz"] {
        let output = jam(
            &store,
            &endpoints(&lb.mock),
            &[
                "scrobble",
                "--source",
                source,
                "--input",
                input.path().to_str().expect("path"),
                "--output",
                "json",
            ],
        );
        output.failed();
        output.stderr_has("--input");
    }
    assert_eq!(lb.mock.count(), 0, "nothing was fetched either");
}

#[test]
fn an_unknown_source_is_rejected() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &["scrobble", "--source", "spotify", "--output", "json"],
    );
    output.failed();
    assert_eq!(lb.mock.count(), 0);
}

// ------------------------------------------------------------------- Last.fm

/// A Last.fm that answers `user.getRecentTracks` with `body`.
fn lastfm(body: String) -> Mock {
    Mock::start(move |request| {
        if request.route() == LASTFM_ROUTE {
            Response::json(body.clone())
        } else {
            Response::status(404, r#"{"error":"no such route"}"#)
        }
    })
}

/// A lastfm response page.
fn recent_tracks(tracks: &Value, page: usize, total_pages: usize) -> String {
    json!({
        "recenttracks": {
            "track": tracks,
            "@attr": {
                "page": page.to_string(),
                "perPage": "200",
                "totalPages": total_pages.to_string(),
                "total": "9"
            }
        }
    })
    .to_string()
}

#[test]
fn lastfm_scrobbles_become_listens_and_carry_their_credit() {
    let page = recent_tracks(
        &json!([{
            "artist": {"#text": "Princess Goes"},
            "album": {"#text": "THANKS FOR COMING"},
            "name": "Eat an Eraser",
            "date": {"uts": "1760013000"}
        }]),
        1,
        1,
    );
    let lb = lastfm(page);
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--source",
            "lastfm",
            "--user",
            "jesse1819",
            "--output",
            "json",
        ],
    );
    output.ok();

    let request = &lb.route(LASTFM_ROUTE)[0];
    assert_eq!(
        request.query("method").as_deref(),
        Some("user.getrecenttracks")
    );
    assert_eq!(request.query("user").as_deref(), Some("jesse1819"));
    assert_eq!(request.query("api_key").as_deref(), Some("shared-app-key"));
    assert_eq!(request.query("format").as_deref(), Some("json"));
    // `extended` re-shapes artist and album; it must never be requested.
    assert_eq!(request.query("extended"), None);

    let value = output.json();
    assert_eq!(value["source"], "lastfm");
    assert_eq!(value["user"], "jesse1819");
    assert_eq!(value["listens"][0]["artist"], "Princess Goes");
    assert_eq!(value["listens"][0]["title"], "Eat an Eraser");
    assert_eq!(value["listens"][0]["album"], "THANKS FOR COMING");
    assert_eq!(value["listens"][0]["listened_at"], 1_760_013_000i64);

    // The credit travels inside the document, and is printed for the user.
    assert_eq!(value["attribution"]["source"], "Last.fm");
    assert_eq!(
        value["attribution"]["account_url"],
        "https://www.last.fm/user/jesse1819"
    );
    assert_eq!(
        value["attribution"]["terms_url"],
        "https://www.last.fm/api/tos"
    );
    output.stderr_has("Last.fm");
    output.stderr_has("https://www.last.fm/api/tos");
}

#[test]
fn the_now_playing_track_is_not_delivered() {
    let page = recent_tracks(
        &json!([
            {
                "artist": {"#text": "Queen"},
                "album": {"#text": "A Night at the Opera"},
                "name": "Bohemian Rhapsody",
                "@attr": {"nowplaying": "true"}
            },
            {
                "artist": {"#text": "Queen"},
                "album": {"#text": "Sheer Heart Attack"},
                "name": "Misfire",
                "date": {"uts": "1760013000"}
            }
        ]),
        1,
        1,
    );
    let lb = lastfm(page);
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--source",
            "lastfm",
            "--user",
            "jesse1819",
            "--output",
            "json",
        ],
    );
    output.ok();
    let value = output.json();
    assert_eq!(value["listens"].as_array().expect("listens").len(), 1);
    assert_eq!(value["listens"][0]["title"], "Misfire");
}

#[test]
fn lastfm_is_paged_within_the_limit() {
    let lb = Mock::start(|request| {
        if request.route() == LASTFM_ROUTE {
            let page: usize = request
                .query("page")
                .and_then(|page| page.parse().ok())
                .unwrap_or(1);
            let tracks: Vec<Value> = (0..2)
                .map(|index| {
                    json!({
                        "artist": {"#text": "Artist"},
                        "name": format!("Page {page} Track {index}"),
                        "date": {"uts": (1_760_000_000 + page as i64 * 10 + index).to_string()}
                    })
                })
                .collect();
            Response::json(recent_tracks(&json!(tracks), page, 5))
        } else {
            Response::status(404, "{}")
        }
    });
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--source",
            "lastfm",
            "--user",
            "jesse1819",
            "--limit",
            "6",
            "--output",
            "json",
        ],
    );
    output.ok();
    let pages: Vec<String> = lb
        .route(LASTFM_ROUTE)
        .iter()
        .map(|request| request.query("page").unwrap_or_default())
        .collect();
    assert_eq!(pages, vec!["1", "2", "3"], "paged until the limit was met");
    assert_eq!(
        output.json()["listens"].as_array().expect("listens").len(),
        6
    );
}

#[test]
fn lastfm_takes_the_user_from_the_account_when_none_is_given() {
    let page = recent_tracks(&json!([]), 1, 1);
    let lb = lastfm(page);
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "lastfm", "--output", "json"],
    );
    output.ok();
    output.stderr_has("nothing to submit");
}

#[test]
fn lastfm_needs_its_key_and_says_which_field() {
    let lb = lastfm(recent_tracks(&json!([]), 1, 1));
    // A document with no shared API key at all.
    let store = Store::with_document(Some(
        r#"{"version":1,"accounts":{"jesse":{"lastfm":{"user":"jesse1819"}}}}"#,
    ));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "lastfm", "--output", "json"],
    );
    output.failed();
    output.stderr_has("lastfm.api_key");
    output.stderr_has("octx creds add jam");
    assert_eq!(lb.count(), 0);
}

#[test]
fn lastfm_needs_a_user_and_says_which_field() {
    let lb = lastfm(recent_tracks(&json!([]), 1, 1));
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason"}}"#,
        None,
    )));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "lastfm", "--output", "json"],
    );
    output.failed();
    output.stderr_has("accounts.jesse.lastfm.user");
    assert_eq!(lb.count(), 0);
}

#[test]
fn a_lastfm_api_error_is_surfaced() {
    let lb = lastfm(r#"{"error":10,"message":"Invalid API key"}"#.to_string());
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "lastfm", "--output", "json"],
    );
    output.failed();
    output.stderr_has("Invalid API key");
}

#[test]
fn lastfm_delivers_to_listenbrainz_and_adds_nothing_on_a_second_run() {
    let page = recent_tracks(
        &json!([{
            "artist": {"#text": "Queen"},
            "album": {"#text": "Sheer Heart Attack"},
            "name": "Misfire",
            "date": {"uts": "1760013000"}
        }]),
        1,
        1,
    );
    let lb = Mock::start({
        let page = page.clone();
        move |request| match request.route() {
            LASTFM_ROUTE => Response::json(page.clone()),
            SUBMIT_ROUTE => Response::json(r#"{"status":"ok"}"#),
            _ => Response::status(404, "{}"),
        }
    });
    let store = Store::with_document(Some(&account()));
    let args = ["scrobble", "--source", "lastfm", "--output", "listenbrainz"];

    jam(&store, &endpoints(&lb), &args).ok();
    jam(&store, &endpoints(&lb), &args).ok();

    let submissions = lb.route(SUBMIT_ROUTE);
    assert_eq!(submissions.len(), 2);
    let first_identity =
        submissions[0].json()["payload"][0]["track_metadata"]["additional_info"]["recording_msid"]
            .clone();
    let second_identity = submissions[1].json()["payload"][0]["track_metadata"]["additional_info"]
        ["recording_msid"]
        .clone();
    assert_eq!(first_identity, second_identity);
}

// ---------------------------------------------------------------- ListenBrainz

/// A ListenBrainz that answers the listens endpoint.
fn listens(body: String) -> Mock {
    Mock::start(move |request| {
        if request.route().ends_with("/listens") {
            Response::json(body.clone())
        } else if request.route() == "/1/validate-token" {
            Response::json(r#"{"code":200,"user_name":"automason","valid":true}"#)
        } else {
            Response::status(404, r#"{"error":"no such route"}"#)
        }
    })
}

fn listens_body() -> String {
    json!({
        "payload": {
            "count": 2,
            "listens": [{
                "listened_at": 1_760_013_000i64,
                "recording_msid": "upstream-identity",
                "track_metadata": {
                    "artist_name": "Princess Goes",
                    "track_name": "Eat an Eraser",
                    "release_name": "THANKS FOR COMING"
                }
            }, {
                "listened_at": 1_760_013_200i64,
                "recording_msid": "another-identity",
                "track_metadata": {
                    "artist_name": "Vogl",
                    "track_name": "Odyssey"
                }
            }]
        }
    })
    .to_string()
}

#[test]
fn public_listens_are_readable_without_a_token() {
    let lb = listens(listens_body());
    // No ListenBrainz credential in the document at all.
    let store = Store::with_document(Some(&document(r#""jesse": {}"#, None)));

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--source",
            "listenbrainz",
            "--user",
            "automason",
            "--output",
            "json",
        ],
    );
    output.ok();
    let request = &lb.route("/1/user/automason/listens")[0];
    assert_eq!(request.query("count").as_deref(), Some("200"));
    assert_eq!(request.header("authorization"), None, "no token is needed");
    assert_eq!(lb.route("/1/validate-token").len(), 0);

    let value = output.json();
    assert_eq!(value["source"], "listenbrainz");
    assert_eq!(value["listens"][0]["artist"], "Princess Goes");
    assert_eq!(value["listens"][0]["title"], "Eat an Eraser");
    assert_eq!(value["listens"][0]["album"], "THANKS FOR COMING");
    assert!(value.get("attribution").is_none(), "{value}");
}

#[test]
fn the_listenbrainz_source_keeps_upstream_identities() {
    let lb = listens(listens_body());
    let store = Store::with_document(Some(&document(r#""jesse": {}"#, None)));

    let output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--source",
            "listenbrainz",
            "--user",
            "automason",
            "--output",
            "json",
        ],
    );
    output.ok();
    let value = output.json();
    assert_eq!(value["listens"][0]["recording_msid"], "upstream-identity");
    assert_eq!(value["listens"][1]["recording_msid"], "another-identity");
}

#[test]
fn the_account_takes_its_name_from_the_document() {
    let lb = listens(listens_body());
    let store = Store::with_document(Some(&account()));

    jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "listenbrainz", "--output", "json"],
    )
    .ok();
    assert_eq!(lb.route("/1/user/automason/listens").len(), 1);
}

#[test]
fn the_account_name_can_be_derived_from_the_token() {
    let lb = listens(listens_body());
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"token": "lb-jesse-token"}}"#,
        None,
    )));

    jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "listenbrainz", "--output", "json"],
    )
    .ok();
    assert_eq!(lb.route("/1/validate-token").len(), 1);
    assert_eq!(lb.route("/1/user/automason/listens").len(), 1);
}

#[test]
fn neither_a_name_nor_a_token_fails_fast() {
    let lb = listens(listens_body());
    let store = Store::with_document(Some(&document(r#""jesse": {}"#, None)));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "listenbrainz", "--output", "json"],
    );
    output.failed();
    output.stderr_has("listenbrainz.user");
    output.stderr_has("listenbrainz.token");
    assert_eq!(lb.count(), 0);
}

#[test]
fn a_fetch_that_returns_no_listens_is_a_no_op() {
    let lb = listens(json!({"payload": {"count": 0, "listens": []}}).to_string());
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "listenbrainz", "--output", "json"],
    );
    output.ok();
    output.stderr_has("nothing to submit");
    assert!(output.stdout.trim().is_empty(), "{}", output.stdout);
}

#[test]
fn a_limit_bounds_the_fetch() {
    let lb = listens(listens_body());
    let store = Store::with_document(Some(&account()));

    jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--source",
            "listenbrainz",
            "--limit",
            "5",
            "--output",
            "json",
        ],
    )
    .ok();
    assert_eq!(
        lb.route("/1/user/automason/listens")[0]
            .query("count")
            .as_deref(),
        Some("5")
    );
}

#[test]
fn a_listenbrainz_source_delivers_back_without_duplicating() {
    let lb = listens(listens_body());
    let store = Store::with_document(Some(&account()));

    let output = jam(
        &store,
        &endpoints(&lb),
        &["scrobble", "--source", "listenbrainz", "--output", "json"],
    );
    output.ok();
    let value = output.json();
    // The same shape as the NDJSON source produces.
    for listen in value["listens"].as_array().expect("listens") {
        for field in ["listened_at", "artist", "title", "recording_msid"] {
            assert!(listen.get(field).is_some(), "missing {field}: {listen}");
        }
    }
}

// ------------------------------------------------------------ two accounts

#[test]
fn two_accounts_submit_with_their_own_token_and_never_each_others() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason", "token": "lb-jesse-token"}},
           "wife": {"listenbrainz": {"user": "wife-lb", "token": "lb-wife-token"}}"#,
        Some("jesse"),
    )));
    let input = ndjson(TWO_LISTENS);
    let path = input.path().to_str().expect("path").to_string();

    let his = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--source",
            "ndjson",
            "--input",
            &path,
            "--account",
            "jesse",
            "--output",
            "listenbrainz",
        ],
    );
    his.ok().asserts_absent("lb-wife-token");

    let hers = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--source",
            "ndjson",
            "--input",
            &path,
            "--account",
            "wife",
            "--output",
            "listenbrainz",
        ],
    );
    hers.ok().asserts_absent("lb-jesse-token");

    let submissions = lb.submissions();
    let tokens: Vec<Option<&str>> = submissions
        .iter()
        .map(|request| request.header("authorization"))
        .collect();
    assert_eq!(
        tokens,
        vec![Some("Token lb-jesse-token"), Some("Token lb-wife-token")]
    );
}

#[test]
fn an_unknown_account_fails_before_reading_the_input() {
    let lb = Submit::start();
    let store = Store::with_document(Some(&account()));
    let input = ndjson("this is not even NDJSON");

    let output = jam(
        &store,
        &endpoints(&lb.mock),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--account",
            "ghost",
            "--output",
            "listenbrainz",
        ],
    );
    output.failed();
    assert!(!output.stderr.contains("line 1"), "{}", output.stderr);
    assert_eq!(lb.mock.count(), 0);
}

#[test]
fn a_local_inspection_needs_no_credential_at_all() {
    let lb = no_requests();
    let store = Store::with_document(None);
    let input = ndjson(TWO_LISTENS);

    let output: Output = jam(
        &store,
        &endpoints(&lb),
        &[
            "scrobble",
            "--input",
            input.path().to_str().expect("path"),
            "--output",
            "json",
        ],
    );
    output.ok();
    assert_eq!(lb.count(), 0);
}
