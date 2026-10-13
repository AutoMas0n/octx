//! `jam discover`, driven end to end through the binary against a mock Last.fm.
//!
//! These prove the wiring rather than the ranking: that the source runs with no
//! ListenBrainz credential, that it reads only the Last.fm account's own data,
//! and that the dated default lands on one playlist per UTC day.

mod support;

use support::{Mock, Response, Store, document, jam, no_requests};

const LASTFM_ROUTE: &str = "/2.0/";

/// A document whose account has a Last.fm key and user but **no** ListenBrainz
/// credential at all — the point being that discovery must not need one.
fn lastfm_only_account() -> String {
    document(r#""jesse": {"lastfm": {"user": "jesse1819"}}"#, None)
}

/// A document whose account carries a ListenBrainz credential too, so the
/// absence of LB traffic in a discovery run cannot be blamed on its absence.
fn both_account() -> String {
    document(
        r#""jesse": {"lastfm": {"user": "jesse1819"},
                     "listenbrainz": {"user": "automason", "token": "lb-jesse-token"}}"#,
        None,
    )
}

/// A Last.fm answering the discovery read endpoints with a small, fixed catalog.
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
                let title = format!("{artist} Track");
                Response::json(format!(
                    r#"{{"toptracks":{{"track":[{{"name":"{title}","artist":{{"name":"{artist}"}}}}],"@attr":{{"totalPages":"1"}}}}}}"#
                ))
            }
            Some("user.getrecenttracks") => {
                Response::json(r#"{"recenttracks":{"track":[],"@attr":{"totalPages":"1"}}}"#)
            }
            _ => Response::status(404, r#"{"error":6,"message":"no such method"}"#),
        }
    })
}

/// The endpoints a discovery run needs: Last.fm only.
fn endpoints(lastfm: &Mock) -> Vec<(&'static str, String)> {
    vec![(
        "OCTX_JAM_LASTFM_API",
        format!("{}{LASTFM_ROUTE}", lastfm.base),
    )]
}

/// Run `discover --output json` and return the parsed document.
fn discover_json(store: &Store, lastfm: &Mock, extra: &[&str]) -> support::Output {
    let mut args = vec!["discover", "--output", "json"];
    args.extend_from_slice(extra);
    jam(store, &endpoints(lastfm), &args)
}

#[test]
fn discover_runs_without_a_listenbrainz_credential() {
    // The account has no ListenBrainz credential; if the run needed one it
    // would fail before producing a document.
    let lastfm = lastfm();
    let store = Store::with_document(Some(&lastfm_only_account()));

    let output = discover_json(&store, &lastfm, &[]);
    output.ok();

    let value = output.json();
    assert_eq!(value["source"]["source"], "discover");
    assert_eq!(value["source"]["user"], "jesse1819");
    assert!(
        value["entries"].as_array().is_some_and(|a| !a.is_empty()),
        "discovery produced no entries: {value}"
    );
}

#[test]
fn discover_never_contacts_listenbrainz() {
    // A document that carries both credentials, so a ListenBrainz request would
    // be possible: its absence is the assertion.
    let lastfm = lastfm();
    let store = Store::with_document(Some(&both_account()));

    // A ListenBrainz mock that would record any request; none may arrive.
    let lb = no_requests();
    let mut endpoints = endpoints(&lastfm);
    endpoints.push(("OCTX_JAM_LB_API", lb.base.clone()));

    let output = jam(&store, &endpoints, &["discover", "--output", "json"]);
    output.ok();
    assert_eq!(lb.count(), 0, "discovery must not contact ListenBrainz");
}

#[test]
fn a_seed_artist_skips_the_accounts_top_artists() {
    let lastfm = lastfm();
    let store = Store::with_document(Some(&both_account()));

    let output = discover_json(&store, &lastfm, &["--seed-artist", "Reneé Rapp"]);
    output.ok();

    let methods: Vec<String> = lastfm
        .requests()
        .into_iter()
        .filter_map(|request| request.query("method"))
        .collect();
    assert!(
        !methods.iter().any(|m| m == "user.getTopArtists"),
        "a seed artist must replace the account default: {methods:?}"
    );
    assert!(
        methods.iter().any(|m| m == "artist.getSimilar"),
        "a seed artist must fan out through similar artists: {methods:?}"
    );
}

#[test]
fn seed_artist_and_prompt_together_are_rejected() {
    let lastfm = lastfm();
    let store = Store::with_document(Some(&both_account()));

    let output = discover_json(
        &store,
        &lastfm,
        &["--seed-artist", "Reneé Rapp", "--prompt", "chillwave"],
    );
    output.failed();
    assert!(
        output.stderr.contains("--seed-artist") || output.stderr.contains("--prompt"),
        "{}",
        output.stderr
    );
    assert_eq!(
        lastfm.count(),
        0,
        "no request may precede the argument check"
    );
}

#[test]
fn a_prompt_seeds_from_a_tag() {
    let lastfm = Mock::start(|request| {
        if request.route() != LASTFM_ROUTE {
            return Response::status(404, r#"{"error":"no such route"}"#);
        }
        match request.query("method").as_deref() {
            Some("tag.getTopTracks") => Response::json(
                r#"{"tracks":{"track":[{"name":"Genesis","artist":{"name":"Grimes"}}],"@attr":{"totalPages":"1"}}}"#,
            ),
            Some("user.getrecenttracks") => {
                Response::json(r#"{"recenttracks":{"track":[],"@attr":{"totalPages":"1"}}}"#)
            }
            _ => Response::status(404, r#"{"error":6,"message":"no such method"}"#),
        }
    });
    let store = Store::with_document(Some(&both_account()));

    let output = discover_json(&store, &lastfm, &["--prompt", "chillwave"]);
    output.ok();

    let value = output.json();
    assert_eq!(value["entries"][0]["title"], "Genesis");
    assert_eq!(value["entries"][0]["artist_credit"], "Grimes");
}

#[test]
fn the_dated_default_names_one_playlist_per_utc_day() {
    // The name is derived from the run's date. Two runs on one day must name
    // the same playlist; the name must carry a UTC date and no MBID-style id.
    let name = octx_jam::sync::default_playlist("discover", "2025-10-13");
    assert_eq!(name, "Last.fm Discovery 2025-10-13");
    assert_eq!(
        name,
        octx_jam::sync::default_playlist("discover", "2025-10-13"),
        "the same date must always derive the same name"
    );
    assert_ne!(
        name,
        octx_jam::sync::default_playlist("discover", "2025-10-14"),
        "a different day must name a different playlist"
    );
}
