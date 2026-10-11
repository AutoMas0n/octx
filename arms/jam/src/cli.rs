//! Command-line surface.
//!
//! The sinks are separate enums per direction, so a sink that does not belong
//! to a subcommand is not merely rejected — it cannot be written down. clap
//! resolves that, and `--output` is required with no default, so a bare
//! invocation never silently writes anywhere.

use clap::{Parser, Subcommand, ValueEnum};

/// Turn ListenBrainz recommendations into a playlist document, and feed
/// listens back into ListenBrainz.
#[derive(Debug, Parser)]
#[command(name = "jam", version, about, long_about = None)]
pub struct Cli {
    /// Account to speak as. Defaults to the document's `default`, or the sole
    /// account when the document holds exactly one.
    #[arg(long, global = true)]
    pub account: Option<String>,

    /// User name for the source that needs one, instead of the account's.
    #[arg(long, global = true)]
    pub user: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fetch a user's collaborative-filtering recording recommendations
    #[command(override_usage = "jam recs --output <json|qobuz> [OPTIONS]")]
    Recs {
        /// How many recordings to ask for (default 25, maximum 1000)
        #[arg(long)]
        count: Option<usize>,

        /// Where the playlist document goes
        #[arg(long, value_enum)]
        output: PlaylistSink,

        /// Target playlist name or id (qobuz sink)
        #[arg(long)]
        playlist: Option<String>,

        /// Report the changes that would be made without making them
        #[arg(long)]
        dry_run: bool,
    },

    /// Fetch an LB Radio playlist for a prompt
    #[command(
        override_usage = "jam radio --prompt <PROMPT> --mode <easy|medium|hard> --output <json|qobuz> [OPTIONS]"
    )]
    Radio {
        /// ListenBrainz prompt language, for example "artist:(queen)"
        #[arg(long)]
        prompt: String,

        /// Radio mode: how adventurous the picks are
        #[arg(long, value_enum)]
        mode: Mode,

        /// Where the playlist document goes
        #[arg(long, value_enum)]
        output: PlaylistSink,

        /// Target playlist name or id (qobuz sink)
        #[arg(long)]
        playlist: Option<String>,

        /// Report the changes that would be made without making them
        #[arg(long)]
        dry_run: bool,
    },

    /// Read listens from a source and deliver them to a sink
    #[command(override_usage = "jam scrobble --output <json|listenbrainz> [OPTIONS]")]
    Scrobble {
        /// Where the listens come from
        #[arg(long, value_enum, default_value_t = ListenSource::Ndjson)]
        source: ListenSource,

        /// Input file for the ndjson source, or - for standard input
        #[arg(long)]
        input: Option<String>,

        /// How many listens to fetch from a networked source
        #[arg(long)]
        limit: Option<usize>,

        /// Where the listen document goes
        #[arg(long, value_enum)]
        output: ListenSink,
    },

    /// Report whether a `recs` run could succeed
    Status,
}

/// Where a playlist document goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PlaylistSink {
    /// The normalized document on stdout
    Json,
    /// Resolved and synced into a Qobuz playlist
    Qobuz,
}

/// Where a listen document goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ListenSink {
    /// The normalized document on stdout
    Json,
    /// Submitted to ListenBrainz
    Listenbrainz,
}

/// Where listens are read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ListenSource {
    /// NDJSON from `--input`
    Ndjson,
    /// The user's Last.fm scrobbles
    Lastfm,
    /// The user's listens as ListenBrainz holds them
    Listenbrainz,
}

impl ListenSource {
    /// Whether this source reads `--input` rather than the network.
    #[must_use]
    pub fn is_local(self) -> bool {
        matches!(self, ListenSource::Ndjson)
    }

    /// Name as it appears in messages and provenance.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ListenSource::Ndjson => "ndjson",
            ListenSource::Lastfm => "lastfm",
            ListenSource::Listenbrainz => "listenbrainz",
        }
    }
}

/// How adventurous LB Radio should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Mode {
    /// Familiar picks
    Easy,
    /// A middle setting
    Medium,
    /// Adventurous picks
    Hard,
}

impl Mode {
    /// The value the endpoint expects.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Easy => "easy",
            Mode::Medium => "medium",
            Mode::Hard => "hard",
        }
    }
}
