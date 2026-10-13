//! The Last.fm read source.
//!
//! Qobuz publishes plays from its own servers, so Last.fm is the documented
//! route to a Qobuz history. `user.getRecentTracks` needs only the
//! application's API key — unsigned, no user token — which is why the key is
//! shared across accounts while the user name selects whose scrobbles are read.
//!
//! The mapper deliberately requests no `extended` parameter: adding it
//! silently re-shapes `artist` and `album` from `{"#text": …}` to `{"name": …}`.
//! Both shapes are accepted here anyway, so the day someone turns it on is not
//! the day the arm breaks.
//!
//! The discovery source reads the same key from a different angle: the account's
//! top artists, the artists Last.fm calls similar to them, and those artists'
//! popular tracks. None of those endpoints needs more than the API key either.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::document::{Attribution, Listen};
use crate::http::Http;
use crate::scrobble::identity;

/// The most scrobbles the API returns in one page.
pub const PAGE_SIZE: usize = 200;
/// Scrobbles fetched when the caller names no limit.
pub const DEFAULT_LIMIT: usize = 200;
/// A guard against paging forever on an upstream that always answers.
const MAX_PAGES: usize = 20;

/// What a Last.fm fetch produced.
pub struct Fetched {
    /// The listens, newest first, as Last.fm returned them.
    pub listens: Vec<Listen>,
    /// The account the scrobbles belong to.
    pub user: String,
    /// The credit the API terms require on any copy of this data.
    pub attribution: Attribution,
}

/// Fetch an account's recent scrobbles.
///
/// `user` is the Last.fm account to read; `identity_user` is what listen
/// identities are derived from, so that the same invocations keep deriving the
/// same identities.
pub async fn recent_tracks(
    http: &Http,
    api_key: &str,
    user: &str,
    limit: usize,
    identity_user: &str,
) -> Result<Fetched> {
    let mut listens: Vec<Listen> = Vec::new();
    let per_page = limit.clamp(1, PAGE_SIZE);
    let mut page = 1usize;

    loop {
        let url = format!(
            "{}?method=user.getrecenttracks&user={}&api_key={}&format=json&limit={}&page={}",
            http.cfg.lastfm_api,
            urlencode(user),
            urlencode(api_key),
            per_page,
            page
        );
        let response = http.send_other(|| http.client.get(&url)).await?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("could not read the response from Last.fm")?;

        let parsed: RecentTracksResponse = serde_json::from_str(&body).with_context(|| {
            format!(
                "the Last.fm response was not the expected shape: {}",
                snippet(&body)
            )
        })?;

        // Last.fm reports its own errors in a 200-shaped body with `error`.
        refused(
            parsed.error,
            parsed.message.as_deref().unwrap_or("no message"),
            status,
            &body,
        )?;

        let recent = parsed
            .recenttracks
            .ok_or_else(|| anyhow::anyhow!("the Last.fm response carried no recent tracks"))?;

        let (tracks, attr) = recent.into_parts();
        if tracks.is_empty() {
            break;
        }
        let last_page = attr.as_ref().and_then(PageAttr::pages).unwrap_or(page);

        for track in tracks {
            if let Some(listen) = track.into_listen(identity_user) {
                listens.push(listen);
                if listens.len() >= limit {
                    break;
                }
            }
        }

        if listens.len() >= limit || page >= last_page || page >= MAX_PAGES {
            break;
        }
        page += 1;
    }

    listens.truncate(limit);
    Ok(Fetched {
        listens,
        user: user.to_string(),
        attribution: Attribution::lastfm(user),
    })
}

/// The most list items one page asks for.
pub const LIST_PAGE_SIZE: usize = 200;

/// One track a list endpoint named, with the artist credited on it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Track {
    /// The artist credit, as Last.fm spells it.
    pub artist: String,
    /// The track title, as Last.fm spells it.
    pub title: String,
}

