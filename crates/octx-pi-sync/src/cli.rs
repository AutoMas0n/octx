//! The pi-sync command line.
//!
//! Five verbs, no setup step: `push`, `pull`, `sync`, `apply`, `status`. The
//! blob defaults to `{data_dir}/octx/pi-sync/blob`; an explicit path replaces it
//! for that invocation only. There is deliberately no transport here — the user
//! moves the blob however they like.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::{apply, blob, collect, consolidate, log, paths, record::Record};

/// Carry user-authored pi configuration between devices.
#[derive(Parser)]
#[command(
    name = "pi-sync",
    version,
    about = "Carry your user-authored pi configuration between devices through untrusted storage",
    long_about = "Collects the pi agent directory's authored content (skills, extensions, portable \
settings, AGENTS.md), writes it as one passphrase-encrypted blob, and applies it on \
another device.\n\nThe blob lives at {data_dir}/octx/pi-sync/blob by default; pass a path to use a \
different one for a single invocation. The passphrase is read from the octx credential \
store under the host `pi-sync`; add it once per device with `octx creds add pi-sync`.\n\nThe \
arm never talks to the network. Moving the blob between devices is your transport."
)]
pub struct Cli {
    /// The pi agent directory (default: ~/.pi/agent, or $PI_CODING_AGENT_DIR).
    #[arg(long, global = true, value_name = "DIR")]
    pub agent_dir: Option<PathBuf>,

    /// The blob path (default: {data_dir}/octx/pi-sync/blob).
    #[arg(long, global = true, value_name = "PATH")]
    pub blob: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// The pi-sync verbs.
#[derive(Subcommand)]
pub enum Command {
    /// Record this device's authored configuration into the blob.
    Push,
    /// Read the blob and add its records to this device's log.
    Pull,
    /// Pull, consolidate, and apply in one step.
    Sync {
        /// Show what would change without writing to the pi agent directory.
        #[arg(long)]
        dry_run: bool,
        /// Prefer this device's value when a key exists in several series.
        #[arg(long, value_name = "HOST")]
        prefer: Option<String>,
    },
    /// Apply the log's consolidated view to the pi agent directory.
    Apply {
        /// Prefer this device's value when a key exists in several series.
        #[arg(long, value_name = "HOST")]
        prefer: Option<String>,
    },
    /// Report the log's contents and conflicts; writes nothing.
    Status,
}

/// Resolve the effective blob path.
fn blob_path(explicit: &Option<PathBuf>) -> PathBuf {
    explicit.clone().unwrap_or_else(paths::blob_path)
}

/// Resolve the effective pi agent directory.
fn agent_dir(explicit: &Option<PathBuf>) -> PathBuf {
    explicit.clone().unwrap_or_else(paths::pi_agent_dir)
}

/// Run the CLI and return an exit code.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let agent = agent_dir(&cli.agent_dir);
    let blob = blob_path(&cli.blob);

    match cli.command {
        Command::Push => push(&agent, &blob),
        Command::Pull => {
            pull(&blob)?;
            Ok(())
        }
        Command::Sync { dry_run, prefer } => sync(&agent, &blob, dry_run, prefer.as_deref()),
        Command::Apply { prefer } => {
            let applied = apply_log(&agent, prefer.as_deref())?;
            report(&applied);
            Ok(())
        }
        Command::Status => status(&agent),
    }
}

fn push(agent: &Path, blob_path: &Path) -> Result<()> {
    let host = paths::host_id().context("cannot determine this device's identity")?;
    let existing = log::read(&paths::log_path()).context("cannot read the pi-sync log")?;
    let passphrase = blob::passphrase()?;

    // Read the medium before writing anything local: never clobber what it
    // already holds, and fail before touching the log when the passphrase is
    // wrong. This is what stops a device that pushes before it pulls from
    // dropping another device's work.
    let mut all = existing.clone();
    if blob_path.exists() {
        all = log::union(&all, &read_records(blob_path, &passphrase)?);
    }

    // Collected records are numbered from zero; re-index them against both the
    // local log and the medium's records so this push extends the series
    // instead of reusing positions an earlier push already wrote.
    let fresh = log::reindex(&collect::collect(agent, &host), &all);
    all = log::union(&all, &fresh);

    log::append(&paths::log_path(), &fresh).context("cannot append to the pi-sync log")?;

    let mut body = String::new();
    for record in &all {
        body.push_str(&serde_json::to_string(record)?);
        body.push('\n');
    }

    blob::write(blob_path, &passphrase, body.as_bytes())?;

    println!(
        "recorded {} new records ({} total), wrote {} to {}",
        fresh.len(),
        all.len(),
        human_bytes(body.len()),
        blob_path.display(),
    );
    Ok(())
}

/// Decrypt and parse the records held at `blob_path`.
fn read_records(blob_path: &Path, passphrase: &str) -> Result<Vec<Record>> {
    let plaintext = blob::read(blob_path, passphrase).with_context(|| {
        format!(
            "cannot read the existing blob at {}\n  the passphrase comes from `octx creds pi-sync`",
            blob_path.display()
        )
    })?;

    let body = String::from_utf8(plaintext).context("the decrypted blob is not valid UTF-8")?;
    let mut records = Vec::new();
    for (index, line) in body.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        records.push(
            serde_json::from_str(line)
                .with_context(|| format!("corrupt record at line {}", index + 1))?,
        );
    }
    Ok(records)
}

