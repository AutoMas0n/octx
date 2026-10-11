//! Orchestration: subcommand → source → document → sink.
//!
//! Two rules shape the control flow here. A credential is read only when a run
//! actually needs a field from the document, so a public ListenBrainz read with
//! an explicit `--user` never touches the store. And a source with nothing to
//! give is a failure on the read side — it must never reach a sink, because a
//! sink's job is to converge and converging on nothing empties the playlist.

use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result, bail};
use octx_creds::{Store, machine_id};

use crate::cli::{Cli, Command, ListenSink, ListenSource, Mode, PlaylistSink};
use crate::config::Config;
use crate::credentials::{self, Selected};
use crate::document::{ListenDocument, PlaylistDocument, Provenance};
use crate::http::Http;
use crate::{lastfm, lb, qobuz, scrobble, sink, sync};

/// Run one invocation.
pub async fn run(cli: Cli, cfg: Config) -> Result<()> {
    let http = Http::new(cfg.clone())?;
    let store = open_store(&cfg)?;
    let Cli {
        account,
        user,
        command,
    } = cli;

    let context = Ctx {
        http: &http,
        store: &store,
        account: account.as_deref(),
        user: user.as_deref(),
    };

    match command {
        Command::Recs {
            count,
            output,
            playlist,
            dry_run,
        } => {
            let document = context.recs_document(count).await?;
            context
                .deliver_playlist(document, output, playlist.as_deref(), dry_run)
                .await
        }
        Command::Radio {
            prompt,
            mode,
            output,
            playlist,
            dry_run,
        } => {
            let document = context.radio_document(&prompt, mode).await?;
            context
                .deliver_playlist(document, output, playlist.as_deref(), dry_run)
                .await
        }
        Command::Scrobble {
            source,
            input,
            limit,
            output,
        } => {
            context
                .scrobble(source, input.as_deref(), limit, output)
                .await
        }
        Command::Status => context.status().await,
    }
}

/// Open the credential store, honouring the test hook when one was set.
fn open_store(cfg: &Config) -> Result<Store> {
    match &cfg.store_dir {
        Some(dir) => {
            let identity = machine_id().context("could not determine this machine's identity")?;
            Ok(Store::open(dir, identity))
        }
        None => Store::open_default().context("could not open the credential store"),
    }
}

/// What every subcommand needs to reach the outside world.
struct Ctx<'a> {
    http: &'a Http,
    store: &'a Store,
    account: Option<&'a str>,
    user: Option<&'a str>,
}