/// The artists an account listens to most, most-played first.
pub async fn top_artists(
    http: &Http,
    api_key: &str,
    user: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let params = [("user", user.to_string())];
    let items: Vec<ArtistItem> = page_through::<ArtistsPage, _>(
        http,
        api_key,
        "user.getTopArtists",
        &params,
        limit,
        |value| value.get("topartists").cloned(),
    )
    .await?;
    Ok(items.into_iter().filter_map(|item| item.name).collect())
}

/// The artists Last.fm reports as similar to one artist.
pub async fn similar_artists(
    http: &Http,
    api_key: &str,
    artist: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let params = [("artist", artist.to_string())];
    let items: Vec<ArtistItem> = page_through::<ArtistsPage, _>(
        http,
        api_key,
        "artist.getSimilar",
        &params,
        limit,
        |value| value.get("similarartists").cloned(),
    )
    .await?;
    Ok(items.into_iter().filter_map(|item| item.name).collect())
}

/// An artist's most popular tracks, most popular first.
pub async fn artist_top_tracks(
    http: &Http,
    api_key: &str,
    artist: &str,
    limit: usize,
) -> Result<Vec<Track>> {
    let params = [("artist", artist.to_string())];
    let items: Vec<TrackItem> = page_through::<TracksPage, _>(
        http,
        api_key,
        "artist.getTopTracks",
        &params,
        limit,
        |value| value.get("toptracks").cloned(),
    )
    .await?;
    Ok(items
        .into_iter()
        .filter_map(|item| {
            let title = item.name?;
            Some(Track {
                artist: item
                    .artist
                    .and_then(Text::into_string)
                    .unwrap_or_else(|| artist.to_string()),
                title,
            })
        })
        .collect())
}

/// The tracks an account has already scrobbled, for exclusion from recommendations.
pub async fn existing_tracks(
    http: &Http,
    api_key: &str,
    user: &str,
    limit: usize,
) -> Result<Vec<Track>> {
    let fetched = recent_tracks(http, api_key, user, limit, user).await?;
    Ok(tracks_of(fetched.listens))
}

/// Map listens to the artist-and-title pairs exclusion matches on.
fn tracks_of(listens: Vec<Listen>) -> Vec<Track> {
    listens
        .into_iter()
        .map(|listen| Track {
            artist: listen.artist,
            title: listen.title,
        })
        .collect()
}

/// Send one list request to Last.fm and return the parsed body.
///
/// The list endpoints answer with a named block of items and report their own
/// errors in the same `error`/`message` pair `user.getRecentTracks` uses, so the
/// check is shared and every Last.fm refusal is reported one way.
async fn list_call(
    http: &Http,
    api_key: &str,
    method: &str,
    params: &[(&str, String)],
) -> Result<serde_json::Value> {
    let mut url = format!(
        "{}?method={}&api_key={}&format=json",
        http.cfg.lastfm_api,
        urlencode(method),
        urlencode(api_key)
    );
    for (name, value) in params {
        url.push('&');
        url.push_str(&urlencode(name));
        url.push('=');
        url.push_str(&urlencode(value));
    }

    let response = http.send_other(|| http.client.get(&url)).await?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("could not read the response from Last.fm")?;

    let parsed: serde_json::Value = serde_json::from_str(&body).with_context(|| {
        format!(
            "the Last.fm response was not the expected shape: {}",
            snippet(&body)
        )
    })?;
    refused(
        parsed.get("error").and_then(serde_json::Value::as_i64),
        parsed
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("no message"),
        status,
        &body,
    )?;
    Ok(parsed)
}

