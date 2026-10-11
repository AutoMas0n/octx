//! The read side, driven end to end against a mock ListenBrainz.
//!
//! Every test here runs the real binary, so what is asserted is the arm's
//! behaviour: which request it made, what it refused to print, and what it
//! wrote to stdout.

mod support;

use serde_json::json;
use support::{Mock, Response, Store, document, jam, jam_with_env, no_requests};

const RECS_ROUTE: &str = "/1/cf/recommendation/user/automason/recording";
const PLAYER_ROUTE: &str = "/player/";
const RADIO_ROUTE: &str = "/1/explore/lb-radio";
const TOKEN_ROUTE: &str = "/1/validate-token";

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

/// A player response for the MBIDs the request asked for.
///
/// Generated from the query so the chunking tests can assert what came back as
/// well as what went out.
fn player_body(mbids: &str) -> String {
    let tracks: Vec<serde_json::Value> = mbids
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

/// A ListenBrainz that recommends `mbids` and expands whatever it is asked to.
fn listenbrainz(recs: String) -> Mock {
    Mock::start(move |request| match request.route() {
        RECS_ROUTE => Response::json(recs.clone()),
        PLAYER_ROUTE => Response::json(player_body(
            &request.query("recording_mbids").unwrap_or_default(),
        )),
        RADIO_ROUTE => Response::json(radio_body()),
        TOKEN_ROUTE => Response::json(r#"{"code":200,"user_name":"automason","valid":true}"#),
        _ => Response::status(404, r#"{"error":"no such route"}"#),
    })
}

/// The endpoints a read-side run needs.
fn endpoints(lb: &Mock, qobuz: &Mock) -> Vec<(&'static str, String)> {
    vec![
        ("OCTX_JAM_LB_API", lb.base.clone()),
        ("OCTX_JAM_LB_PLAYER", format!("{}{PLAYER_ROUTE}", lb.base)),
        ("OCTX_JAM_QOBUZ_API", qobuz.base.clone()),
    ]
}

/// A document with one account that can read recommendations.
fn one_account() -> String {
    document(
        r#""jesse": {"listenbrainz": {"user": "automason", "token": "lb-jesse-token"}}"#,
        None,
    )
}

// ---------------------------------------------------------------- credentials

#[test]
fn the_user_comes_from_the_account_and_the_result_is_expanded() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.ok();

    let requests = lb.requests();
    assert_eq!(requests.len(), 2, "{requests:#?}");
    assert_eq!(requests[0].route(), RECS_ROUTE);
    assert_eq!(requests[0].query("count").as_deref(), Some("25"));
    assert_eq!(requests[1].route(), PLAYER_ROUTE);
    assert_eq!(
        requests[1].query("recording_mbids").as_deref(),
        Some("rec-a")
    );

    let value = output.json();
    assert_eq!(value["source"]["user"], "automason");
    assert_eq!(value["source"]["model_id"], "0fe09b1a");
    assert_eq!(value["source"]["last_updated"], 1_760_000_000i64);
    assert_eq!(value["entries"][0]["recording_mbid"], "rec-a");
    assert_eq!(value["entries"][0]["title"], "Track rec-a");
    assert_eq!(value["entries"][0]["artist_credit"], "An Artist");
    assert_eq!(value["entries"][0]["album"], "An Album");
    assert_eq!(value["entries"][0]["duration_ms"], 356_000);
    assert_eq!(value["entries"][0]["score"], 0.5);
}

#[test]
fn a_public_read_with_an_explicit_user_never_reads_the_store() {
    // The store is empty: a read that needed a credential would fail here.
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(None);

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--user", "automason", "--output", "json"],
    );
    output.ok();
    assert_eq!(output.json()["source"]["user"], "automason");
    assert_eq!(lb.count(), 2);
}

#[test]
fn several_accounts_and_no_default_fail_before_any_request() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason"}},
           "wife": {"listenbrainz": {"user": "wife-lb"}}"#,
        None,
    )));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(output.stderr.contains("jesse"), "{}", output.stderr);
    assert!(output.stderr.contains("wife"), "{}", output.stderr);
    assert!(output.stderr.contains("--account"), "{}", output.stderr);
    assert_eq!(lb.count(), 0, "a failing run must not reach the service");
}

#[test]
fn an_unknown_account_fails_before_any_request() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--account", "ghost", "--output", "json"],
    );
    output.failed();
    assert!(output.stderr.contains("ghost"), "{}", output.stderr);
    assert!(output.stderr.contains("jesse"), "{}", output.stderr);
    assert_eq!(lb.count(), 0);
}

