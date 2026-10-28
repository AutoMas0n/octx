//! Reminder schedule setup and execution.
//!
//! A reminder is installed into the platform's scheduler (`termux-job-scheduler`
//! or `crontab`) as two daily runs: one in the morning and one in the evening.
//! The installed command calls `aidhd reminders run`, which reuses the same
//! selection, quiet hours, and deduplication as `next` and `due --notify`.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use chrono::{Local, Timelike};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::secretary;
use crate::sink::Sink;

/// Default morning reminder hour in local time.
pub const MORNING_HOUR: u8 = 8;
/// Default evening reminder hour in local time.
pub const EVENING_HOUR: u8 = 19;

/// Persisted reminder settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderSettings {
    pub enabled: bool,
    pub morning: u8,
    pub evening: u8,
}

impl Default for ReminderSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            morning: MORNING_HOUR,
            evening: EVENING_HOUR,
        }
    }
}

impl ReminderSettings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

fn settings_path() -> Result<PathBuf> {
    let base = dirs::data_dir().context("no data directory")?;
    Ok(base.join("octx").join("aidhd").join("reminders.json"))
}

/// Schedulers this arm knows how to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheduler {
    Termux,
    Cron,
}

impl Scheduler {
    pub fn name(self) -> &'static str {
        match self {
            Scheduler::Termux => "termux-job-scheduler",
            Scheduler::Cron => "crontab",
        }
    }
}

/// Detect a usable scheduler, preferring the Termux job scheduler.
pub fn detect_scheduler() -> Option<Scheduler> {
    if command_exists("termux-job-scheduler") {
        Some(Scheduler::Termux)
    } else if command_exists("crontab") {
        Some(Scheduler::Cron)
    } else {
        None
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The executable path used inside installed schedules.
fn arm_binary() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
        .unwrap_or_else(|| "aidhd".into())
}

/// Write a small wrapper script the Termux job scheduler can execute.
fn write_wrapper_script(binary: &str) -> Result<PathBuf> {
    let base = dirs::data_dir().context("no data directory")?;
    let dir = base.join("octx").join("aidhd");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("reminders.sh");
    let body = format!("#!/bin/sh\n\"{binary}\" reminders run\n");
    std::fs::write(&path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms)?;
    }
    Ok(path)
}

/// Description of the schedule that `enable` would install.
pub fn schedule_description(settings: &ReminderSettings) -> String {
    format!(
        "morning {:02}:00 and evening {:02}:00 daily via `{} reminders run`",
        settings.morning, settings.evening, "aidhd"
    )
}

/// Install the two daily reminder runs. Returns a human description.
pub fn install(scheduler: Scheduler, settings: &ReminderSettings) -> Result<String> {
    let binary = arm_binary();
    match scheduler {
        Scheduler::Termux => {
            // The Termux job scheduler only runs periodically, so install one
            // hourly job and let `run` gate on the configured reminder hours.
            let script = write_wrapper_script(&binary)?;
            let output = Command::new("termux-job-scheduler")
                .args([
                    "--script",
                    &script.to_string_lossy(),
                    "--period-ms",
                    "3600000",
                    "--job-id",
                    "aidhd-reminders",
                    "--persisted",
                    "true",
                ])
                .output()
                .context("run termux-job-scheduler")?;
            if !output.status.success() {
                bail!(
                    "termux-job-scheduler failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            Ok(format!(
                "installed {} ({})",
                schedule_description(settings),
                scheduler.name()
            ))
        }
        Scheduler::Cron => {
            let line = |hour: u8| format!("0 {hour} * * * {binary} reminders run");
            let body = format!("{}\n{}\n", line(settings.morning), line(settings.evening));
            let existing = Command::new("crontab").arg("-l").output();
            let current = existing
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
                .unwrap_or_default();
            let filtered: String = current
                .lines()
                .filter(|entry| !entry.contains("aidhd reminders run"))
                .map(|entry| format!("{entry}\n"))
                .collect();
            let next = format!("{filtered}{body}");
            let mut child = Command::new("crontab")
                .arg("-")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .context("run crontab")?;
            {
                use std::io::Write;
                let stdin = child.stdin.as_mut().context("crontab stdin")?;
                stdin.write_all(next.as_bytes())?;
            }
            let status = child.wait()?;
            if !status.success() {
                bail!("crontab rejected the reminder schedule");
            }
            Ok(format!(
                "installed {} ({})",
                schedule_description(settings),
                scheduler.name()
            ))
        }
    }
}