/// The one place a Last.fm refusal becomes an error.
fn refused(
    error: Option<i64>,
    message: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> Result<()> {
    if let Some(code) = error {
        bail!("Last.fm refused the request (error {code}): {message}");
    }
    if !status.is_success() {
        bail!("Last.fm answered {status}: {}", snippet(body));
    }
    Ok(())
}

/// A page of a Last.fm list endpoint.
trait ListPage {
    /// What one item on the page is.
    type Item;
    /// The page's items.
    fn items(self) -> Vec<Self::Item>;
    /// How many pages the endpoint claims, when it claims a number at all.
    fn pages(&self) -> Option<usize>;
}

impl ListPage for ArtistsPage {
    type Item = ArtistItem;

    fn items(self) -> Vec<ArtistItem> {
        self.artist.map(OneOrMany::into_vec).unwrap_or_default()
    }

    fn pages(&self) -> Option<usize> {
        self.attr.as_ref().and_then(PageAttr::pages)
    }
}

impl ListPage for TracksPage {
    type Item = TrackItem;

    fn items(self) -> Vec<TrackItem> {
        self.track.map(OneOrMany::into_vec).unwrap_or_default()
    }

    fn pages(&self) -> Option<usize> {
        self.attr.as_ref().and_then(PageAttr::pages)
    }
}

/// Read a list endpoint, page by page, up to `limit` items.
///
/// `block` names the part of the answer that holds the page, because each
/// endpoint spells it differently.
async fn page_through<P, F>(
    http: &Http,
    api_key: &str,
    method: &str,
    params: &[(&str, String)],
    limit: usize,
    block: F,
) -> Result<Vec<P::Item>>
where
    P: ListPage + serde::de::DeserializeOwned,
    F: Fn(&serde_json::Value) -> Option<serde_json::Value>,
{
    let per_page = limit.clamp(1, LIST_PAGE_SIZE);
    let mut items = Vec::new();
    let mut page = 1usize;

    loop {
        let mut call = params.to_vec();
        call.push(("limit", per_page.to_string()));
        call.push(("page", page.to_string()));

        let value = list_call(http, api_key, method, &call).await?;
        let page_value = block(&value)
            .ok_or_else(|| anyhow::anyhow!("the Last.fm response carried no list to read"))?;
        let parsed: P = serde_json::from_value(page_value).map_err(|error| {
            anyhow::anyhow!("the Last.fm response was not the expected shape ({error})")
        })?;

        let total = parsed.pages();
        items.extend(parsed.items());
        if items.len() >= limit || page >= total.unwrap_or(page) || page >= MAX_PAGES {
            break;
        }
        page += 1;
    }

    items.truncate(limit);
    Ok(items)
}

/// A list that Last.fm returns as a lone object when it holds one item.
///
/// `Many` is tried first on purpose: a derived struct will happily read a
/// sequence positionally, so with `One` first an array of scrobbles deserializes
/// as a single scrobble with its fields read off the wrong elements.
#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    Many(Vec<T>),
    One(T),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            OneOrMany::One(item) => vec![item],
            OneOrMany::Many(items) => items,
        }
    }
}

/// The `artist` block of `user.getTopArtists` and `artist.getSimilar`.
#[derive(Deserialize)]
struct ArtistsPage {
    #[serde(default)]
    artist: Option<OneOrMany<ArtistItem>>,
    #[serde(default, rename = "@attr")]
    attr: Option<PageAttr>,
}

/// The `track` block of `artist.getTopTracks`.
#[derive(Deserialize)]
struct TracksPage {
    #[serde(default)]
    track: Option<OneOrMany<TrackItem>>,
    #[serde(default, rename = "@attr")]
    attr: Option<PageAttr>,
}