#[test]
fn two_accounts_read_their_own_user() {
    let lb = Mock::start(|request| match request.route() {
        route if route.starts_with("/1/cf/recommendation/user/") => {
            Response::json(recs_body(&["rec-a"]))
        }
        PLAYER_ROUTE => Response::json(player_body(
            &request.query("recording_mbids").unwrap_or_default(),
        )),
        _ => Response::status(404, "{}"),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason"}},
           "wife": {"listenbrainz": {"user": "wife-lb"}}"#,
        Some("jesse"),
    )));

    jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--account", "jesse", "--output", "json"],
    )
    .ok();
    jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--account", "wife", "--output", "json"],
    )
    .ok();

    let paths: Vec<String> = lb
        .requests()
        .into_iter()
        .filter(|request| request.route().starts_with("/1/cf/"))
        .map(|request| request.path)
        .collect();
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert!(paths[0].contains("/user/automason/"), "{:?}", paths[0]);
    assert!(paths[1].contains("/user/wife-lb/"), "{:?}", paths[1]);
}

#[test]
fn an_absent_document_names_the_host_and_the_command_and_asks_nothing() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(None);

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(output.stderr.contains("jam"), "{}", output.stderr);
    assert!(
        output.stderr.contains("octx creds add jam"),
        "{}",
        output.stderr
    );
    assert_eq!(lb.count(), 0);
}

#[test]
fn a_malformed_document_is_reported_without_being_echoed() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some("this is not JSON, and it is secret"));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(
        output.stderr.contains("not valid JSON"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("octx creds add jam"),
        "{}",
        output.stderr
    );
    output.asserts_absent("it is secret");
    assert_eq!(lb.count(), 0);
}

#[test]
fn a_missing_field_names_its_path_before_any_request() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    // The account carries neither a user name nor a token.
    let store = Store::with_document(Some(&document(
        r#""jesse": {"lastfm": {"user": "jesse1819"}}"#,
        None,
    )));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(
        output.stderr.contains("accounts.jesse.listenbrainz.user"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("accounts.jesse.listenbrainz.token"),
        "{}",
        output.stderr
    );
    assert_eq!(lb.count(), 0);
}

#[test]
fn the_user_is_derived_from_the_token_when_the_account_names_no_user() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"token": "lb-jesse-token"}}"#,
        None,
    )));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.ok();
    let token_requests = lb.route(TOKEN_ROUTE);
    assert_eq!(token_requests.len(), 1, "{:#?}", lb.requests());
    assert_eq!(
        token_requests[0].header("authorization"),
        Some("Token lb-jesse-token")
    );
    assert_eq!(output.json()["source"]["user"], "automason");
}

#[test]
fn no_credential_value_is_ever_printed() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    // A success path that uses the token…
    let ok = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &[
            "radio",
            "--prompt",
            "artist:(queen)",
            "--mode",
            "easy",
            "--output",
            "json",
        ],
    );
    ok.ok()
        .asserts_absent("lb-jesse-token")
        .asserts_absent("shared-app-key");

    // …and a failure path that reports a rejected token.
    let rejecting = Mock::start(|request| {
        if request.route() == TOKEN_ROUTE || request.route() == RADIO_ROUTE {
            Response::status(401, r#"{"error":"Invalid token"}"#)
        } else {
            Response::json(recs_body(&["rec-a"]))
        }
    });
    let failed = jam(
        &store,
        &endpoints(&rejecting, &qobuz),
        &[
            "radio",
            "--prompt",
            "artist:(queen)",
            "--mode",
            "easy",
            "--output",
            "json",
        ],
    );
    failed
        .failed()
        .asserts_absent("lb-jesse-token")
        .asserts_absent("shared-app-key");
}

// ------------------------------------------------------------------- the CLI

#[test]
fn help_lists_every_subcommand_and_both_sink_families() {
    let store = Store::with_document(None);
    let output = jam(&store, &[], &["--help"]);
    output.ok();
    for expected in ["recs", "radio", "scrobble", "status"] {
        assert!(output.stdout.contains(expected), "{}", output.stdout);
    }
    let qobuz = jam(&store, &[], &["recs", "--help"]);
    qobuz.ok();
    assert!(qobuz.stdout.contains("json"), "{}", qobuz.stdout);
    assert!(qobuz.stdout.contains("qobuz"), "{}", qobuz.stdout);
    let listening = jam(&store, &[], &["scrobble", "--help"]);
    listening.ok();
    assert!(
        listening.stdout.contains("listenbrainz"),
        "{}",
        listening.stdout
    );
}