/// Remove installed reminder runs.
pub fn remove(scheduler: Scheduler) -> Result<()> {
    match scheduler {
        Scheduler::Termux => {
            let _ = Command::new("termux-job-scheduler")
                .args(["--cancel", "aidhd-reminders"])
                .output();
        }
        Scheduler::Cron => {
            let existing = Command::new("crontab").arg("-l").output();
            let current = existing
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
                .unwrap_or_default();
            let filtered: String = current
                .lines()
                .filter(|entry| !entry.contains("aidhd reminders run"))
                .map(|entry| format!("{entry}\n"))
                .collect();
            let mut child = Command::new("crontab")
                .arg("-")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .context("run crontab")?;
            {
                use std::io::Write;
                let stdin = child.stdin.as_mut().context("crontab stdin")?;
                stdin.write_all(filtered.as_bytes())?;
            }
            let status = child.wait()?;
            if !status.success() {
                bail!("crontab rejected the reminder removal");
            }
        }
    }
    Ok(())
}

/// Print the manual cron lines used when no scheduler is available.
pub fn manual_cron_lines(settings: &ReminderSettings) -> String {
    let binary = arm_binary();
    format!(
        "0 {morning} * * * {binary} reminders run\n0 {evening} * * * {binary} reminders run",
        morning = settings.morning,
        evening = settings.evening
    )
}

/// Run one reminder window, reusing the same selection and quiet hours as
/// `next` and `due --notify`. Deduplicates through `notified_at`.
pub fn run(conn: &Connection, sink: &dyn Sink, settings: &ReminderSettings) -> Result<()> {
    let config = crate::config::resolve_secretary()?;
    run_at(
        conn,
        sink,
        settings,
        Local::now().hour(),
        config.quiet_hours,
    )
}

/// Run one reminder window at a specific local hour and quiet-hours window
/// (exposed for tests).
pub fn run_at(
    conn: &Connection,
    sink: &dyn Sink,
    settings: &ReminderSettings,
    hour: u32,
    quiet_hours: (u8, u8),
) -> Result<()> {
    if !settings.enabled {
        return Ok(());
    }
    // Cron fires at the exact hour; the hourly Termux job needs this gate.
    if hour != settings.morning as u32 && hour != settings.evening as u32 {
        return Ok(());
    }
    if secretary::in_quiet_hours(hour, quiet_hours) {
        return Ok(());
    }
    let today = Local::now().date_naive();
    let Some(next) = db::next_task(conn, today)? else {
        return Ok(());
    };
    if next.notified_at.is_some() {
        return Ok(());
    }
    let body = format!(
        "Reminder: {} ({}).",
        next.title,
        crate::app::short_id(&next.id)
    );
    sink.notify("aidhd reminder", &body)
        .map_err(|error| anyhow::anyhow!("notification failed: {error}"))?;
    db::mark_notified(conn, std::slice::from_ref(&next.id))?;
    println!("{body}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewTask;
    use crate::sink::Result as SinkResult;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        messages: Mutex<Vec<String>>,
    }

    impl Sink for RecordingSink {
        fn notify(&self, title: &str, body: &str) -> SinkResult<()> {
            self.messages
                .lock()
                .unwrap()
                .push(format!("{title}: {body}"));
            Ok(())
        }
        fn speak(&self, _text: &str) -> SinkResult<()> {
            Ok(())
        }
        fn listen(&self) -> SinkResult<String> {
            Ok(String::new())
        }
    }

    fn settings(enabled: bool, morning: u8, evening: u8) -> ReminderSettings {
        ReminderSettings {
            enabled,
            morning,
            evening,
        }
    }

    fn setup_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.db");
        let mut conn = crate::db::open(&path).unwrap();
        crate::db::add_task(
            &mut conn,
            &NewTask {
                title: "test task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        (dir, conn)
    }

    #[test]
    fn disabled_does_nothing() {
        let (_dir, conn) = setup_db();
        let sink = RecordingSink::default();
        run_at(&conn, &sink, &settings(false, 8, 19), 8, (0, 0)).unwrap();
        assert!(sink.messages.lock().unwrap().is_empty());
    }

    #[test]
    fn wrong_hour_does_nothing() {
        let (_dir, conn) = setup_db();
        let sink = RecordingSink::default();
        run_at(&conn, &sink, &settings(true, 8, 19), 12, (0, 0)).unwrap();
        assert!(sink.messages.lock().unwrap().is_empty());
    }

    #[test]
    fn quiet_hours_suppresses_the_prompt() {
        let (_dir, conn) = setup_db();
        let sink = RecordingSink::default();
        run_at(&conn, &sink, &settings(true, 8, 19), 8, (7, 9)).unwrap();
        assert!(sink.messages.lock().unwrap().is_empty());
    }

    #[test]
    fn sends_once_and_dedups_the_second_run() {
        let (_dir, conn) = setup_db();
        let sink = RecordingSink::default();
        run_at(&conn, &sink, &settings(true, 8, 19), 8, (0, 0)).unwrap();
        assert_eq!(sink.messages.lock().unwrap().len(), 1);
        run_at(&conn, &sink, &settings(true, 8, 19), 8, (0, 0)).unwrap();
        assert_eq!(sink.messages.lock().unwrap().len(), 1);
    }
}