impl Ctx<'_> {
    /// The credential document and its selected account, loaded once.
    ///
    /// Reached only by a run that needs a field: a public read with `--user`
    /// and no sink credential never calls it.
    fn selected<'a>(&self, slot: &'a mut Option<Selected>) -> Result<&'a Selected> {
        if slot.is_none() {
            *slot = Some(credentials::load_selected(self.store, self.account)?);
        }
        slot.as_ref().ok_or_else(|| {
            anyhow::anyhow!("internal error: the credential document was not loaded")
        })
    }

    /// The user name a listen identity is derived from, when one is known
    /// without another request.
    ///
    /// `--user` wins; otherwise the selected account's ListenBrainz user. A
    /// token is never spent on `validate-token` just to name an identity.
    fn identity_user(&self, slot: &Option<Selected>) -> String {
        self.user
            .map(str::to_string)
            .or_else(|| {
                slot.as_ref()
                    .and_then(Selected::listenbrainz_user)
                    .map(str::to_string)
            })
            .unwrap_or_default()
    }

    /// The ListenBrainz user name for a run: the recorded name, else derived
    /// from that account's token. Reading recommendations is public, so the
    /// token's only job here is naming the user when the document does not.
    async fn listenbrainz_user(&self, selected: &Selected) -> Result<String> {
        if let Some(user) = selected.listenbrainz_user() {
            return Ok(user.to_string());
        }
        let Some(token) = selected.listenbrainz_token() else {
            bail!(
                "the credential document at host `jam` carries no ListenBrainz account name for \
                 account `{name}`\n  it needs either `{user_path}` or `{token_path}`, to derive the \
                 name from\n  add one with `octx creds add jam`",
                name = selected.name,
                user_path = selected.path("listenbrainz.user"),
                token_path = selected.path("listenbrainz.token"),
            );
        };
        lb::validate_token(self.http, token).await
    }

    /// The read side: a source, normalized into a playlist document.
    async fn recs_document(&self, count: Option<usize>) -> Result<PlaylistDocument> {
        let mut slot = None;
        // A public read with `--user` needs no credential at all.
        let user = match self.user {
            Some(user) => user.to_string(),
            None => {
                let selected = self.selected(&mut slot)?;
                self.listenbrainz_user(selected).await?
            }
        };

        match lb::recs(self.http, &user, count).await? {
            lb::Recs::Ready {
                recommendations,
                model_id,
                last_updated,
                total_mbid_count,
            } => {
                if recommendations.is_empty() {
                    bail!("{}", no_model(&user));
                }
                let mbids: Vec<String> = recommendations
                    .iter()
                    .map(|item| item.recording_mbid.clone())
                    .collect();
                let mut entries = lb::expand(self.http, &mbids).await?;

                // Carry the ranking, and keep the playlist in it: the
                // recommendation order is the order the sink writes.
                let ranking: HashMap<&str, (Option<f64>, Option<i64>)> = recommendations
                    .iter()
                    .map(|item| {
                        (
                            item.recording_mbid.as_str(),
                            (item.score, item.latest_listened_at),
                        )
                    })
                    .collect();
                for entry in &mut entries {
                    if let Some((score, latest)) = ranking.get(entry.recording_mbid.as_str()) {
                        entry.score = *score;
                        entry.latest_listened_at = *latest;
                    }
                }
                let rank: HashMap<&str, usize> = recommendations
                    .iter()
                    .enumerate()
                    .map(|(index, item)| (item.recording_mbid.as_str(), index))
                    .collect();
                entries.sort_by_key(|entry| {
                    rank.get(entry.recording_mbid.as_str())
                        .copied()
                        .unwrap_or(usize::MAX)
                });

                Ok(PlaylistDocument::new(
                    Provenance {
                        source: "recs".to_string(),
                        user: Some(user),
                        model_id,
                        last_updated,
                        total_mbid_count,
                        prompt: None,
                        mode: None,
                    },
                    entries,
                ))
            }
            lb::Recs::NoModel { .. } => bail!("{}", no_model(&user)),
            lb::Recs::UnknownUser => bail!(
                "ListenBrainz does not know the user `{user}`\n  check the ListenBrainz account \
                 name in the credential document (`{user}`) and in `--user`"
            ),
        }
    }

    /// The read side, from LB Radio.
    async fn radio_document(&self, prompt: &str, mode: Mode) -> Result<PlaylistDocument> {
        let mut slot = None;
        let selected = self.selected(&mut slot)?;
        // LB Radio is the one read that genuinely needs a token.
        let token = selected
            .require(selected.listenbrainz_token(), "listenbrainz.token")?
            .to_string();

        let entries = lb::radio(self.http, &token, prompt, mode.as_str()).await?;
        if entries.is_empty() {
            bail!(
                "LB Radio returned no recordings for that prompt and mode, so there is nothing to \
                 write\n  the prompt may match nothing in ListenBrainz's data; nothing was written"
            );
        }

        Ok(PlaylistDocument::new(
            Provenance {
                source: "radio".to_string(),
                user: selected.listenbrainz_user().map(str::to_string),
                prompt: Some(prompt.to_string()),
                mode: Some(mode.as_str().to_string()),
                ..Provenance::default()
            },
            entries,
        ))
    }

    /// The readiness check, so a failure can be avoided rather than reported.
    async fn status(&self) -> Result<()> {
        let mut slot = None;
        let selected = self.selected(&mut slot)?;
        let user = self.listenbrainz_user(selected).await?;

        println!("credential: ok (host `jam`, account `{}`)", selected.name);
        println!("user: {user}");

        match lb::recs(self.http, &user, Some(1)).await? {
            lb::Recs::Ready {
                last_updated,
                total_mbid_count,
                ..
            } => {
                println!(
                    "model: ready (trained {}, {} recording{})",
                    format_time(last_updated),
                    total_mbid_count
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "an unknown number of".to_string()),
                    if total_mbid_count == Some(1) { "" } else { "s" }
                );
                Ok(())
            }
            lb::Recs::NoModel { .. } => {
                println!("model: none");
                bail!("{}", no_model(&user))
            }
            lb::Recs::UnknownUser => bail!("ListenBrainz does not know the user `{user}`"),
        }
    }

    /// The write side: listens from a source, into a listen document, out.
    async fn scrobble(
        &self,
        source: ListenSource,
        input: Option<&str>,
        limit: Option<usize>,
        output: ListenSink,
    ) -> Result<()> {
        // Argument rules first: nothing is read or fetched before they hold.
        match (source, input) {
            (ListenSource::Ndjson, None) => bail!(
                "`jam scrobble --source ndjson` reads `--input <path>`, or `--input -` for \
                 standard input"
            ),
            (source, Some(_)) if !source.is_local() => bail!(
                "`--input` is only read by the `ndjson` source, and `--source {}` fetches its \
                 listens instead",
                source.as_str()
            ),
            _ => {}
        }

        let mut slot = None;

        // The sink's credential, before anything is read or fetched.
        let token = match output {
            ListenSink::Listenbrainz => {
                let selected = self.selected(&mut slot)?;
                Some(
                    selected
                        .require(selected.listenbrainz_token(), "listenbrainz.token")?
                        .to_string(),
                )
            }
            ListenSink::Json => None,
        };

        let mut rejections = Vec::new();
        let (listens, user, attribution) = match source {
            ListenSource::Ndjson => {
                let text =
                    read_input(input.ok_or_else(|| anyhow::anyhow!("`--input` is required"))?)?;
                let parsed = scrobble::parse_ndjson(&text, &self.identity_user(&slot));
                rejections = parsed.rejections;
                (parsed.listens, self.user.map(str::to_string), None)
            }
            ListenSource::Lastfm => {
                let selected = self.selected(&mut slot)?;
                let api_key = selected
                    .lastfm_api_key
                    .clone()
                    .ok_or_else(|| credentials::missing("lastfm.api_key"))?;
                let user = match self.user {
                    Some(user) => user.to_string(),
                    None => selected
                        .require(selected.lastfm_user(), "lastfm.user")?
                        .to_string(),
                };
                let identity_user = self.identity_user(&slot);
                let limit = limit.unwrap_or(lastfm::DEFAULT_LIMIT);
                let fetched =
                    lastfm::recent_tracks(self.http, &api_key, &user, limit, &identity_user)
                        .await?;
                eprintln!("jam: {}", fetched.attribution.line());
                (
                    fetched.listens,
                    Some(fetched.user),
                    Some(fetched.attribution),
                )
            }
            ListenSource::Listenbrainz => {
                let user = match self.user {
                    Some(user) => user.to_string(),
                    None => {
                        let selected = self.selected(&mut slot)?;
                        self.listenbrainz_user(selected).await?
                    }
                };
                let listens = lb::user_listens(self.http, &user, limit).await?;
                (listens, Some(user), None)
            }
        };

        if listens.is_empty() {
            if !rejections.is_empty() {
                bail!(
                    "nothing was delivered: every line of the input was rejected\n{}",
                    scrobble::Parsed {
                        listens: Vec::new(),
                        rejections,
                    }
                    .report()
                );
            }
            // Nothing to submit is the normal state of a quiet run, unlike an
            // absent recommendation model.
            eprintln!(
                "jam: nothing to submit — {} yielded no listens",
                source.as_str()
            );
            return Ok(());
        }

        let mut document = ListenDocument::new(source.as_str(), user, listens);
        if let Some(attribution) = attribution {
            document = document.with_attribution(attribution);
        }
        let rejected = rejections.len();
        if !rejections.is_empty() {
            eprintln!(
                "jam: {rejected} input line{} rejected\n{}",
                if rejected == 1 { "" } else { "s" },
                scrobble::Parsed {
                    listens: Vec::new(),
                    rejections,
                }
                .report()
            );
        }

        match token {
            Some(token) => sink::submit(self.http, &token, &document, rejected).await?,
            None => sink::write_listens(&document)?,
        }

        // Valid lines were delivered; the exit status still reports the loss.
        if rejected > 0 {
            bail!(
                "{rejected} input line{} rejected; the valid listens were still delivered",
                if rejected == 1 { "" } else { "s" }
            );
        }
        Ok(())
    }

    /// Hand a playlist document to the sink the caller selected.
    async fn deliver_playlist(
        &self,
        document: PlaylistDocument,
        sink: PlaylistSink,
        requested: Option<&str>,
        dry_run: bool,
    ) -> Result<()> {
        match sink {
            PlaylistSink::Json => sink::write_playlist(&document),
            PlaylistSink::Qobuz => {
                // The sink's credential is read before the first Qobuz request.
                let mut slot = None;
                let selected = self.selected(&mut slot)?;
                let token = selected
                    .require(selected.qobuz_token(), "qobuz.user_auth_token")?
                    .to_string();
                let app_id = selected.qobuz_app_id().map(str::to_string);
                let client = qobuz::Client::new(self.http, &token, app_id.as_deref());
                let summary = sync::sync(
                    &client,
                    &document.entries,
                    requested,
                    &document.source.source,
                    dry_run,
                )
                .await?;
                eprintln!("jam: {}", summary.line());
                Ok(())
            }
        }
    }
}

