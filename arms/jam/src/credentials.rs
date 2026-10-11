//! The account document jam expects at credential host `jam`.
//!
//! The store is a flat host → value map, so jam reads exactly one host and
//! interprets its value itself: the store neither parses nor validates this
//! document, and no credential value is ever printed. Failures name a path
//! into the document (`accounts.wife.listenbrainz.token`) and the command that
//! fixes it, never the value.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use octx_creds::Store;
use serde::Deserialize;

/// The one credential host this arm reads.
pub const HOST: &str = "jam";

/// The document stored at [`HOST`].
#[derive(Debug, Deserialize)]
pub struct Document {
    /// Shape version; the arm reads [`DOCUMENT_VERSION`].
    pub version: u32,
    /// The account a run speaks as when `--account` is omitted.
    #[serde(default)]
    pub default: Option<String>,
    /// Fields shared by every account: one API key reads any public scrobbles.
    #[serde(default)]
    pub lastfm: Option<LastfmApp>,
    /// The accounts, by name.
    pub accounts: BTreeMap<String, Account>,
}

/// The version of the document shape this build reads.
pub const DOCUMENT_VERSION: u32 = 1;

/// Application-level Last.fm fields.
#[derive(Debug, Default, Deserialize)]
pub struct LastfmApp {
    /// The Last.fm API key. Belongs to the application, not to a person.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// One person's services. Only the fields that person's configurations need.
#[derive(Debug, Default, Deserialize)]
pub struct Account {
    /// Last.fm fields.
    #[serde(default)]
    pub lastfm: Option<LastfmAccount>,
    /// ListenBrainz fields.
    #[serde(default)]
    pub listenbrainz: Option<ListenBrainzAccount>,
    /// Qobuz fields.
    #[serde(default)]
    pub qobuz: Option<QobuzAccount>,
}

/// A person's Last.fm fields.
#[derive(Debug, Default, Deserialize)]
pub struct LastfmAccount {
    /// The Last.fm user name.
    #[serde(default)]
    pub user: Option<String>,
}

/// A person's ListenBrainz fields.
#[derive(Debug, Default, Deserialize)]
pub struct ListenBrainzAccount {
    /// The ListenBrainz user name.
    #[serde(default)]
    pub user: Option<String>,
    /// The ListenBrainz token.
    #[serde(default)]
    pub token: Option<String>,
}

/// A person's Qobuz fields.
#[derive(Debug, Default, Deserialize)]
pub struct QobuzAccount {
    /// The Qobuz user authentication token.
    #[serde(default)]
    pub user_auth_token: Option<String>,
    /// Overrides the application id the arm ships with.
    #[serde(default)]
    pub app_id: Option<String>,
}

/// The document, plus the one account a run speaks as.
pub struct Selected {
    /// The account's name.
    pub name: String,
    /// The account's fields.
    pub account: Account,
    /// The shared Last.fm API key, when the document carries one.
    pub lastfm_api_key: Option<String>,
}

/// A `Selected` prints nothing it holds.
///
/// Debugging output is exactly the kind of place a credential escapes, so the
/// redaction is part of the type rather than a discipline the call sites have
/// to keep.
impl std::fmt::Debug for Selected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Selected")
            .field("name", &self.name)
            .field("account", &"<redacted>")
            .field("lastfm_api_key", &"<redacted>")
            .finish()
    }
}

/// Read the document and select one account.
///
/// Callers reach for this only when the run needs a field from it: a public
/// ListenBrainz read with an explicit `--user` never touches the store.
pub fn load_selected(store: &Store, requested: Option<&str>) -> Result<Selected> {
    let secret = store.get(HOST)?;
    let document = parse(secret.expose())?;
    select(document, requested)
}