#[derive(Deserialize)]
struct ArtistItem {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct TrackItem {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    artist: Option<Text>,
}

/// Percent-encode a query value.
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// A couple of lines of an upstream body, for an error message.
fn snippet(body: &str) -> String {
    let trimmed = body.trim();
    let mut out: String = trimmed.chars().take(200).collect();
    if trimmed.chars().count() > 200 {
        out.push('…');
    }
    out
}

#[derive(Deserialize)]
struct RecentTracksResponse {
    #[serde(default)]
    recenttracks: Option<RecentTracks>,
    #[serde(default)]
    error: Option<i64>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct RecentTracks {
    #[serde(default)]
    track: Option<OneOrMany<Scrobble>>,
    #[serde(default, rename = "@attr")]
    attr: Option<PageAttr>,
}

impl RecentTracks {
    /// The page's scrobbles and its page metadata, together.
    ///
    /// They come out together because the scrobbles are the page and the
    /// metadata says how many more pages the endpoint holds.
    fn into_parts(self) -> (Vec<Scrobble>, Option<PageAttr>) {
        (
            self.track.map(OneOrMany::into_vec).unwrap_or_default(),
            self.attr,
        )
    }
}

#[derive(Deserialize)]
struct PageAttr {
    /// Last.fm spells it `totalPages`.
    #[serde(default, rename = "totalPages", alias = "total_pages")]
    total_pages: Option<String>,
}

impl PageAttr {
    /// The page count, when the endpoint reported one.
    fn pages(&self) -> Option<usize> {
        self.total_pages
            .as_deref()
            .and_then(|pages| pages.parse().ok())
    }
}

#[derive(Deserialize)]
struct Scrobble {
    #[serde(default)]
    artist: Option<Text>,
    #[serde(default)]
    album: Option<Text>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    date: Option<Date>,
}

impl Scrobble {
    /// Map one scrobble to a listen.
    ///
    /// A scrobble with no timestamp is the track that is playing right now,
    /// which is not a listen and is dropped.
    fn into_listen(self, identity_user: &str) -> Option<Listen> {
        let listened_at = self.date?.uts?.parse::<i64>().ok()?;
        let artist = self.artist.and_then(Text::into_string).unwrap_or_default();
        let title = self.name.unwrap_or_default();

        Some(Listen {
            listened_at,
            recording_msid: identity(identity_user, listened_at, &artist, &title),
            artist,
            title,
            album: self.album.and_then(Text::into_string),
            duration_ms: None,
        })
    }
}

#[derive(Deserialize)]
struct Date {
    #[serde(default)]
    uts: Option<String>,
}

/// A Last.fm text field: an object keyed `#text`, or a bare string.
#[derive(Deserialize)]
#[serde(untagged)]
enum Text {
    Plain(String),
    Object {
        #[serde(default)]
        name: Option<String>,
        #[serde(default, rename = "#text")]
        text: Option<String>,
    },
}

impl Text {
    fn into_string(self) -> Option<String> {
        match self {
            Text::Plain(value) => Some(value),
            Text::Object { name, text } => name.or(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrobble(body: &str) -> Vec<Listen> {
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        parsed
            .recenttracks
            .expect("recenttracks")
            .into_parts()
            .0
            .into_iter()
            .filter_map(|track| track.into_listen("automason"))
            .collect()
    }

    #[test]
    fn scrobbles_map_to_listens() {
        // Pinned against the live response: `#text` fields and a string `uts`.
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"#text": "Princess Goes", "mbid": ""},
                    "album": {"#text": "THANKS FOR COMING"},
                    "name": "Eat an Eraser",
                    "date": {"uts": "1760013000", "#text": "11 Oct 2026, 11:30"}
                }],
                "@attr": {"page": "1", "perPage": "200", "totalPages": "1", "total": "9"}
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens.len(), 1);
        assert_eq!(listens[0].listened_at, 1_760_013_000);
        assert_eq!(listens[0].artist, "Princess Goes");
        assert_eq!(listens[0].title, "Eat an Eraser");
        assert_eq!(listens[0].album.as_deref(), Some("THANKS FOR COMING"));
    }