/// The message for a source that has nothing to give.
fn no_model(user: &str) -> String {
    format!(
        "ListenBrainz holds no recommendations for `{user}`\n  the collaborative-filtering model \
         is trained from listens ListenBrainz has received, so it has nothing to recommend until \
         something has scrobbled some\n  nothing was written — run `jam status` to check again, \
         and `jam scrobble` to send listens you already have"
    )
}

/// Read the NDJSON input, from a file or standard input.
fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut buffer = String::new();
        std::io::stdin()
            .read_to_string(&mut buffer)
            .context("could not read standard input")?;
        Ok(buffer)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("could not read {path}"))
    }
}

/// Format a UNIX timestamp for the status report.
fn format_time(seconds: Option<i64>) -> String {
    match seconds.and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0)) {
        Some(when) => when.format("%Y-%m-%d %H:%M UTC").to_string(),
        None => "at an unknown time".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_no_model_message_says_what_to_do() {
        let message = no_model("automason");
        assert!(message.contains("automason"), "{message}");
        assert!(message.contains("jam status"), "{message}");
        assert!(message.contains("jam scrobble"), "{message}");
        assert!(message.contains("nothing was written"), "{message}");
    }

    #[test]
    fn timestamps_are_formatted_or_explained() {
        assert_eq!(format_time(Some(0)), "1970-01-01 00:00 UTC");
        assert_eq!(format_time(None), "at an unknown time");
    }
}