/// Parse the document, reporting shape problems without echoing the value.
fn parse(raw: &str) -> Result<Document> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
        anyhow::anyhow!(
            "the credential stored for host `{HOST}` is not valid JSON \
             ({}, at line {} column {})\n  replace it with `octx creds add {HOST}`",
            json_error_kind(&error),
            error.line(),
            error.column()
        )
    })?;

    let document: Document = serde_json::from_value(value).map_err(|error| {
        anyhow::anyhow!(
            "the credential stored for host `{HOST}` is not a jam account document ({})\n  \
             expected: {{\"version\":{DOCUMENT_VERSION},\"default\":\"<name>\",\
             \"lastfm\":{{\"api_key\":\"<key>\"}},\"accounts\":{{\"<name>\":{{…}}}}}}\n  \
             replace it with `octx creds add {HOST}`",
            sanitize(&error.to_string())
        )
    })?;

    if document.version != DOCUMENT_VERSION {
        bail!(
            "the credential stored for host `{HOST}` declares document version {}, \
             and this build reads version {DOCUMENT_VERSION}\n  \
             update to a jam that reads it, or rewrite it with `octx creds add {HOST}`",
            document.version
        );
    }
    Ok(document)
}

/// Select the account a run speaks as: the named one, else the document's
/// default, else the sole account. Several candidates without a default is an
/// error that lists them rather than a guess, and there is no fallback between
/// accounts — a run speaks for exactly one.
fn select(document: Document, requested: Option<&str>) -> Result<Selected> {
    let Document {
        accounts,
        default,
        lastfm,
        ..
    } = document;

    if accounts.is_empty() {
        bail!(
            "the credential document at host `{HOST}` lists no accounts\n  \
             add one with `octx creds add {HOST}`"
        );
    }

    let name = match requested {
        Some(name) => {
            if !accounts.contains_key(name) {
                bail!(
                    "no account `{name}` in the credential document at host `{HOST}`\n  \
                     accounts present: {}\n  pass --account with one of those names",
                    names(&accounts)
                );
            }
            name.to_string()
        }
        None => match default {
            Some(name) => {
                if !accounts.contains_key(&name) {
                    bail!(
                        "the credential document at host `{HOST}` names `{name}` as its \
                         default account, and no such account exists\n  accounts present: {}",
                        names(&accounts)
                    );
                }
                name
            }
            None => {
                let mut keys = accounts.keys();
                match (keys.next(), keys.next()) {
                    (Some(only), None) => only.clone(),
                    _ => bail!(
                        "the credential document at host `{HOST}` holds {} accounts and no \
                         `default`\n  accounts present: {}\n  pass --account <name>",
                        accounts.len(),
                        names(&accounts)
                    ),
                }
            }
        },
    };

    let account = accounts
        .into_iter()
        .find(|(key, _)| key == &name)
        .map(|(_, account)| account)
        .ok_or_else(|| anyhow::anyhow!("no account `{name}`"))?;

    Ok(Selected {
        name,
        account,
        lastfm_api_key: lastfm.and_then(|app| app.api_key),
    })
}

impl Selected {
    /// The account's ListenBrainz user name, when it records one.
    pub fn listenbrainz_user(&self) -> Option<&str> {
        self.account
            .listenbrainz
            .as_ref()
            .and_then(|lb| lb.user.as_deref())
            .filter(|user| !user.is_empty())
    }

    /// The account's ListenBrainz token, when it carries one.
    pub fn listenbrainz_token(&self) -> Option<&str> {
        self.account
            .listenbrainz
            .as_ref()
            .and_then(|lb| lb.token.as_deref())
            .filter(|token| !token.is_empty())
    }

    /// The account's Last.fm user name, when it records one.
    pub fn lastfm_user(&self) -> Option<&str> {
        self.account
            .lastfm
            .as_ref()
            .and_then(|lastfm| lastfm.user.as_deref())
            .filter(|user| !user.is_empty())
    }

    /// The account's Qobuz token, when it carries one.
    pub fn qobuz_token(&self) -> Option<&str> {
        self.account
            .qobuz
            .as_ref()
            .and_then(|qobuz| qobuz.user_auth_token.as_deref())
            .filter(|token| !token.is_empty())
    }