#[test]
fn no_subcommand_prints_usage_and_fails() {
    let store = Store::with_document(None);
    let output = jam(&store, &[], &[]);
    output.failed();
    assert!(output.stderr.contains("recs"), "{}", output.stderr);
}

#[test]
fn a_missing_sink_fails_before_any_request() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(&store, &endpoints(&lb, &qobuz), &["recs"]);
    output.failed();
    assert!(output.stderr.contains("--output"), "{}", output.stderr);
    assert!(output.stderr.contains("json"), "{}", output.stderr);
    assert_eq!(lb.count(), 0);
}

#[test]
fn an_unknown_sink_fails_before_any_request() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "spotify"],
    );
    output.failed();
    assert_eq!(lb.count(), 0);
}

#[test]
fn a_sink_from_the_other_direction_is_rejected() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "listenbrainz"],
    );
    output.failed();
    assert_eq!(lb.count(), 0);

    let scrobble = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["scrobble", "--input", "-", "--output", "qobuz"],
    );
    scrobble.failed();
    assert_eq!(lb.count(), 0);
}

#[test]
fn status_needs_no_sink() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(&store, &endpoints(&lb, &qobuz), &["status"]);
    output.ok();
    assert!(
        output.stdout.contains("credential: ok"),
        "{}",
        output.stdout
    );
    assert!(output.stdout.contains("automason"), "{}", output.stdout);
    assert!(output.stdout.contains("model: ready"), "{}", output.stdout);
    assert!(output.stdout.contains("2025-10-09"), "{}", output.stdout);
}

