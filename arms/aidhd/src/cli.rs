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
        #[arg(long)]
        due: Option<NaiveDate>,
        #[arg(long, value_parser = ["daily", "weekly", "monthly"])]
        repeat: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long = "tag", action = clap::ArgAction::Append)]
        tags: Vec<String>,
    },
    /// List tasks. Defaults to pending tasks.
    List {
        #[arg(long, value_parser = ["pending", "completed", "cancelled"])]
        status: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        parent: Option<String>,
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
        #[arg(long)]
        due: Option<NaiveDate>,
    },
    /// Reopen a completed or cancelled task.
    Reopen { id: String },
    /// Mark a task complete.
    Done { id: String },
    /// Search titles, descriptions, and notes.
    Search { text: String },
    /// Show task counts.
    Stats,
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
}