    /// The account's Qobuz application id override, when it carries one.
    pub fn qobuz_app_id(&self) -> Option<&str> {
        self.account
            .qobuz
            .as_ref()
            .and_then(|qobuz| qobuz.app_id.as_deref())
            .filter(|id| !id.is_empty())
    }

    /// The path of an account field, for an error message.
    #[must_use]
    pub fn path(&self, field: &str) -> String {
        format!("accounts.{}.{field}", self.name)
    }

    /// Require an account field, naming its path when it is absent.
    pub fn require<'a>(&'a self, value: Option<&'a str>, field: &str) -> Result<&'a str> {
        value.ok_or_else(|| missing(&self.path(field)))
    }
}

/// The error for an absent field: its path, and the command that supplies it.
pub fn missing(path: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "the credential document at host `{HOST}` has no `{path}`\n  \
         add it with `octx creds add {HOST}`"
    )
}

/// The accounts present, for an error message.
fn names(accounts: &BTreeMap<String, Account>) -> String {
    accounts.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// Classify a JSON syntax error without repeating any of the input.
fn json_error_kind(error: &serde_json::Error) -> &'static str {
    use serde_json::error::Category;
    match error.classify() {
        Category::Io => "could not be read",
        Category::Syntax => "a syntax error",
        Category::Data => "the wrong data shape",
        Category::Eof => "truncated input",
    }
}