#[test]
fn status_reports_no_model_and_fails() {
    let lb = Mock::start(|request| match request.route() {
        RECS_ROUTE => Response::no_content(),
        _ => Response::json(r#"{"user_name":"automason"}"#),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(&store, &endpoints(&lb, &qobuz), &["status"]);
    output.failed();
    assert!(output.stdout.contains("model: none"), "{}", output.stdout);
    assert!(
        output.stderr.contains("no recommendations"),
        "{}",
        output.stderr
    );
    assert!(output.stderr.contains("jam status"), "{}", output.stderr);
}

#[test]
fn status_reports_an_unknown_user() {
    let lb = Mock::start(|request| match request.route() {
        RECS_ROUTE => Response::status(404, r#"{"error":"unknown user"}"#),
        _ => Response::json(r#"{"user_name":"automason"}"#),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(&store, &endpoints(&lb, &qobuz), &["status"]);
    output.failed();
    assert!(output.stderr.contains("does not know"), "{}", output.stderr);
}

#[test]
fn status_requires_a_credential_and_names_the_command() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(None);

    let output = jam(&store, &endpoints(&lb, &qobuz), &["status"]);
    output.failed();
    assert!(
        output.stderr.contains("octx creds add jam"),
        "{}",
        output.stderr
    );
    assert_eq!(lb.count(), 0);
}

#[test]
fn status_changes_nothing() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    jam(&store, &endpoints(&lb, &qobuz), &["status"]).ok();
    for request in lb.requests() {
        assert_eq!(request.method, "GET", "{request:#?}");
    }
    assert_eq!(qobuz.count(), 0);
}

// ------------------------------------------------------------- recommendation

#[test]
fn an_explicit_count_is_forwarded() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--count", "50", "--output", "json"],
    )
    .ok();
    assert_eq!(
        lb.route(RECS_ROUTE)[0].query("count").as_deref(),
        Some("50")
    );
}

#[test]
fn a_count_beyond_the_maximum_is_clamped() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--count", "5000", "--output", "json"],
    )
    .ok();
    assert_eq!(
        lb.route(RECS_ROUTE)[0].query("count").as_deref(),
        Some("1000")
    );
}

#[test]
fn no_model_is_a_failure_and_writes_a_document_to_nobody() {
    let lb = Mock::start(|request| match request.route() {
        RECS_ROUTE => Response::no_content(),
        _ => Response::json("{}"),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(output.stdout.trim().is_empty(), "{}", output.stdout);
    assert!(
        output.stderr.contains("no recommendations"),
        "{}",
        output.stderr
    );
    // The absent-data response is terminal: it is not retried.
    assert_eq!(lb.count(), 1, "{:#?}", lb.requests());
    assert_eq!(qobuz.count(), 0);
}

#[test]
fn an_empty_model_is_a_failure_too() {
    let lb = listenbrainz(recs_body(&[]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(output.stdout.trim().is_empty(), "{}", output.stdout);
    assert_eq!(lb.route(PLAYER_ROUTE).len(), 0);
}

#[test]
fn an_unknown_user_is_reported() {
    let lb = Mock::start(|request| match request.route() {
        RECS_ROUTE => Response::status(404, r#"{"error":"unknown user"}"#),
        _ => Response::json("{}"),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(output.stderr.contains("does not know"), "{}", output.stderr);
}

#[test]
fn the_entries_keep_the_recommendation_order() {
    let lb = listenbrainz(recs_body(&["c", "a", "b"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.ok();
    let value = output.json();
    let titles: Vec<&str> = value["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["title"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(titles, vec!["Track c", "Track a", "Track b"]);
}

// ------------------------------------------------------------ normalization

/// Run a `recs` whose model holds `count` recordings, and return the player
/// requests the arm made.
fn chunked_player_requests(count: usize) -> Vec<support::Request> {
    let mbids: Vec<String> = (0..count).map(|index| format!("m{index}")).collect();
    let refs: Vec<&str> = mbids.iter().map(String::as_str).collect();
    let lb = listenbrainz(recs_body(&refs));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    )
    .ok();
    lb.route(PLAYER_ROUTE)
}

fn chunk_sizes(requests: &[support::Request]) -> Vec<usize> {
    requests
        .iter()
        .map(|request| {
            request
                .query("recording_mbids")
                .unwrap_or_default()
                .split(',')
                .filter(|mbid| !mbid.is_empty())
                .count()
        })
        .collect()
}

#[test]
fn forty_nine_recordings_are_one_request() {
    let sizes = chunk_sizes(&chunked_player_requests(49));
    assert_eq!(sizes, vec![49]);
}

#[test]
fn fifty_recordings_are_one_request() {
    let sizes = chunk_sizes(&chunked_player_requests(50));
    assert_eq!(sizes, vec![50]);
}

#[test]
fn fifty_one_recordings_are_two_requests() {
    let sizes = chunk_sizes(&chunked_player_requests(51));
    assert_eq!(sizes, vec![50, 1]);
}

#[test]
fn one_hundred_and_twenty_recordings_are_three_requests() {
    let sizes = chunk_sizes(&chunked_player_requests(120));
    assert_eq!(sizes, vec![50, 50, 20]);
}

#[test]
fn a_changed_player_shape_fails_loudly() {
    // The `playlist` key nested a level shallower than the live response.
    let lb = Mock::start(|request| match request.route() {
        RECS_ROUTE => Response::json(recs_body(&["rec-a"])),
        PLAYER_ROUTE => Response::json(r#"{"playlist": {"track": []}}"#),
        _ => Response::status(404, "{}"),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(
        output.stderr.contains("not the expected shape"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_changed_recommendation_shape_fails_loudly() {
    let lb = Mock::start(|request| match request.route() {
        // The `payload` key renamed upstream.
        RECS_ROUTE => Response::json(r#"{"result": {"mbids": []}}"#),
        PLAYER_ROUTE => Response::json(player_body("rec-a")),
        _ => Response::status(404, "{}"),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.failed();
    assert!(
        output.stderr.contains("not the expected shape"),
        "{}",
        output.stderr
    );
}

// ---------------------------------------------------------------- the radio

#[test]
fn radio_forwards_its_prompt_and_mode_and_parses_the_playlist() {
    let lb = listenbrainz(recs_body(&[]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &[
            "radio",
            "--prompt",
            "artist:(queen)",
            "--mode",
            "hard",
            "--output",
            "json",
        ],
    );
    output.ok();

    let request = &lb.route(RADIO_ROUTE)[0];
    assert_eq!(request.query("prompt").as_deref(), Some("artist:(queen)"));
    assert_eq!(request.query("mode").as_deref(), Some("hard"));
    assert_eq!(
        request.header("authorization"),
        Some("Token lb-jesse-token")
    );

    let value = output.json();
    assert_eq!(value["source"]["source"], "radio");
    assert_eq!(value["source"]["prompt"], "artist:(queen)");
    assert_eq!(value["source"]["mode"], "hard");
    assert_eq!(value["entries"][0]["title"], "Misfire");
    assert_eq!(value["entries"][0]["artist_credit"], "Queen");
    assert_eq!(value["entries"][0]["duration_ms"], 114_000);
    assert_eq!(
        value["entries"][0]["recording_mbid"],
        "1123e365-7b81-4a1f-b876-877c4bc0ca1e"
    );
}

#[test]
fn radio_without_a_prompt_or_mode_names_the_missing_argument() {
    let lb = listenbrainz(recs_body(&[]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let no_prompt = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["radio", "--mode", "easy", "--output", "json"],
    );
    no_prompt.failed();
    assert!(
        no_prompt.stderr.contains("--prompt"),
        "{}",
        no_prompt.stderr
    );

    let no_mode = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["radio", "--prompt", "artist:(queen)", "--output", "json"],
    );
    no_mode.failed();
    assert!(no_mode.stderr.contains("--mode"), "{}", no_mode.stderr);

    let bad_mode = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &[
            "radio",
            "--prompt",
            "artist:(queen)",
            "--mode",
            "wild",
            "--output",
            "json",
        ],
    );
    bad_mode.failed();
    assert_eq!(lb.count(), 0);
}

#[test]
fn radio_needs_a_token_and_says_which_field() {
    let lb = listenbrainz(recs_body(&[]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason"}}"#,
        None,
    )));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &[
            "radio",
            "--prompt",
            "artist:(queen)",
            "--mode",
            "easy",
            "--output",
            "json",
        ],
    );
    output.failed();
    assert!(
        output.stderr.contains("accounts.jesse.listenbrainz.token"),
        "{}",
        output.stderr
    );
    assert_eq!(lb.count(), 0);
}

// --------------------------------------------------------------- throttling

#[test]
fn requests_to_one_service_are_spaced() {
    let lb = listenbrainz(recs_body(&["a", "b"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    jam_with_env(
        &store,
        &endpoints(&lb, &qobuz),
        &[("OCTX_JAM_INTERVAL_MS", "120")],
        &["recs", "--output", "json"],
    )
    .ok();

    let requests = lb.requests();
    assert!(requests.len() >= 2, "{requests:#?}");
    for pair in requests.windows(2) {
        let gap = pair[1].at.duration_since(pair[0].at);
        assert!(
            gap >= std::time::Duration::from_millis(100),
            "requests were {gap:?} apart, which is inside the guidance: {requests:#?}"
        );
    }
}

#[test]
fn a_rate_limited_request_is_retried_rather_than_failing() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let attempts = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&attempts);

    let lb = Mock::start(move |request| match request.route() {
        RECS_ROUTE => {
            // Rate-limit the first attempt only.
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                Response::status(429, r#"{"error":"slow down"}"#)
            } else {
                Response::json(recs_body(&["rec-a"]))
            }
        }
        PLAYER_ROUTE => Response::json(player_body(
            &request.query("recording_mbids").unwrap_or_default(),
        )),
        _ => Response::status(404, "{}"),
    });
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.ok();
    assert_eq!(lb.route(RECS_ROUTE).len(), 2, "the 429 was retried");
    assert_eq!(output.json()["entries"][0]["recording_mbid"], "rec-a");
}

// --------------------------------------------------------------- the sinks

#[test]
fn the_json_sink_is_qobuz_free() {
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    // An account with no Qobuz field at all.
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "json"],
    );
    output.ok();
    assert_eq!(qobuz.count(), 0);
    let value = output.json();
    assert_eq!(value["kind"], "playlist");
    assert_eq!(value["version"], 1);
}

#[test]
fn radio_with_only_a_listenbrainz_credential_needs_no_qobuz_credential() {
    let lb = listenbrainz(recs_body(&[]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&document(
        r#""jesse": {"listenbrainz": {"user": "automason", "token": "lb-jesse-token"}}"#,
        None,
    )));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &[
            "radio",
            "--prompt",
            "tag:(shoegaze)",
            "--mode",
            "easy",
            "--output",
            "json",
        ],
    );
    output.ok();
    assert_eq!(qobuz.count(), 0);
    assert!(
        output.json()["entries"]
            .as_array()
            .is_some_and(|e| !e.is_empty())
    );
}

#[test]
fn the_qobuz_sink_is_not_silently_ignored() {
    // The Qobuz sink needs a Qobuz credential, so this account (which carries
    // only a ListenBrainz one) fails loudly rather than writing nothing.
    let lb = listenbrainz(recs_body(&["rec-a"]));
    let qobuz = no_requests();
    let store = Store::with_document(Some(&one_account()));

    let output = jam(
        &store,
        &endpoints(&lb, &qobuz),
        &["recs", "--output", "qobuz"],
    );
    output.failed();
    assert!(output.stderr.contains("qobuz"), "{}", output.stderr);
}