    #[test]
    fn the_now_playing_track_is_not_a_listen() {
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"#text": "Queen"},
                    "album": {"#text": "A Night at the Opera"},
                    "name": "Bohemian Rhapsody",
                    "@attr": {"nowplaying": "true"}
                }, {
                    "artist": {"#text": "Queen"},
                    "album": {"#text": "Sheer Heart Attack"},
                    "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }]
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens.len(), 1);
        assert_eq!(listens[0].title, "Misfire");
    }

    #[test]
    fn the_extended_shape_is_tolerated() {
        // `extended=1` swaps `#text` for `name`; the mapper does not ask for it
        // but must not break if it ever arrives.
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"name": "Queen"},
                    "album": {"name": "Sheer Heart Attack"},
                    "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }]
            }
        }"##;
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        let tracks = parsed.recenttracks.expect("recenttracks").into_parts().0;
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name.as_deref(), Some("Misfire"));
    }

    #[test]
    fn the_page_count_is_read_from_the_attr_block() {
        // Pinned: Last.fm spells it `totalPages`, and getting this wrong makes
        // the fetch stop after one page without saying so.
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": {"#text": "Queen"}, "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }],
                "@attr": {"page": "1", "perPage": "200", "totalPages": "5", "total": "9"}
            }
        }"##;
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        let attr = parsed
            .recenttracks
            .expect("recenttracks")
            .attr
            .expect("attr");
        assert_eq!(attr.total_pages.as_deref(), Some("5"));
        assert_eq!(attr.pages(), Some(5));
    }

    #[test]
    fn an_error_body_is_reported() {
        let body = r##"{"error":6,"message":"User not found"}"##;
        let parsed: RecentTracksResponse = serde_json::from_str(body).expect("parse");
        assert_eq!(parsed.error, Some(6));
        assert_eq!(parsed.message.as_deref(), Some("User not found"));
    }

    #[test]
    fn a_bare_string_artist_is_accepted() {
        let body = r##"{
            "recenttracks": {
                "track": [{
                    "artist": "Queen",
                    "album": "Sheer Heart Attack",
                    "name": "Misfire",
                    "date": {"uts": "1760013000"}
                }]
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens[0].artist, "Queen");
        assert_eq!(listens[0].album.as_deref(), Some("Sheer Heart Attack"));
    }

    #[test]
    fn an_unparseable_timestamp_is_dropped_rather_than_guessed() {
        let body = r##"{
            "recenttracks": {"track": [{
                "artist": {"#text": "Queen"}, "name": "Misfire", "date": {"uts": "soon"}
            }]}
        }"##;
        assert!(scrobble(body).is_empty());
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }

    /// The names in a `topartists` or `similarartists` block.
    fn artists(body: &str, key: &str) -> Vec<String> {
        let value: serde_json::Value = serde_json::from_str(body).expect("parse");
        let page: ArtistsPage =
            serde_json::from_value(value.get(key).cloned().expect("block")).expect("page");
        page.items()
            .into_iter()
            .filter_map(|item| item.name)
            .collect()
    }

    /// The tracks in a `toptracks` block, as the discovery source reads them.
    fn tracks(body: &str, key: &str) -> Vec<Track> {
        let value: serde_json::Value = serde_json::from_str(body).expect("parse");
        let page: TracksPage =
            serde_json::from_value(value.get(key).cloned().expect("block")).expect("page");
        page.items()
            .into_iter()
            .filter_map(|item| {
                Some(Track {
                    artist: item.artist.and_then(Text::into_string)?,
                    title: item.name?,
                })
            })
            .collect()
    }

    #[test]
    fn top_artists_map_to_names() {
        // Pinned against the live `user.getTopArtists` shape.
        let body = r##"{
            "topartists": {
                "artist": [
                    {"name": "Reneé Rapp", "playcount": "31", "mbid": "", "url": "https://www.last.fm/music/Rene%C3%A9+Rapp"},
                    {"name": "Chappell Roan", "playcount": "19", "mbid": "", "url": "https://www.last.fm/music/Chappell+Roan"}
                ],
                "@attr": {"page": "1", "perPage": "50", "totalPages": "1", "total": "2"}
            }
        }"##;
        assert_eq!(
            artists(body, "topartists"),
            vec!["Reneé Rapp", "Chappell Roan"]
        );
    }

    #[test]
    fn similar_artists_map_to_names() {
        // Pinned against the live `artist.getSimilar` shape, whose `@attr`
        // block names the artist instead of counting pages.
        let body = r##"{
            "similarartists": {
                "artist": [
                    {"name": "Fletcher", "mbid": "", "match": "0.91"},
                    {"name": "Conan Gray", "mbid": "", "match": "0.82"}
                ],
                "@attr": {"artist": "Reneé Rapp"}
            }
        }"##;
        assert_eq!(
            artists(body, "similarartists"),
            vec!["Fletcher", "Conan Gray"]
        );
        let value: serde_json::Value = serde_json::from_str(body).expect("parse");
        let page: ArtistsPage =
            serde_json::from_value(value["similarartists"].clone()).expect("page");
        assert_eq!(page.pages(), None);
    }

    #[test]
    fn artist_top_tracks_map_to_tracks() {
        let body = r##"{
            "toptracks": {
                "track": [
                    {"name": "Snow Angel", "artist": {"name": "Reneé Rapp", "mbid": "", "url": "https://www.last.fm/music/Rene%C3%A9+Rapp"}},
                    {"name": "Pretty Girls", "artist": {"name": "Reneé Rapp", "mbid": ""}}
                ],
                "@attr": {"artist": "Reneé Rapp", "page": "1", "perPage": "50", "totalPages": "1", "total": "2"}
            }
        }"##;
        assert_eq!(
            tracks(body, "toptracks"),
            vec![
                Track {
                    artist: "Reneé Rapp".to_string(),
                    title: "Snow Angel".to_string()
                },
                Track {
                    artist: "Reneé Rapp".to_string(),
                    title: "Pretty Girls".to_string()
                }
            ]
        );
    }

    #[test]
    fn existing_tracks_reuse_the_scrobble_shape() {
        let body = r##"{
            "recenttracks": {"track": [{
                "artist": {"#text": "Princess Goes"}, "name": "Eat an Eraser",
                "date": {"uts": "1760013000"}
            }]}
        }"##;
        assert_eq!(
            tracks_of(scrobble(body)),
            vec![Track {
                artist: "Princess Goes".to_string(),
                title: "Eat an Eraser".to_string()
            }]
        );
    }

    #[test]
    fn a_lone_artist_arrives_as_an_object() {
        // Why `OneOrMany` exists: with one item Last.fm drops the array.
        let body = r##"{
            "topartists": {
                "artist": {"name": "Chappell Roan", "playcount": "19"},
                "@attr": {"page": "1", "perPage": "50", "totalPages": "1", "total": "1"}
            }
        }"##;
        assert_eq!(artists(body, "topartists"), vec!["Chappell Roan"]);
        assert!(serde_json::from_str::<Vec<ArtistItem>>(r##"{"name": "Chappell Roan"}"##).is_err());
    }

    #[test]
    fn a_lone_similar_artist_arrives_as_an_object() {
        let body = r##"{
            "similarartists": {"artist": {"name": "Fletcher"}, "@attr": {"artist": "Reneé Rapp"}}
        }"##;
        assert_eq!(artists(body, "similarartists"), vec!["Fletcher"]);
    }

    #[test]
    fn a_lone_top_track_arrives_as_an_object() {
        let body = r##"{
            "toptracks": {"track": {"name": "Snow Angel", "artist": {"name": "Reneé Rapp"}}}
        }"##;
        let items = tracks(body, "toptracks");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Snow Angel");
        assert!(serde_json::from_str::<Vec<TrackItem>>(r##"{"name": "Snow Angel"}"##).is_err());
    }

    #[test]
    fn a_lone_scrobble_arrives_as_an_object() {
        let body = r##"{
            "recenttracks": {
                "track": {"artist": {"#text": "Queen"}, "name": "Misfire", "date": {"uts": "1760013000"}}
            }
        }"##;
        let listens = scrobble(body);
        assert_eq!(listens.len(), 1);
        assert_eq!(listens[0].title, "Misfire");
    }

    #[test]
    fn a_last_fm_refusal_names_last_fm() {
        let error = refused(Some(6), "User not found", reqwest::StatusCode::OK, "")
            .expect_err("an error body is an error");
        assert!(error.to_string().contains("Last.fm"));
        assert!(error.to_string().contains("User not found"));

        let error = refused(
            None,
            "no message",
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "slow down",
        )
        .expect_err("a non-success status is an error");
        assert!(error.to_string().contains("Last.fm"));
        assert!(error.to_string().contains("slow down"));
    }
}