/// Remove every quoted literal from a serde message.
///
/// serde reports a type mismatch by quoting the value it found, and the value
/// it found is the credential document — so the quote is removed before the
/// message can be printed. The shape of the message survives.
fn sanitize(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut in_quotes = false;
    for ch in message.chars() {
        match ch {
            '"' if in_quotes => in_quotes = false,
            '"' => {
                in_quotes = true;
                out.push('…');
            }
            _ if !in_quotes => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const TWO_ACCOUNTS: &str = r#"{
        "version": 1,
        "default": "jesse",
        "lastfm": { "api_key": "shared-app-key" },
        "accounts": {
            "jesse": {
                "lastfm": { "user": "jesse1819" },
                "listenbrainz": { "user": "automason", "token": "lb-jesse-token" },
                "qobuz": { "user_auth_token": "qz-jesse-token" }
            },
            "wife": {
                "lastfm": { "user": "wife1819" },
                "listenbrainz": { "user": "wife-lb", "token": "lb-wife-token" }
            }
        }
    }"#;

    fn store_with(dir: &Path, value: &str) -> Store {
        let store = Store::open(dir, "machine-a");
        store.add(HOST, value).expect("add jam document");
        store
    }

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    #[test]
    fn named_account_is_used() {
        let dir = tempdir();
        let store = store_with(dir.path(), TWO_ACCOUNTS);
        let selected = load_selected(&store, Some("wife")).expect("select");
        assert_eq!(selected.name, "wife");
        assert_eq!(
            selected
                .account
                .listenbrainz
                .as_ref()
                .unwrap()
                .user
                .as_deref(),
            Some("wife-lb")
        );
        assert_eq!(selected.lastfm_api_key.as_deref(), Some("shared-app-key"));
    }

    #[test]
    fn document_default_is_used_when_no_account_is_given() {
        let dir = tempdir();
        let store = store_with(dir.path(), TWO_ACCOUNTS);
        let selected = load_selected(&store, None).expect("select");
        assert_eq!(selected.name, "jesse");
    }

    #[test]
    fn a_sole_account_needs_no_default() {
        let dir = tempdir();
        let store = store_with(
            dir.path(),
            r#"{"version":1,"accounts":{"solo":{"lastfm":{"user":"solo"}}}}"#,
        );
        let selected = load_selected(&store, None).expect("select");
        assert_eq!(selected.name, "solo");
    }

    #[test]
    fn several_accounts_and_no_default_lists_them() {
        let dir = tempdir();
        let store = store_with(
            dir.path(),
            r#"{"version":1,"accounts":{"jesse":{},"wife":{}}}"#,
        );
        let error = load_selected(&store, None).expect_err("must fail");
        let text = error.to_string();
        assert!(
            text.contains("jesse, wife") || text.contains("wife, jesse"),
            "{text}"
        );
        assert!(text.contains("--account <name>"), "{text}");
    }

    #[test]
    fn an_unknown_account_lists_the_ones_present() {
        let dir = tempdir();
        let store = store_with(dir.path(), TWO_ACCOUNTS);
        let error = load_selected(&store, Some("ghost")).expect_err("must fail");
        let text = error.to_string();
        assert!(text.contains("ghost"), "{text}");
        assert!(text.contains("jesse, wife"), "{text}");
    }

    #[test]
    fn an_absent_document_names_the_host_and_the_command() {
        let dir = tempdir();
        let store = Store::open(dir.path(), "machine-a");
        let error = load_selected(&store, None).expect_err("must fail");
        let text = format!("{error:#}");
        assert!(text.contains("jam"), "{text}");
        assert!(text.contains("octx creds add jam"), "{text}");
    }

    #[test]
    fn a_malformed_document_is_reported_without_being_echoed() {
        let dir = tempdir();
        let store = store_with(dir.path(), "not json at all");
        let error = load_selected(&store, None).expect_err("must fail");
        let text = format!("{error:#}");
        assert!(text.contains("not valid JSON"), "{text}");
        assert!(text.contains("octx creds add jam"), "{text}");
        assert!(!text.contains("not json at all"), "value echoed: {text}");
    }

    #[test]
    fn a_wrong_shaped_document_never_echoes_a_secret() {
        let dir = tempdir();
        // A bare string is valid JSON and the wrong shape; serde quotes it.
        let store = store_with(dir.path(), r#""lb-super-secret-token""#);
        let error = load_selected(&store, None).expect_err("must fail");
        let text = format!("{error:#}");
        assert!(text.contains("not a jam account document"), "{text}");
        assert!(
            !text.contains("lb-super-secret-token"),
            "value echoed: {text}"
        );
    }

    #[test]
    fn an_unknown_document_version_is_refused() {
        let dir = tempdir();
        let store = store_with(dir.path(), r#"{"version":99,"accounts":{"a":{}}}"#);
        let error = load_selected(&store, None).expect_err("must fail");
        assert!(error.to_string().contains("version 99"), "{error}");
    }

    #[test]
    fn an_absent_field_names_its_path() {
        let dir = tempdir();
        let store = store_with(dir.path(), TWO_ACCOUNTS);
        let selected = load_selected(&store, Some("wife")).expect("select");
        let account = selected.account.qobuz.as_ref();
        let token = account.and_then(|q| q.user_auth_token.as_deref());
        let error = selected
            .require(token, "qobuz.user_auth_token")
            .expect_err("must fail");
        let text = error.to_string();
        assert!(
            text.contains("accounts.wife.qobuz.user_auth_token"),
            "{text}"
        );
        assert!(text.contains("octx creds add jam"), "{text}");
    }

    #[test]
    fn a_selected_account_never_prints_its_values() {
        let dir = tempdir();
        let store = store_with(dir.path(), TWO_ACCOUNTS);
        let selected = load_selected(&store, Some("wife")).expect("select");
        let printed = format!("{selected:?}");
        assert!(printed.contains("wife"), "{printed}");
        for secret in ["lb-wife-token", "shared-app-key"] {
            assert!(!printed.contains(secret), "{secret} leaked: {printed}");
        }
    }

    #[test]
    fn sanitize_removes_quoted_values_but_keeps_the_shape() {
        let message =
            r#"invalid type: string "secret-value", expected struct Document at line 1 column 20"#;
        let clean = sanitize(message);
        assert!(!clean.contains("secret-value"), "{clean}");
        assert!(clean.contains("invalid type: string"), "{clean}");
        assert!(clean.contains("expected struct Document"), "{clean}");
    }
}
