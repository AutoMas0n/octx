//! The Last.fm-seeded discovery source, driven against a mock Last.fm.
//!
//! These call the library's discovery function directly rather than the binary:
//! the subcommand that reaches it does not exist until the CLI work lands, and
//! the rules below — the caps, the interleave, the exclusion — are the source's
//! own, so they are asserted where they live. What a run writes to a sink is a
//! property of the orchestration and is covered with it.

mod support;

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::{Value, json};
use support::{Mock, Response};

use octx_jam::config::Config;
use octx_jam::discover::{self, Discovered, MAX_SEEDS, Seed};
use octx_jam::http::Http;

const API_KEY: &str = "shared-app-key";
const USER: &str = "jesse1819";
const LASTFM_ROUTE: &str = "/2.0/";

/// A canned Last.fm, keyed by the method each request names.
#[derive(Default, Clone)]
struct Catalog {
    /// `user.getTopArtists`, most-played first.
    top_artists: Vec<String>,
    /// `artist.getSimilar`, by seed artist.
    similar: HashMap<String, Vec<String>>,
    /// `artist.getTopTracks`, by artist, as `(artist, title)`.
    top_tracks: HashMap<String, Vec<(String, String)>>,
    /// `tag.getTopTracks`, by tag, as `(artist, title)`.
    tag_tracks: HashMap<String, Vec<(String, String)>>,
    /// `user.getrecenttracks`, as `(artist, title)`.
    scrobbles: Vec<(String, String)>,
}

impl Catalog {
    /// A catalog where each seed artist expands into the given similar artists,
    /// each of which contributes the given tracks.
    fn with_similar(
        top_artists: &[&str],
        similar: &[(&str, &[&str])],
        tracks: &[(&str, &[(&str, &str)])],
    ) -> Self {
        Self {
            top_artists: top_artists.iter().map(|name| name.to_string()).collect(),
            similar: similar
                .iter()
                .map(|(seed, names)| {
                    (
                        seed.to_string(),
                        names.iter().map(|name| name.to_string()).collect(),
                    )
                })
                .collect(),
            top_tracks: tracks
                .iter()
                .map(|(artist, items)| {
                    (
                        artist.to_string(),
                        items
                            .iter()
                            .map(|(artist, title)| (artist.to_string(), title.to_string()))
                            .collect(),
                    )
                })
                .collect(),
            ..Self::default()
        }
    }