fn pull(blob_path: &Path) -> Result<Vec<Record>> {
    let passphrase = blob::passphrase()?;
    let incoming = read_records(blob_path, &passphrase)?;

    let existing = log::read(&paths::log_path()).context("cannot read the pi-sync log")?;

    // A position already in the log is not appended again: pull is idempotent.
    let new_records: Vec<Record> = incoming
        .iter()
        .filter(|record| {
            !existing
                .iter()
                .any(|known| log::same_position(known, record))
        })
        .cloned()
        .collect();

    log::append(&paths::log_path(), &new_records).context("cannot append to the pi-sync log")?;
    println!(
        "read {} records from {}, added {} to this device's log",
        incoming.len(),
        blob_path.display(),
        new_records.len()
    );
    Ok(log::union(&existing, &incoming))
}

fn sync(agent: &Path, blob_path: &Path, dry_run: bool, prefer: Option<&str>) -> Result<()> {
    // A preview writes nothing at all: neither the pi agent directory nor the
    // log. It reads the blob, merges it in memory, and reports.
    if dry_run {
        let passphrase = blob::passphrase()?;
        let incoming = read_records(blob_path, &passphrase)?;
        let existing = log::read(&paths::log_path()).context("cannot read the pi-sync log")?;
        let merged = log::union(&existing, &incoming);
        let view = consolidate::view(&merged, prefer);

        let pending = incoming
            .iter()
            .filter(|record| {
                !existing
                    .iter()
                    .any(|known| log::same_position(known, record))
            })
            .count();
        println!(
            "would add {pending} records from {} and apply {} keys across {} tags (nothing written)",
            blob_path.display(),
            view.values.len(),
            view.tags().len()
        );
        for line in consolidate::conflict_lines(&view) {
            println!("! {line}");
        }
        return Ok(());
    }

    let records = pull(blob_path)?;
    let applied = apply_view(agent, &records, prefer)?;
    report(&applied);
    Ok(())
}

fn apply_log(agent: &Path, prefer: Option<&str>) -> Result<apply::Applied> {
    let records = log::read(&paths::log_path()).context("cannot read the pi-sync log")?;
    if records.is_empty() {
        bail!("the pi-sync log is empty — run `pi-sync pull` first");
    }
    apply_view(agent, &records, prefer)
}

fn apply_view(agent: &Path, records: &[Record], prefer: Option<&str>) -> Result<apply::Applied> {
    let view = consolidate::view(records, prefer);
    let applied = apply::apply(&view, agent);

    for line in consolidate::conflict_lines(&view) {
        println!("! {line}");
    }
    Ok(applied)
}

fn status(agent: &Path) -> Result<()> {
    let records = log::read(&paths::log_path()).context("cannot read the pi-sync log")?;
    if records.is_empty() {
        println!("log is empty ({})", paths::log_path().display());
        return Ok(());
    }
    let view = consolidate::view(&records, None);

    let mut by_tag: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for record in &records {
        *by_tag.entry(record.tag.clone()).or_default() += 1;
    }

    let mut hosts: Vec<&str> = records.iter().map(|r| r.host.as_str()).collect();
    hosts.sort_unstable();
    hosts.dedup();

    println!(
        "{} records from {} device{}, {} keys in view",
        records.len(),
        hosts.len(),
        if hosts.len() == 1 { "" } else { "s" },
        view.values.len()
    );
    for (tag, count) in &by_tag {
        println!("  {tag}: {count} records");
    }

    let pins: Vec<String> = view
        .tags()
        .into_iter()
        .filter(|tag| tag.starts_with("pin:"))
        .map(|tag| tag.trim_start_matches("pin:").to_string())
        .collect();
    if !pins.is_empty() {
        println!("  pinned: {}", pins.join(", "));
    }

    for line in consolidate::conflict_lines(&view) {
        println!("  ! {line}");
    }

    println!("agent directory: {}", agent.display());
    Ok(())
}

fn report(applied: &apply::Applied) {
    println!(
        "applied {} files, {} settings{}",
        applied.files,
        applied.settings,
        if applied.pins.is_empty() {
            String::new()
        } else {
            format!(", checked out pins: {}", applied.pins.join(", "))
        }
    );
    for failure in &applied.failures {
        println!("! not applied: {failure}");
    }
}

fn human_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_mentions_the_default_blob_and_passphrase_host() {
        let mut help = Vec::new();
        Cli::command().write_long_help(&mut help).unwrap();
        let text = String::from_utf8(help).unwrap();
        assert!(text.contains("pi-sync/blob"), "{text}");
        assert!(text.contains("octx creds add pi-sync"), "{text}");
    }

    #[test]
    fn every_subcommand_parses() {
        for args in [
            vec!["pi-sync", "push"],
            vec!["pi-sync", "pull"],
            vec!["pi-sync", "sync"],
            vec!["pi-sync", "sync", "--dry-run"],
            vec!["pi-sync", "apply", "--prefer", "abc"],
            vec!["pi-sync", "status"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
    }

    #[test]
    fn there_is_no_init_subcommand() {
        assert!(Cli::try_parse_from(["pi-sync", "init"]).is_err());
    }

    #[test]
    fn explicit_blob_overrides_the_default_for_one_call() {
        let explicit = blob_path(&Some(PathBuf::from("/somewhere/blob")));
        assert_eq!(explicit, PathBuf::from("/somewhere/blob"));
        assert_ne!(blob_path(&None), PathBuf::from("/somewhere/blob"));
    }
}
