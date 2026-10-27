use std::path::PathBuf;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "aidhd", version, about = "One-shot task management arm")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Add a task. Use `-` as the title to read it from stdin.
    Add {
        title: String,
        #[arg(long, value_parser = ["high", "medium", "low"])]
        priority: Option<String>,
        #[arg(long)]
        category: Option<String>,
        /// Accepts YYYY-MM-DD or a natural date such as `tomorrow` or `friday`.
        #[arg(long)]
        due: Option<String>,
        #[arg(long, value_parser = ["daily", "weekly", "monthly"])]
        repeat: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,
        /// Emit the task and scheduling fields as one JSON object.
        #[arg(long)]
        json: bool,
        /// Leave the due date empty instead of auto-scheduling it.
        #[arg(long)]
        unscheduled: bool,
    },
    /// List tasks. Defaults to pending tasks.
    List {
        #[arg(long, value_parser = ["pending", "completed", "cancelled"])]
        status: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        /// List cancelled tasks (undone subtasks stay reachable).
        #[arg(long)]
        cancelled: bool,
        #[arg(long)]
        json: bool,
    },
    /// Edit a task's fields.
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, value_parser = ["high", "medium", "low"])]
        priority: Option<String>,
        #[arg(long)]
        category: Option<String>,
        /// Accepts YYYY-MM-DD or a natural date such as `tomorrow` or `friday`.
        #[arg(long)]
        due: Option<String>,
    },
    /// Reopen a completed or cancelled task.
    Reopen { id: String },
    /// Mark a task complete.
    Done { id: String },
    /// Mark a task fixed so the secretary never moves it.
    Pin { id: String },
    /// Let the secretary move a task again.
    Unpin { id: String },
    /// Run one secretary pass, preview it, or check scheduler health.
    Secretary {
        /// Print proposed decisions without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Send one notification summarising the pass.
        #[arg(long)]
        notify: bool,
        /// Report the last run and exit non-zero if it is overdue.
        #[arg(long)]
        status: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show the decision log, or prune it.
    Decisions {
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
        #[command(subcommand)]
        command: Option<DecisionsCommand>,
    },
    /// Reverse a decision, restoring the previous due date.
    Undo { target: Option<String> },
    /// Search titles, descriptions, and notes.
    Search { text: String },
    /// Show task counts.
    Stats,
    /// Show what to do today, grouped into overdue, today, and upcoming.
    Today {
        #[arg(long)]
        json: bool,
    },
    /// Show one task's full detail.
    Show { id: String },
    /// Show the most urgent open task as an action prompt.
    Next,
    /// List tasks due within a window and optionally notify.
    Due {
        #[arg(long, default_value = "1d")]
        within: String,
        #[arg(long)]
        notify: bool,
        #[arg(long)]
        force: bool,
    },
    /// Export all tasks.
    Export {
        #[arg(long, value_parser = ["json", "csv"])]
        format: String,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Capture one voice utterance and save it as a task.
    Voice,
    /// Ask a question grounded in stored tasks.
    Ask { question: String },
    /// Suggest improvements for a task.
    Suggest { id: String },
    /// Serve the dashboard and JSON API on loopback only.
    Serve {
        /// Port to listen on (bound to 127.0.0.1).
        #[arg(long, default_value_t = 7317)]
        port: u16,
    },
}

#[derive(Debug, Subcommand)]
pub enum DecisionsCommand {
    /// Delete old decisions; undone decisions are always kept.
    Prune {
        #[arg(long)]
        before: Option<NaiveDate>,
    },
}