    /// The catalog as a mock Last.fm.
    fn mock(self) -> Mock {
        Mock::start(move |request| {
            let method = request.query("method").unwrap_or_default();
            match method.as_str() {
                "user.getTopArtists" => Response::json(
                    json!({ "topartists": { "artist": names(&self.top_artists) } }).to_string(),
                ),
                "artist.getSimilar" => {
                    let artist = request.query("artist").unwrap_or_default();
                    let similar = self.similar.get(&artist).cloned().unwrap_or_default();
                    Response::json(
                        json!({ "similarartists": { "artist": names(&similar) } }).to_string(),
                    )
                }
                "artist.getTopTracks" => {
                    let artist = request.query("artist").unwrap_or_default();
                    let tracks = self.top_tracks.get(&artist).cloned().unwrap_or_default();
                    Response::json(tracks_body("toptracks", &tracks))
                }
                "tag.getTopTracks" => {
                    let tag = request.query("tag").unwrap_or_default();
                    let tracks = self.tag_tracks.get(&tag).cloned().unwrap_or_default();
                    Response::json(tracks_body("tracks", &tracks))
                }
                "user.getrecenttracks" => Response::json(recent_body(&self.scrobbles)),
                _ => Response::status(404, r#"{"error":6,"message":"no such method"}"#),
            }
        })
    }
}

/// The `artist` array a name list serializes to.
fn names(names: &[String]) -> Vec<Value> {
    names.iter().map(|name| json!({ "name": name })).collect()
}

/// A list of tracks under a named block.
fn tracks_body(block: &str, tracks: &[(String, String)]) -> String {
    json!({
        block: {
            "track": tracks
                .iter()
                .map(|(artist, title)| json!({ "name": title, "artist": { "name": artist } }))
                .collect::<Vec<_>>()
        }
    })
    .to_string()
}

/// The scrobble page `user.getrecenttracks` answers with.
fn recent_body(tracks: &[(String, String)]) -> String {
    json!({
        "recenttracks": {
            "track": tracks
                .iter()
                .map(|(artist, title)| json!({
                    "artist": { "#text": artist },
                    "name": title,
                    "date": { "uts": "1760013000" }
                }))
                .collect::<Vec<_>>()
        }
    })
    .to_string()
}

/// An HTTP client pointed at the mock, with the throttle turned off.
fn http(mock: &Mock) -> Http {
    let cfg = Config {
        lastfm_api: format!("{}{LASTFM_ROUTE}", mock.base),
        other_interval: Duration::ZERO,
        ..Config::default()
    };
    Http::new(cfg).expect("build an HTTP client")
}

/// The methods a run asked the mock for, in order.
fn methods(mock: &Mock) -> Vec<String> {
    mock.requests()
        .into_iter()
        .filter_map(|request| request.query("method"))
        .collect()
}

/// How many times a run asked the mock for one method.
fn calls(mock: &Mock, method: &str) -> usize {
    methods(mock)
        .into_iter()
        .filter(|name| name == method)
        .count()
}

/// The titles a discovery result holds, in order.
fn titles(discovered: &Discovered) -> Vec<String> {
    match discovered {
        Discovered::Ready(entries) => entries.iter().map(|entry| entry.title.clone()).collect(),
        Discovered::Nothing => Vec::new(),
    }
}

/// Run the source over the catalog and return what it produced.
async fn run(catalog: Catalog, seed: &Seed, count: Option<usize>) -> (Mock, Discovered) {
    run_remembering(catalog, seed, count, &HashSet::new()).await
}

/// Run the source with a set of recently delivered tracks to exclude.
async fn run_remembering(
    catalog: Catalog,
    seed: &Seed,
    count: Option<usize>,
    remembered: &HashSet<String>,
) -> (Mock, Discovered) {
    let mock = catalog.mock();
    let discovered = discover::discover(&http(&mock), API_KEY, USER, seed, count, remembered)
        .await
        .expect("run the discovery source");
    (mock, discovered)
}

#[tokio::test]
async fn a_remembered_track_is_not_delivered_again() {
    // Two similar artists each with one track: with the first remembered, the
    // second must still be delivered rather than the result coming up short.
    let catalog = Catalog::with_similar(
        &[],
        &[("Seed", &["Sim A", "Sim B"])],
        &[
            ("Sim A", &[("Sim A", "A One")]),
            ("Sim B", &[("Sim B", "B One")]),
        ],
    );
    let remembered: HashSet<String> = [discover::exclusion_key("Sim A", "A One")]
        .into_iter()
        .collect();
    let (_, discovered) =
        run_remembering(catalog, &Seed::Artist("Seed".into()), Some(10), &remembered).await;
    let titles = titles(&discovered);
    assert!(!titles.contains(&"A One".to_string()), "{titles:?}");
    assert!(titles.contains(&"B One".to_string()), "{titles:?}");
}

// ------------------------------------------------- seeding and fan-out

#[tokio::test]
async fn the_accounts_top_artists_are_expanded_through_similar_artists() {
    // The seed artist's own tracks and its similar artists' tracks both count.
    let catalog = Catalog::with_similar(
        &["Seed One"],
        &[("Seed One", &["Sim One A"])],
        &[
            ("Seed One", &[("Seed One", "Own Track")]),
            ("Sim One A", &[("Sim One A", "Sim Track")]),
        ],
    );

    let (mock, discovered) = run(catalog, &Seed::Account, Some(10)).await;
    let mut titles = titles(&discovered);
    titles.sort();
    assert_eq!(titles, vec!["Own Track", "Sim Track"]);
    assert!(calls(&mock, "user.getTopArtists") == 1);
    assert!(calls(&mock, "artist.getSimilar") == 1);
}

#[tokio::test]
async fn a_seed_artist_fans_out_through_similar_artists() {
    // One seed, several similar artists: the result must draw on more than one
    // of them rather than dumping the first one's catalogue.
    let catalog = Catalog::with_similar(
        &[],
        &[("Reneé Rapp", &["Sim A", "Sim B"])],
        &[
            ("Sim A", &[("Sim A", "A One"), ("Sim A", "A Two")]),
            ("Sim B", &[("Sim B", "B One")]),
        ],
    );

    let (mock, discovered) = run(catalog, &Seed::Artist("Reneé Rapp".to_string()), Some(10)).await;
    let mut titles = titles(&discovered);
    titles.sort();
    assert_eq!(titles, vec!["A One", "A Two", "B One"]);

    // A seed replaces the account's own top artists; they are never read.
    assert_eq!(calls(&mock, "user.getTopArtists"), 0);
    assert_eq!(calls(&mock, "artist.getSimilar"), 1);
}

#[tokio::test]
async fn the_result_is_spread_across_seed_artists() {
    // The first seed's bucket could fill the whole result on its own. The draw
    // must still return entries from the second seed.
    let catalog = Catalog::with_similar(
        &["Seed One", "Seed Two"],
        &[("Seed One", &[]), ("Seed Two", &[])],
        &[
            (
                "Seed One",
                &[
                    ("Seed One", "One 1"),
                    ("Seed One", "One 2"),
                    ("Seed One", "One 3"),
                ],
            ),
            ("Seed Two", &[("Seed Two", "Two 1")]),
        ],
    );

    let (_, discovered) = run(catalog, &Seed::Account, Some(3)).await;
    let titles = titles(&discovered);
    assert_eq!(titles.len(), 3);
    assert!(titles.contains(&"One 1".to_string()), "{titles:?}");
    assert!(titles.contains(&"Two 1".to_string()), "{titles:?}");
}

// ------------------------------------------------------- exclusion

#[tokio::test]
async fn an_already_scrobbled_candidate_is_excluded() {
    let mut catalog = Catalog::with_similar(
        &["Seed One"],
        &[("Seed One", &[])],
        &[("Seed One", &[("Seed One", "Known"), ("Seed One", "Fresh")])],
    );
    catalog.scrobbles = vec![("Seed One".to_string(), "Known".to_string())];

    let (_, discovered) = run(catalog, &Seed::Account, Some(10)).await;
    assert_eq!(titles(&discovered), vec!["Fresh"]);
}

#[tokio::test]
async fn the_exclusion_matches_case_insensitively() {
    let mut catalog = Catalog::with_similar(
        &["Seed One"],
        &[("Seed One", &[])],
        &[("Seed One", &[("Seed One", "Known Track")])],
    );
    catalog.scrobbles = vec![("seed one".to_string(), "known track".to_string())];

    let (_, discovered) = run(catalog, &Seed::Account, Some(10)).await;
    assert_eq!(discovered, Discovered::Nothing);
}

#[tokio::test]
async fn the_accounts_listening_is_read_once_per_run() {
    let catalog = Catalog::with_similar(
        &["Seed One", "Seed Two"],
        &[("Seed One", &["Sim A"]), ("Seed Two", &["Sim B"])],
        &[
            ("Seed One", &[("Seed One", "One 1")]),
            ("Sim A", &[("Sim A", "A 1")]),
            ("Seed Two", &[("Seed Two", "Two 1")]),
            ("Sim B", &[("Sim B", "B 1")]),
        ],
    );

    let (mock, _) = run(catalog, &Seed::Account, Some(10)).await;
    assert_eq!(
        calls(&mock, "user.getrecenttracks"),
        1,
        "one exclusion read for the whole run"
    );
}

// --------------------------------------------------- nothing to give

#[tokio::test]
async fn an_account_with_no_scrobbles_has_nothing_to_seed_from() {
    let catalog = Catalog::default();
    let (_, discovered) = run(catalog, &Seed::Account, Some(10)).await;
    assert_eq!(discovered, Discovered::Nothing);

    let message = discover::no_listening(USER);
    assert!(message.contains("Last.fm"), "{message}");
}

// ------------------------------------------------------ a prompt is a tag

#[tokio::test]
async fn a_prompt_reads_the_tags_own_tracks() {
    let mut catalog = Catalog::default();
    catalog.tag_tracks.insert(
        "chillwave".to_string(),
        vec![
            ("Washed Out".to_string(), "Feel It All Around".to_string()),
            ("Tycho".to_string(), "Awake".to_string()),
        ],
    );

    let (mock, discovered) = run(catalog, &Seed::Tag("chillwave".to_string()), Some(10)).await;
    let mut titles = titles(&discovered);
    titles.sort();
    assert_eq!(titles, vec!["Awake", "Feel It All Around"]);

    // The tag is read directly, and the account's own artists are not seeds.
    assert_eq!(calls(&mock, "tag.getTopTracks"), 1);
    assert_eq!(calls(&mock, "user.getTopArtists"), 0);
}

#[tokio::test]
async fn a_prompt_produces_a_result_for_an_account_with_no_top_artists() {
    // A young account has no top artists and no scrobbles; the tag still has
    // tracks, so the prompt must not depend on the account having any.
    let mut catalog = Catalog::default();
    catalog.tag_tracks.insert(
        "chillwave".to_string(),
        vec![("Washed Out".to_string(), "Feel It All Around".to_string())],
    );

    let (_, discovered) = run(catalog, &Seed::Tag("chillwave".to_string()), None).await;
    assert_eq!(titles(&discovered), vec!["Feel It All Around"]);
}

// ------------------------------------------------------------- caps

#[tokio::test]
async fn the_seed_and_fan_out_caps_bound_the_calls() {
    // Far more seeds and fan-out than the caps allow, so a run that ignored
    // them would make a request per artist named.
    let many: Vec<String> = (0..40).map(|index| format!("Seed {index}")).collect();
    let mut catalog = Catalog {
        top_artists: many.clone(),
        ..Catalog::default()
    };
    for seed in &many {
        catalog.similar.insert(
            seed.clone(),
            (0..40).map(|index| format!("{seed} Sim {index}")).collect(),
        );
        for index in 0..40 {
            let artist = format!("{seed} Sim {index}");
            catalog.top_tracks.insert(
                artist.clone(),
                (0..5)
                    .map(|t| (artist.clone(), format!("{artist} T{t}")))
                    .collect(),
            );
        }
    }

    let (mock, _) = run(catalog, &Seed::Account, Some(25)).await;

    assert!(
        calls(&mock, "artist.getSimilar") <= MAX_SEEDS,
        "one similar read per expanded seed"
    );
    let top_track_calls = calls(&mock, "artist.getTopTracks");
    assert!(
        top_track_calls <= MAX_SEEDS * (discover::MAX_SIMILAR + 1),
        "per-seed fan-out bounded, got {top_track_calls}"
    );
}
