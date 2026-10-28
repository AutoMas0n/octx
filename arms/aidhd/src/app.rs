use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, bail};
use chrono::{Datelike, Duration, Local, NaiveDate, Timelike};
use rusqlite::Connection;

use crate::cli::{Cli, Commands, DecisionsCommand, RemindersCommand};
use crate::config;
use crate::db;
use crate::export;
use crate::llm::{LlmClient, LlmError};
use crate::model::{NewTask, Priority, Recurrence, Status, Task, TaskFilter, TaskUpdate};
use crate::reminders::{self, ReminderSettings};
use crate::secretary;
use crate::sink::{Cue, PlatformSink, Sink, SinkError};

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let path = db::default_db_path()?;
    let mut conn = db::open(&path)?;
    let sink = PlatformSink::new(crate::sink::detect());
    let llm = LlmClient::new(config::resolve()?);
    dispatch(&mut conn, cli.command, &sink, &llm)
}

fn dispatch(
    conn: &mut Connection,
    command: Commands,
    sink: &dyn Sink,
    llm: &LlmClient,
) -> anyhow::Result<()> {
    match command {
        Commands::Add {
            title,
            priority,
            category,
            due,
            repeat,
            parent,
            tags,
            json,
            unscheduled,
        } => {
            let raw_title = read_title(title)?;
            let mut parsed = deterministic_parse(&raw_title);
            if let Some(ai_task) = parse_with_ai(llm, &raw_title) {
                parsed = ai_task;
            }
            if let Some(value) = parse_priority(priority)? {
                parsed.priority = Some(value);
            }
            if let Some(value) = category {
                parsed.category = Some(value);
            }
            if let Some(value) = due {
                parsed.due_date = Some(parse_due_date(&value)?);
            }
            if let Some(value) = parse_recurrence(repeat)? {
                parsed.recurrence_pattern = Some(value);
            }
            if !tags.is_empty() {
                parsed.tags = tags;
            }
            parsed.parent_task_id = parent
                .map(|prefix| db::find_task_by_prefix(conn, &prefix).map(|task| task.id))
                .transpose()?;
            if parsed.title.trim().is_empty() {
                parsed.title = raw_title;
            }

            // Auto-schedule only when nothing supplied a date and the task is
            // not recurring: a recurring task's next date comes from its
            // pattern, never from an arbitrary slot.
            let secretary = config::resolve_secretary()?;
            let auto = secretary.auto_schedule
                && !unscheduled
                && parsed.recurrence_pattern.is_none()
                && parsed.due_date.is_none();
            let mut scheduling: Option<String> = None;
            if auto {
                let today = Local::now().date_naive();
                let horizon_end = today + Duration::days(secretary.horizon_days);
                let counts = db::load_by_day(conn, today, horizon_end)?;
                let priority = parsed.priority.unwrap_or(Priority::Medium);
                let choice = secretary::auto_schedule(
                    priority,
                    today,
                    &counts,
                    secretary.daily_cap,
                    secretary.horizon_days,
                );
                if let Some(day) = choice {
                    parsed.due_date = Some(day);
                }
                scheduling = Some(secretary::schedule_reason(
                    priority,
                    today,
                    choice,
                    secretary.horizon_days,
                ));
            }

            let task = db::add_task(conn, &parsed)?;
            if json {
                let mut value = serde_json::to_value(&task)?;
                if let (Some(reason), Some(object)) = (&scheduling, value.as_object_mut()) {
                    object.insert(
                        "scheduled_date".into(),
                        serde_json::json!(
                            task.due_date.map(|day| day.format("%Y-%m-%d").to_string())
                        ),
                    );
                    object.insert("scheduling_reason".into(), serde_json::json!(reason));
                }
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}", short_id(&task.id));
                if let Some(reason) = &scheduling {
                    eprintln!("{reason}");
                }
            }
            Ok(())
        }
        Commands::List {
            status,
            category,
            parent,
            cancelled,
            json,
        } => {
            let status = if cancelled {
                Some(Status::Cancelled)
            } else {
                match status {
                    Some(value) => Some(
                        Status::from_str(&value)
                            .with_context(|| format!("unknown status: {value}"))?,
                    ),
                    None => Some(Status::Pending),
                }
            };
            let parent_task_id = parent
                .map(|prefix| db::find_task_by_prefix(conn, &prefix).map(|task| task.id))
                .transpose()?;
            let tasks = db::list_tasks(
                conn,
                &TaskFilter {
                    status,
                    category,
                    parent_task_id,
                    ..TaskFilter::default()
                },
            )?;
            let counts = db::open_child_counts(conn)?;
            if json {
                let mut items = Vec::with_capacity(tasks.len());
                for task in &tasks {
                    let mut value = serde_json::to_value(task)?;
                    if let Some(object) = value.as_object_mut() {
                        object.insert(
                            "step_count".into(),
                            serde_json::json!(counts.get(&task.id).copied().unwrap_or(0)),
                        );
                    }
                    items.push(value);
                }
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                if tasks.is_empty() {
                    print_first_run_hint(conn)?;
                }
                for task in &tasks {
                    println!(
                        "{}",
                        format_task(task, counts.get(&task.id).copied().unwrap_or(0))
                    );
                }
            }
            Ok(())
        }
        Commands::Edit {
            id,
            title,
            priority,
            category,
            due,
        } => {
            let task = resolve(conn, &id)?;
            let updated = db::update_task(
                conn,
                &task.id,
                &TaskUpdate {
                    title,
                    priority: parse_priority(priority)?,
                    category: category.map(Some),
                    due_date: due
                        .as_deref()
                        .map(|value| parse_due_date(value).map(Some))
                        .transpose()?,
                    ..TaskUpdate::default()
                },
            )?;
            println!("{}", short_id(&updated.id));
            Ok(())
        }
        Commands::Reopen { id } => {
            let task = resolve(conn, &id)?;
            let reopened = db::reopen_task(conn, &task.id)?;
            println!("{}", short_id(&reopened.id));
            Ok(())
        }
        Commands::Done { id } => {
            let task = resolve_done_target(conn, id.as_deref())?;
            let next = db::complete_task(conn, &task.id)?;
            let done_today = db::completed_on(conn, Local::now().date_naive())?;
            match next {
                Some(next) => println!(
                    "completed {}; next occurrence {} due {} ({done_today} done today)",
                    short_id(&task.id),
                    short_id(&next.id),
                    next.due_date
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default()
                ),
                None => println!("completed {} ({done_today} done today)", short_id(&task.id)),
            }
            Ok(())
        }
        Commands::Pin { id } => pin_task(conn, &id, true),
        Commands::Unpin { id } => pin_task(conn, &id, false),
        Commands::Secretary {
            dry_run,
            notify,
            status,
            json,
        } => {
            if status {
                secretary_status(conn, json)
            } else {
                secretary_pass(conn, dry_run, notify, json, sink, llm)
            }
        }
        Commands::Reminders { dry_run, action } => {
            let action = action.unwrap_or(RemindersCommand::Preview);
            match action {
                RemindersCommand::Preview => {
                    println!(
                        "{}",
                        reminders::schedule_description(&ReminderSettings::load()?)
                    );
                    Ok(())
                }
                RemindersCommand::Enable => {
                    let mut settings = ReminderSettings::load()?;
                    if dry_run {
                        println!(
                            "would install {}",
                            reminders::schedule_description(&settings)
                        );
                        return Ok(());
                    }
                    if let Some(scheduler) = reminders::detect_scheduler() {
                        let message = reminders::install(scheduler, &settings)?;
                        settings.enabled = true;
                        settings.save()?;
                        println!("{message}");
                        Ok(())
                    } else {
                        println!(
                            "No supported scheduler found. Add these lines to your crontab manually:"
                        );
                        println!("{}", reminders::manual_cron_lines(&settings));
                        bail!(
                            "no supported scheduler found; install termux-job-scheduler or crontab"
                        )
                    }
                }
                RemindersCommand::Disable => {
                    if dry_run {
                        println!("would stop future reminder prompts");
                        return Ok(());
                    }
                    if let Some(scheduler) = reminders::detect_scheduler() {
                        reminders::remove(scheduler)?;
                    }
                    let mut settings = ReminderSettings::load()?;
                    settings.enabled = false;
                    settings.save()?;
                    println!("reminders disabled");
                    Ok(())
                }
                RemindersCommand::Run => {
                    let settings = ReminderSettings::load()?;
                    reminders::run(conn, sink, &settings)
                }
            }
        }
        Commands::Decisions {
            limit,
            json,
            command,
        } => match command {
            Some(DecisionsCommand::Prune { before }) => {
                let result = db::prune_decisions(conn, before)?;
                println!(
                    "deleted {} decisions; kept {} undone",
                    result.deleted, result.retained_undone
                );
                Ok(())
            }
            None => {
                let decisions = db::list_decisions(conn, None, None, limit)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&decisions)?);
                } else if decisions.is_empty() {
                    println!("No decisions.");
                } else {
                    for decision in decisions {
                        println!("{}", format_decision(&decision));
                    }
                }
                Ok(())
            }
        },
        Commands::Undo { target } => {
            let id = match target.as_deref() {
                None | Some("last") => db::list_decisions(conn, None, None, None)?
                    .into_iter()
                    .find(|decision| decision.undone_at.is_none())
                    .map(|decision| decision.id)
                    .ok_or_else(|| anyhow::anyhow!("no decision to undo"))?,
                Some(id) => db::get_decision(conn, id)?.id,
            };
            let task = db::undo_decision(conn, &id)?;
            println!(
                "undid {}; {} now due {}",
                short_id(&id),
                short_id(&task.id),
                task.due_date
                    .map(|day| day.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "none".into())
            );
            Ok(())
        }
        Commands::Search { text } => {
            let tasks = db::search_tasks(conn, &text)?;
            if tasks.is_empty() {
                println!("No tasks match '{text}'. Add one with `aidhd add \"{text}\"`.");
                print_first_run_hint(conn)?;
            } else {
                for task in &tasks {
                    println!("{}", format_task(task, 0));
                }
            }
            Ok(())
        }
        Commands::Stats => {
            let stats = db::stats(conn)?;
            println!(
                "pending={} completed={} cancelled={} total={}",
                stats.pending, stats.completed, stats.cancelled, stats.total
            );
            Ok(())
        }
        Commands::Today { json } => {
            let today = Local::now().date_naive();
            let tasks = db::list_tasks(
                conn,
                &TaskFilter {
                    status: Some(Status::Pending),
                    ..TaskFilter::default()
                },
            )?;
            let counts = db::open_child_counts(conn)?;
            let groups = partition_by_due(today, &tasks);
            if json {
                let mut value = serde_json::Map::new();
                for (label, items) in &groups {
                    if items.is_empty() {
                        continue;
                    }
                    value.insert(label.to_string(), serde_json::to_value(items)?);
                }
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else if tasks.is_empty() {
                println!("No open tasks. Add one with `aidhd add \"<title>\"`.");
            } else {
                for (label, items) in &groups {
                    if items.is_empty() {
                        continue;
                    }
                    println!("{label}:");
                    for task in items {
                        println!(
                            "  {}",
                            format_task(task, counts.get(&task.id).copied().unwrap_or(0))
                        );
                    }
                }
            }
            Ok(())
        }
        Commands::Show { id } => show_task(conn, &id),
        Commands::Next => {
            match db::next_task(conn, Local::now().date_naive())? {
                Some(task) => println!("{}", action_prompt(&task)),
                None => {
                    println!("No open tasks.");
                    print_first_run_hint(conn)?;
                }
            }
            Ok(())
        }
        Commands::Due {
            within,
            notify,
            force,
        } => {
            let end = parse_window(&within)?;
            let due = db::due_tasks(conn, end)?;
            if notify {
                notify_due(conn, sink, due, force)
            } else {
                for task in due {
                    println!("{}", format_task(&task, 0));
                }
                Ok(())
            }
        }
        Commands::Export { format, out } => {
            match format.as_str() {
                "json" => export::export_json(conn, out.as_deref())?,
                "csv" => export::export_csv(conn, out.as_deref())?,
                _ => bail!("unsupported format: {format}"),
            }
            if let Some(out) = out {
                println!("{}", out.display());
            }
            Ok(())
        }
        Commands::Voice => {
            // The judge is optional: without a credential or with AI off,
            // voice stays fully local and deterministic.
            let judge = (ai_enabled() && llm.has_credential()).then_some(LlmCompleter(llm));
            voice(
                conn,
                sink,
                judge.as_ref().map(|j| j as &dyn secretary::Completer),
            )
        }
        Commands::Ask { question } => {
            let answer = match ask_with_ai(llm, conn, &question)? {
                Some(answer) => answer,
                None => deterministic_ask(conn, &question)?,
            };
            println!("{answer}");
            Ok(())
        }
        Commands::Serve { port } => crate::web::serve(port),
        Commands::Suggest { id } => {
            let task = resolve(conn, &id)?;
            let suggestion =
                suggest_with_ai(llm, &task)?.unwrap_or_else(|| deterministic_suggest(&task));
            println!("{suggestion}");
            Ok(())
        }
    }
}

/// Partition open tasks into the three daily groups, in print order.
fn partition_by_due(today: NaiveDate, tasks: &[Task]) -> [(&'static str, Vec<&Task>); 3] {
    let mut groups: [(&'static str, Vec<&Task>); 3] = [
        ("Overdue", Vec::new()),
        ("Today", Vec::new()),
        ("Upcoming", Vec::new()),
    ];
    for task in tasks {
        let index = match task.due_date {
            Some(due) if due < today => 0,
            Some(due) if due == today => 1,
            _ => 2,
        };
        groups[index].1.push(task);
    }
    groups
}

fn show_task(conn: &Connection, id: &str) -> anyhow::Result<()> {
    let task = db::find_task_by_prefix(conn, id).map_err(|_| {
        anyhow::anyhow!("no task matches '{id}'; run `aidhd today` to see open work")
    })?;
    println!("id: {}", short_id(&task.id));
    println!("title: {}", task.title);
    println!("status: {}", task.status.as_str());
    println!("priority: {}", task.priority.as_str());
    println!(
        "due: {}",
        task.due_date
            .map(|day| day.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "none".into())
    );
    println!("category: {}", task.category.as_deref().unwrap_or("none"));
    println!(
        "tags: {}",
        if task.tags.is_empty() {
            "none".to_string()
        } else {
            task.tags.join(", ")
        }
    );
    println!(
        "description: {}",
        task.description.as_deref().unwrap_or("none")
    );
    println!("notes: {}", task.notes.as_deref().unwrap_or("none"));
    let subtasks = db::list_tasks(
        conn,
        &TaskFilter {
            parent_task_id: Some(task.id.clone()),
            ..TaskFilter::default()
        },
    )?;
    if subtasks.is_empty() {
        println!("subtasks: none");
    } else {
        println!("subtasks:");
        for sub in subtasks {
            println!("  {}  {}", short_id(&sub.id), sub.title);
        }
    }
    let decisions = db::list_decisions(conn, Some(&task.id), None, Some(5))?;
    if decisions.is_empty() {
        println!("decisions: none");
    } else {
        println!("decisions:");
        for decision in decisions {
            println!("  {}", format_decision(&decision));
        }
    }
    Ok(())
}

fn read_title(title: String) -> anyhow::Result<String> {
    if title == "-" {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        let trimmed = input.trim().to_string();
        if trimmed.is_empty() {
            bail!("stdin did not contain a task title");
        }
        Ok(trimmed)
    } else {
        Ok(title)
    }
}

fn parse_priority(value: Option<String>) -> anyhow::Result<Option<Priority>> {
    value
        .map(|v| Priority::from_str(&v).with_context(|| format!("unknown priority: {v}")))
        .transpose()
}

fn parse_recurrence(value: Option<String>) -> anyhow::Result<Option<Recurrence>> {
    value
        .map(|v| Recurrence::from_str(&v).with_context(|| format!("unknown recurrence: {v}")))
        .transpose()
}

fn resolve(conn: &Connection, id: &str) -> anyhow::Result<Task> {
    db::find_task_by_prefix(conn, id)
        .map_err(|_| anyhow::anyhow!("no task matches '{id}'; run `aidhd list` to see tasks"))
}

/// Resolve the target of `done`: no argument or `last` means the task `next`
/// would surface; anything else is an id prefix first, then a title substring.
fn resolve_done_target(conn: &Connection, id: Option<&str>) -> anyhow::Result<Task> {
    match id {
        None | Some("last") => db::next_task(conn, Local::now().date_naive())?.ok_or_else(|| {
            anyhow::anyhow!("no open task to complete; run `aidhd today` to see what is open")
        }),
        Some(value) => {
            if let Ok(task) = db::find_task_by_prefix(conn, value) {
                return Ok(task);
            }
            let matches = db::pending_title_matches(conn, value)?;
            match matches.len() {
                1 => Ok(matches.into_iter().next().expect("one match")),
                0 => Err(anyhow::anyhow!(
                    "no task matches '{value}'; run `aidhd list` to see tasks"
                )),
                _ => {
                    println!("'{value}' matches more than one pending task:");
                    for task in &matches {
                        println!("  {}  {}", short_id(&task.id), task.title);
                    }
                    Err(anyhow::anyhow!(
                        "nothing completed; rerun `done` with a longer title or an id"
                    ))
                }
            }
        }
    }
}

/// Print a getting-started hint when the store is completely empty.
fn print_first_run_hint(conn: &Connection) -> anyhow::Result<()> {
    if db::stats(conn)?.total == 0 {
        println!("Getting started: add your first task with `aidhd add \"buy milk\"`.");
    }
    Ok(())
}

pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

fn pin_task(conn: &Connection, id: &str, pinned: bool) -> anyhow::Result<()> {
    let task = resolve(conn, id)?;
    let updated = db::update_task(
        conn,
        &task.id,
        &TaskUpdate {
            pinned: Some(pinned),
            ..TaskUpdate::default()
        },
    )?;
    println!("{}", short_id(&updated.id));
    Ok(())
}

fn format_decision(decision: &crate::model::Decision) -> String {
    let old = decision
        .old_due_date
        .map(|day| day.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "none".into());
    let new = decision
        .new_due_date
        .map(|day| day.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "none".into());
    format!(
        "{}  {}  {} -> {}  {}{}",
        short_id(&decision.id),
        short_id(&decision.task_id),
        old,
        new,
        decision.reason,
        if decision.undone_at.is_some() {
            "  (undone)"
        } else {
            ""
        }
    )
}

fn cadence_seconds(cadence: &str) -> i64 {
    let text = cadence.trim().to_lowercase();
    match text.as_str() {
        "daily" => 86_400,
        "hourly" => 3_600,
        "weekly" => 604_800,
        other => {
            let parse_unit = |suffix: char, factor: i64| {
                other
                    .strip_suffix(suffix)
                    .and_then(|value| value.parse::<i64>().ok())
                    .map(|value| value * factor)
            };
            parse_unit('h', 3_600)
                .or_else(|| parse_unit('m', 60))
                .or_else(|| parse_unit('d', 86_400))
                .unwrap_or(86_400)
        }
    }
}

fn run_age_seconds(started_at: &str) -> Option<i64> {
    let started = chrono::DateTime::parse_from_rfc3339(started_at).ok()?;
    Some(
        chrono::Utc::now()
            .signed_duration_since(started.with_timezone(&chrono::Utc))
            .num_seconds(),
    )
}

fn secretary_status(conn: &Connection, json: bool) -> anyhow::Result<()> {
    let config = config::resolve_secretary()?;
    let Some(run) = db::latest_run(conn)? else {
        bail!("no secretary run recorded yet");
    };
    let cadence = cadence_seconds(&config.cadence);
    let age = run_age_seconds(&run.started_at);
    let overdue = age.map(|age| age > 2 * cadence).unwrap_or(false);
    if json {
        let value = serde_json::json!({
            "last_run": run.id,
            "started_at": run.started_at,
            "finished_at": run.finished_at,
            "decisions": run.decisions,
            "error": run.error,
            "cadence": config.cadence,
            "overdue": overdue,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "last run {} started {} finished {} decisions {} error {}",
            short_id(&run.id),
            run.started_at,
            run.finished_at.unwrap_or_else(|| "never".into()),
            run.decisions,
            run.error.unwrap_or_else(|| "none".into())
        );
        println!(
            "cadence {}; age {}; overdue {}",
            config.cadence,
            age.map(|age| format!("{age}s"))
                .unwrap_or_else(|| "unknown".into()),
            overdue
        );
    }
    if overdue {
        bail!(
            "scheduler may be dead: last run is older than twice the {} cadence",
            config.cadence
        );
    }
    Ok(())
}

struct LlmCompleter<'a>(&'a LlmClient);

impl secretary::Completer for LlmCompleter<'_> {
    fn complete(&self, system: &str, user: &str) -> std::result::Result<String, String> {
        self.0
            .complete(system, user)
            .map_err(|error| error.to_string())
    }
}

fn secretary_pass(
    conn: &mut Connection,
    dry_run: bool,
    notify: bool,
    json: bool,
    sink: &dyn Sink,
    llm: &LlmClient,
) -> anyhow::Result<()> {
    let config = config::resolve_secretary()?;
    let today = Local::now().date_naive();
    if dry_run {
        let temp =
            std::env::temp_dir().join(format!("aidhd-secretary-dry-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&temp);
        conn.execute("VACUUM INTO ?1", [temp.to_string_lossy().to_string()])?;
        let mut copy = db::open(&temp)?;
        let summary = db::run_secretary_pass(&mut copy, &config, today, None)?;
        let result = print_pass(&copy, &summary, json);
        drop(copy);
        let _ = std::fs::remove_file(&temp);
        return result;
    }

    let adapter = LlmCompleter(llm);
    let summary = if ai_enabled() {
        db::run_secretary_pass(conn, &config, today, Some(&adapter))?
    } else {
        db::run_secretary_pass(conn, &config, today, None)?
    };
    if notify && summary.total() > 0 {
        let hour = Local::now().hour();
        if !secretary::in_quiet_hours(hour, config.quiet_hours)
            && let Some((body, ids)) = secretary_prompt(conn, &summary, today)?
        {
            sink.notify("aidhd secretary", &body)
                .map_err(|error| anyhow::anyhow!("notification failed: {error}"))?;
            db::mark_notified(conn, &ids)?;
            println!("{body}");
        }
    }
    print_pass(conn, &summary, json)
}

/// Build the single combined prompt for a pass, if one should be sent.
///
/// The prompt names the move count, the next action, and how to undo. It is
/// skipped when the same next task was already announced: the shared
/// `notified_at` marker keeps a re-run from nagging about the same task, while
/// the decisions themselves are still recorded.
fn secretary_prompt(
    conn: &Connection,
    summary: &db::PassSummary,
    today: NaiveDate,
) -> anyhow::Result<Option<(String, Vec<String>)>> {
    let Some(next) = db::next_task(conn, today)? else {
        return Ok(None);
    };
    if next.notified_at.is_some() {
        return Ok(None);
    }
    let mut moved = summary.rescheduled + summary.scheduled;
    if summary.decomposed > 0 {
        moved += summary.decomposed;
    }
    let undo = db::list_decisions(conn, None, None, Some(1))?
        .into_iter()
        .next()
        .map(|decision| format!("`aidhd undo {}`", short_id(&decision.id)))
        .unwrap_or_else(|| "`aidhd undo`".into());
    let body = format!(
        "{moved} task(s) moved. Next: {} ({}). Undo with {undo}.",
        next.title,
        short_id(&next.id)
    );
    Ok(Some((body, vec![next.id])))
}

fn print_pass(conn: &Connection, summary: &db::PassSummary, json: bool) -> anyhow::Result<()> {
    let run = db::latest_run(conn)?;
    let decisions = match &run {
        Some(run) => db::list_decisions(conn, None, Some(&run.id), None)?,
        None => Vec::new(),
    };
    if json {
        let value = serde_json::json!({
            "decomposed": summary.decomposed,
            "scheduled": summary.scheduled,
            "rescheduled": summary.rescheduled,
            "skipped": summary.skipped,
            "decisions": decisions,
        });
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!(
            "decomposed={} scheduled={} rescheduled={} skipped={}",
            summary.decomposed, summary.scheduled, summary.rescheduled, summary.skipped
        );
        for decision in decisions {
            println!("{}", format_decision(&decision));
        }
    }
    Ok(())
}

fn format_task(task: &Task, step_count: usize) -> String {
    let due = task
        .due_date
        .map(|d| format!(" due={}", d.format("%Y-%m-%d")))
        .unwrap_or_default();
    let category = task
        .category
        .as_ref()
        .map(|c| format!(" category={c}"))
        .unwrap_or_default();
    let recurrence = task
        .recurrence_pattern
        .map(|r| format!(" repeat={}", r.as_str()))
        .unwrap_or_default();
    let parent = task
        .parent_task_id
        .as_ref()
        .map(|p| format!(" parent={}", short_id(p)))
        .unwrap_or_default();
    let steps = if step_count > 0 {
        format!(" [{step_count} steps]")
    } else {
        String::new()
    };
    format!(
        "{}  [{}]{}{}{}{}{}  {}",
        short_id(&task.id),
        task.priority.as_str(),
        due,
        category,
        recurrence,
        parent,
        steps,
        task.title
    )
}

fn action_prompt(task: &Task) -> String {
    let today = Local::now().date_naive();
    let prefix = match task.due_date {
        Some(due) if due < today => "Overdue",
        Some(_) => "Due",
        None => "Next",
    };
    format!("{prefix}: {}. Do it now?", task.title)
}

/// Parse a due date given as ISO (`2026-03-04`) or a natural phrase.
///
/// Accepted phrases, matched case-insensitively: `today`, `tomorrow`,
/// `yesterday`, a weekday name (`monday`..`sunday`), `next week`, and
/// `in N days` / `in N weeks`. A weekday resolves to its next occurrence,
/// counting today. Anything else is an error so a typo never silently
/// schedules a task on an arbitrary day.
pub fn parse_due_date(value: &str) -> anyhow::Result<NaiveDate> {
    let text = value.trim();
    if text.is_empty() {
        bail!("due date is empty");
    }
    let today = Local::now().date_naive();
    let lower = text.to_lowercase();
    match lower.as_str() {
        "today" => return Ok(today),
        "tomorrow" => return Ok(today + Duration::days(1)),
        "yesterday" => return Ok(today - Duration::days(1)),
        "next week" => return Ok(today + Duration::days(7)),
        _ => {}
    }
    if let Some(day) = weekday_from_name(&lower) {
        let today_weekday = today.weekday();
        let ahead = (day.num_days_from_monday() as i64
            - today_weekday.num_days_from_monday() as i64)
            .rem_euclid(7);
        return Ok(today + Duration::days(ahead));
    }
    if let Some(rest) = lower.strip_prefix("in ") {
        let mut parts = rest.split_whitespace();
        if let (Some(number), Some(unit)) = (parts.next(), parts.next())
            && parts.next().is_none()
            && let Ok(count) = number.parse::<i64>()
        {
            let days = match unit.trim_end_matches('s') {
                "day" => count,
                "week" => count * 7,
                _ => 0,
            };
            if days > 0 {
                return Ok(today + Duration::days(days));
            }
        }
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("unrecognized due date: {value}"))
}

fn weekday_from_name(name: &str) -> Option<chrono::Weekday> {
    use chrono::Weekday;
    match name {
        "monday" | "mon" => Some(Weekday::Mon),
        "tuesday" | "tue" | "tues" => Some(Weekday::Tue),
        "wednesday" | "wed" => Some(Weekday::Wed),
        "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thu),
        "friday" | "fri" => Some(Weekday::Fri),
        "saturday" | "sat" => Some(Weekday::Sat),
        "sunday" | "sun" => Some(Weekday::Sun),
        _ => None,
    }
}

/// Parse a due window such as `1d`, `12h`, or `1w` into an inclusive end date.
pub fn parse_window(value: &str) -> anyhow::Result<NaiveDate> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("window is empty");
    }
    // A bare number means days; otherwise split the digits from the unit.
    let (number, unit) = match trimmed.find(|c: char| !c.is_ascii_digit()) {
        Some(split) => trimmed.split_at(split),
        None => (trimmed, "d"),
    };
    let number: i64 = number
        .parse()
        .with_context(|| format!("invalid window: {value}"))?;
    if number < 0 {
        bail!("window must be positive: {value}");
    }
    let now = Local::now();
    let end = match unit {
        "h" => now + Duration::hours(number),
        "d" => now + Duration::days(number),
        "w" => now + Duration::weeks(number),
        _ => bail!("unsupported window unit: {value}"),
    };
    Ok(end.date_naive())
}

/// Filter unnotified due tasks and send one action prompt. Returns the sent body.
pub fn notify_due(
    conn: &Connection,
    sink: &dyn Sink,
    due: Vec<Task>,
    force: bool,
) -> anyhow::Result<()> {
    let candidates: Vec<Task> = due
        .into_iter()
        .filter(|task| force || task.notified_at.is_none())
        .collect();
    if candidates.is_empty() {
        return Ok(());
    }
    let body = if candidates.len() == 1 {
        action_prompt(&candidates[0])
    } else {
        let next = candidates
            .iter()
            .min_by_key(|task| (task.priority, task.due_date))
            .expect("candidates is non-empty");
        format!(
            "You have {} tasks due. Next: {}. Do it now?",
            candidates.len(),
            next.title
        )
    };
    sink.notify("aidhd", &body)
        .map_err(|e| anyhow::anyhow!("notification failed: {e}"))?;
    let ids: Vec<String> = candidates.into_iter().map(|task| task.id).collect();
    db::mark_notified(conn, &ids)?;
    println!("{body}");
    Ok(())
}

/// Deterministic parser used when no LLM is available or for voice transcripts.
/// Matching is case-insensitive so capitalized speech-to-text transcripts work.
pub fn deterministic_parse(input: &str) -> NewTask {
    let mut text = input.trim().to_string();
    for prefix in [
        "add task",
        "create task",
        "new task",
        "remind me to",
        "remind me",
        "add ",
    ] {
        if let Some(rest) = strip_prefix_ci(&text, prefix) {
            text = rest.trim().to_string();
            break;
        }
    }
    let today = Local::now().date_naive();
    let mut due_date = None;
    let lower = text.to_lowercase();
    if lower.contains("tomorrow") {
        due_date = Some(today + Duration::days(1));
        text = remove_word_ci(&text, "tomorrow");
    } else if lower.contains("today") {
        due_date = Some(today);
        text = remove_word_ci(&text, "today");
    }
    let mut priority = None;
    let lower = text.to_lowercase();
    if lower.contains("urgent") || lower.contains("high priority") {
        priority = Some(Priority::High);
        text = remove_word_ci(&text, "urgent");
        text = remove_word_ci(&text, "high priority");
    }
    NewTask {
        title: text,
        due_date,
        priority,
        ..NewTask::default()
    }
}

/// Strip a leading prefix without regard to ASCII case.
fn strip_prefix_ci(input: &str, prefix: &str) -> Option<String> {
    if input.len() < prefix.len() || !input.is_char_boundary(prefix.len()) {
        return None;
    }
    let head = input.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        Some(input[prefix.len()..].to_string())
    } else {
        None
    }
}

/// Remove a word or phrase case-insensitively and normalize whitespace.
fn remove_word_ci(input: &str, needle: &str) -> String {
    let lower = input.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let mut result = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find(&needle_lower) {
        let start = cursor + rel;
        let end = start + needle.len();
        if !input.is_char_boundary(start) || !input.is_char_boundary(end) {
            break;
        }
        result.push_str(&input[cursor..start]);
        cursor = end;
    }
    result.push_str(&input[cursor..]);
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the voice judge decided about one transcript.
#[derive(Debug)]
enum Judgement {
    Task(NewTask),
    Noise,
}

#[derive(Debug, Default, serde::Deserialize)]
struct VoiceReply {
    #[serde(default)]
    is_task: Option<bool>,
    title: Option<String>,
    due_date: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    recurrence: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

/// Decide whether a transcript is a task.
///
/// The model is a gate, not an authority: it may only choose "task" or
/// "noise" and clean the text. Missing scheduling detail is never a reason to
/// reject, because the secretary dates undated tasks afterwards. Any unusable
/// reply falls back to the deterministic parser.
fn judge_voice(transcript: &str, completer: Option<&dyn secretary::Completer>) -> Judgement {
    let Some(completer) = completer else {
        return deterministic_judgement(transcript);
    };
    let today = Local::now().date_naive();
    let system = format!(
        "Decide whether a speech-to-text transcript is a task the user wants \
remembered. Today is {today}. Reject only greetings, filler, background \
chatter, or fragments with no actionable or memorable content. Accept \
anything the user plausibly wants remembered, even when no date, time, or \
priority is mentioned: missing scheduling detail is never a reason to \
reject. Never invent a due date; use null when none is implied. Strip \
filler words and lead-ins like 'remind me to'. Reply with JSON only: \
{{\"is_task\": true or false, \"title\": string, \"due_date\": \"YYYY-MM-DD\" or null, \
\"priority\": \"high\" or \"medium\" or \"low\" or null}}."
    );
    match completer.complete(&system, transcript) {
        Ok(raw) => parse_judgement(&raw).unwrap_or_else(|| deterministic_judgement(transcript)),
        Err(error) => {
            eprintln!("voice judgement unavailable: {error}; using deterministic parsing.");
            deterministic_judgement(transcript)
        }
    }
}

fn parse_judgement(raw: &str) -> Option<Judgement> {
    let json = extract_json(raw)?;
    let reply: VoiceReply = serde_json::from_str(json).ok()?;
    if reply.is_task == Some(false) {
        return Some(Judgement::Noise);
    }
    let title = reply.title.unwrap_or_default().trim().to_string();
    if title.is_empty() {
        return Some(Judgement::Noise);
    }
    // A date the model invented or that has already passed is dropped, not a
    // reason to reject: the secretary will schedule the task.
    let today = Local::now().date_naive();
    let due_date = reply
        .due_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .filter(|day| *day >= today);
    Some(Judgement::Task(NewTask {
        title,
        due_date,
        priority: reply.priority.as_deref().and_then(Priority::from_str),
        category: reply.category,
        recurrence_pattern: reply.recurrence.as_deref().and_then(Recurrence::from_str),
        tags: reply.tags,
        ..NewTask::default()
    }))
}

fn deterministic_judgement(transcript: &str) -> Judgement {
    let parsed = deterministic_parse(transcript);
    if parsed.title.trim().is_empty() {
        Judgement::Noise
    } else {
        Judgement::Task(parsed)
    }
}

fn voice(
    conn: &mut Connection,
    sink: &dyn Sink,
    judge: Option<&dyn secretary::Completer>,
) -> anyhow::Result<()> {
    // A rising chime says "speak now"; the falling one says "capture ended".
    // Cues are best effort and must never block capture.
    let _ = sink.cue(Cue::Start);
    let captured = sink.listen();
    let _ = sink.cue(Cue::Stop);
    let transcript = captured.map_err(|e| match e {
        SinkError::NoSpeech => anyhow::anyhow!("no speech detected; nothing saved"),
        SinkError::Timeout(seconds) => {
            anyhow::anyhow!("no speech within {seconds}s; nothing saved")
        }
        other => anyhow::anyhow!("voice unavailable: {other}"),
    })?;
    eprintln!("transcript: {transcript}");
    let parsed = match judge_voice(&transcript, judge) {
        Judgement::Task(parsed) => parsed,
        Judgement::Noise => {
            let _ = sink.cue(Cue::Reject);
            bail!("not a task; nothing saved");
        }
    };
    let task = db::add_task(conn, &parsed)?;
    let confirmation = format!("Saved: {}", task.title);
    if let Err(error) = sink.speak(&confirmation) {
        eprintln!("warning: confirmation failed: {error}");
    }
    println!("{}", short_id(&task.id));
    Ok(())
}

fn ai_enabled() -> bool {
    !matches!(
        std::env::var("AIDHD_AI_DISABLED").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

#[derive(Debug, serde::Deserialize)]
struct AiTask {
    title: Option<String>,
    due_date: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    recurrence: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

fn extract_json(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (start <= end).then(|| &raw[start..=end])
}

fn parse_with_ai(client: &LlmClient, text: &str) -> Option<NewTask> {
    if !ai_enabled() {
        return None;
    }
    let today = Local::now().date_naive();
    let system = format!(
        "Parse a task description into JSON only. Today is {today}. Use keys title, due_date (YYYY-MM-DD or null), priority (high, medium, low, or null), category (string or null), recurrence (daily, weekly, monthly, or null), tags (array of strings). Do not invent a due date when none is implied."
    );
    let raw = match client.complete(&system, text) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("AI parsing unavailable: {error}; using deterministic parsing.");
            return None;
        }
    };
    let json = extract_json(&raw)?;
    let parsed: AiTask = serde_json::from_str(json).ok()?;
    let title = parsed.title.unwrap_or_else(|| text.to_string());
    let due_date = parsed
        .due_date
        .as_deref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok());
    let priority = parsed.priority.as_deref().and_then(Priority::from_str);
    let recurrence = parsed.recurrence.as_deref().and_then(Recurrence::from_str);
    Some(NewTask {
        title,
        due_date,
        priority,
        category: parsed.category,
        recurrence_pattern: recurrence,
        tags: parsed.tags,
        ..NewTask::default()
    })
}

fn ask_with_ai(
    client: &LlmClient,
    conn: &Connection,
    question: &str,
) -> anyhow::Result<Option<String>> {
    if !ai_enabled() {
        return Ok(None);
    }
    let pending = db::list_tasks(
        conn,
        &TaskFilter {
            status: Some(Status::Pending),
            limit: Some(20),
            ..TaskFilter::default()
        },
    )?;
    let mut context = String::new();
    for task in pending {
        context.push_str(&format!(
            "- {} (priority {}, due {})\n",
            task.title,
            task.priority.as_str(),
            task.due_date
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "none".into())
        ));
    }
    let system = "You are AIDHD, a concise task assistant. Answer using only the provided task list. Be brief and action-oriented.";
    let user = format!("Question: {question}\nTasks:\n{context}");
    match client.complete(system, &user) {
        Ok(answer) => Ok(Some(answer).filter(|answer| !answer.trim().is_empty())),
        Err(LlmError::Credential(error)) => Err(anyhow::anyhow!("{error}")),
        Err(error) => {
            eprintln!("AI ask unavailable: {error}; using deterministic fallback.");
            Ok(None)
        }
    }
}

fn suggest_with_ai(client: &LlmClient, task: &Task) -> anyhow::Result<Option<String>> {
    if !ai_enabled() {
        return Ok(None);
    }
    let system = "Suggest a priority, category, and tags for this task in one short sentence. Return text only.";
    let user = serde_json::to_string(task)?;
    match client.complete(system, &user) {
        Ok(answer) => Ok(Some(answer).filter(|answer| !answer.trim().is_empty())),
        Err(LlmError::Credential(error)) => Err(anyhow::anyhow!("{error}")),
        Err(error) => {
            eprintln!("AI ask unavailable: {error}; using deterministic fallback.");
            Ok(None)
        }
    }
}

fn deterministic_suggest(task: &Task) -> String {
    let category = task.category.clone().unwrap_or_else(|| "general".into());
    format!(
        "priority={} category={} tags={}",
        task.priority.as_str(),
        category,
        if task.tags.is_empty() {
            "none".to_string()
        } else {
            task.tags.join(",")
        }
    )
}

fn deterministic_ask(conn: &Connection, question: &str) -> anyhow::Result<String> {
    let lower = question.to_lowercase();
    if lower.contains("first") || lower.contains("next") || lower.contains("now") {
        return Ok(match db::next_task(conn, Local::now().date_naive())? {
            Some(task) => action_prompt(&task),
            None => "No open tasks.".into(),
        });
    }
    let stats = db::stats(conn)?;
    Ok(format!(
        "pending={} completed={} cancelled={} total={}",
        stats.pending, stats.completed, stats.cancelled, stats.total
    ))
}

/// Resolve the arm database path for callers and tests.
pub fn database_path() -> anyhow::Result<PathBuf> {
    Ok(db::default_db_path()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sink::{Cue, Result as SinkResult, Sink};
    use std::cell::RefCell;

    struct MockSink {
        notified: RefCell<Vec<(String, String)>>,
        spoken: RefCell<Vec<String>>,
        events: RefCell<Vec<&'static str>>,
        transcript: String,
    }

    impl Sink for MockSink {
        fn notify(&self, title: &str, body: &str) -> SinkResult<()> {
            self.notified.borrow_mut().push((title.into(), body.into()));
            Ok(())
        }

        fn speak(&self, text: &str) -> SinkResult<()> {
            self.spoken.borrow_mut().push(text.into());
            Ok(())
        }

        fn listen(&self) -> SinkResult<String> {
            self.events.borrow_mut().push("listen");
            Ok(self.transcript.clone())
        }

        fn cue(&self, cue: Cue) -> SinkResult<()> {
            self.events.borrow_mut().push(match cue {
                Cue::Start => "start",
                Cue::Stop => "stop",
                Cue::Reject => "reject",
            });
            Ok(())
        }
    }

    fn mock(transcript: &str) -> MockSink {
        MockSink {
            notified: RefCell::new(Vec::new()),
            spoken: RefCell::new(Vec::new()),
            events: RefCell::new(Vec::new()),
            transcript: transcript.into(),
        }
    }

    #[test]
    fn window_parses_units() {
        assert!(parse_window("1d").is_ok());
        assert!(parse_window("12h").is_ok());
        assert!(parse_window("2w").is_ok());
        assert!(parse_window("1m").is_err());
        assert!(parse_window("day").is_err());
    }

    #[test]
    fn deterministic_parser_strips_prefixes_and_dates() {
        let task = deterministic_parse("add task fix garage tomorrow");
        assert_eq!(task.title, "fix garage");
        assert!(task.due_date.is_some());
    }

    struct FakeCompleter(&'static str);

    impl secretary::Completer for FakeCompleter {
        fn complete(&self, _system: &str, _user: &str) -> std::result::Result<String, String> {
            Ok(self.0.to_string())
        }
    }

    #[test]
    fn judgement_accepts_a_task_without_any_scheduling() {
        let judgement = parse_judgement(
            r#"{"is_task": true, "title": "buy milk", "due_date": null, "priority": null}"#,
        )
        .unwrap();
        match judgement {
            Judgement::Task(task) => {
                assert_eq!(task.title, "buy milk");
                assert_eq!(task.due_date, None);
            }
            Judgement::Noise => panic!("an undated task must be accepted"),
        }
    }

    #[test]
    fn judgement_drops_a_past_or_invented_date_but_keeps_the_task() {
        let judgement =
            parse_judgement(r#"{"is_task": true, "title": "buy milk", "due_date": "2001-01-01"}"#)
                .unwrap();
        match judgement {
            Judgement::Task(task) => assert_eq!(task.due_date, None),
            Judgement::Noise => panic!("a past date must not reject the task"),
        }
    }

    #[test]
    fn judgement_rejects_noise_and_empty_titles() {
        assert!(matches!(
            parse_judgement(r#"{"is_task": false, "title": ""}"#),
            Some(Judgement::Noise)
        ));
        assert!(matches!(
            parse_judgement(r#"{"is_task": true, "title": "   "}"#),
            Some(Judgement::Noise)
        ));
        // An unusable reply returns None so the caller uses the deterministic path.
        assert!(parse_judgement("not json").is_none());
    }

    #[test]
    fn deterministic_judgement_rejects_only_empty_speech() {
        assert!(matches!(deterministic_judgement(""), Judgement::Noise));
        assert!(matches!(deterministic_judgement("  "), Judgement::Noise));
        match deterministic_judgement("buy milk") {
            Judgement::Task(task) => assert_eq!(task.title, "buy milk"),
            Judgement::Noise => panic!("plain speech is a task"),
        }
    }

    #[test]
    fn voice_plays_a_reject_cue_and_saves_nothing_for_noise() {
        let mut conn = db::open_in_memory().unwrap();
        let sink = mock("um hello");
        let judge = FakeCompleter(r#"{"is_task": false, "title": ""}"#);
        let error = voice(&mut conn, &sink, Some(&judge)).unwrap_err();
        assert!(error.to_string().contains("not a task"));
        assert!(
            db::list_tasks(&conn, &TaskFilter::default())
                .unwrap()
                .is_empty()
        );
        assert!(sink.spoken.borrow().is_empty());
        assert_eq!(*sink.events.borrow(), ["start", "listen", "stop", "reject"]);
    }

    #[test]
    fn voice_uses_the_judged_task_including_its_cleaned_title() {
        let mut conn = db::open_in_memory().unwrap();
        let sink = mock("uh buy milk thing");
        let judge = FakeCompleter(r#"{"is_task": true, "title": "buy milk"}"#);
        voice(&mut conn, &sink, Some(&judge)).unwrap();
        let tasks = db::list_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(tasks[0].title, "buy milk");
        assert_eq!(sink.events.borrow().len(), 3);
    }

    #[test]
    fn voice_falls_back_when_the_model_reply_is_unusable() {
        let mut conn = db::open_in_memory().unwrap();
        let sink = mock("buy milk");
        let judge = FakeCompleter("garbage");
        voice(&mut conn, &sink, Some(&judge)).unwrap();
        let tasks = db::list_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(tasks[0].title, "buy milk");
    }

    #[test]
    fn voice_saves_without_prompt_and_speaks() {
        let mut conn = db::open_in_memory().unwrap();
        let sink = mock("add task buy milk");
        voice(&mut conn, &sink, None).unwrap();
        let tasks = db::list_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "buy milk");
        assert_eq!(sink.spoken.borrow().len(), 1);
        // The start cue precedes capture; the stop cue follows it.
        assert_eq!(*sink.events.borrow(), ["start", "listen", "stop"]);
    }

    #[test]
    fn due_notification_is_skipped_when_empty() {
        let conn = db::open_in_memory().unwrap();
        let sink = mock("");
        notify_due(&conn, &sink, Vec::new(), false).unwrap();
        assert!(sink.notified.borrow().is_empty());
    }

    #[test]
    fn due_notification_sends_once_and_respects_force() {
        let mut conn = db::open_in_memory().unwrap();
        let task = db::add_task(
            &mut conn,
            &NewTask {
                title: "pay bill".into(),
                due_date: Some(Local::now().date_naive()),
                ..NewTask::default()
            },
        )
        .unwrap();
        let sink = mock("");
        let due = db::due_tasks(&conn, Local::now().date_naive()).unwrap();
        notify_due(&conn, &sink, due, false).unwrap();
        assert_eq!(sink.notified.borrow().len(), 1);
        let due = db::due_tasks(&conn, Local::now().date_naive()).unwrap();
        notify_due(&conn, &sink, due, false).unwrap();
        assert_eq!(sink.notified.borrow().len(), 1);
        let due = db::due_tasks(&conn, Local::now().date_naive()).unwrap();
        notify_due(&conn, &sink, due, true).unwrap();
        assert_eq!(sink.notified.borrow().len(), 2);
        assert!(db::get_task(&conn, &task.id).unwrap().notified_at.is_some());
    }

    #[test]
    fn due_date_parses_iso_and_relative_phrases() {
        let today = Local::now().date_naive();
        assert_eq!(
            parse_due_date("2026-03-04").unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
        );
        assert_eq!(parse_due_date("today").unwrap(), today);
        assert_eq!(
            parse_due_date("Tomorrow").unwrap(),
            today + Duration::days(1)
        );
        assert_eq!(
            parse_due_date("yesterday").unwrap(),
            today - Duration::days(1)
        );
        assert_eq!(
            parse_due_date("next week").unwrap(),
            today + Duration::days(7)
        );
        assert_eq!(
            parse_due_date("in 3 days").unwrap(),
            today + Duration::days(3)
        );
        assert_eq!(
            parse_due_date("in 2 weeks").unwrap(),
            today + Duration::days(14)
        );
    }

    #[test]
    fn due_date_parses_weekday_names_to_next_occurrence() {
        use chrono::Datelike;
        let today = Local::now().date_naive();
        for (index, name) in [
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ]
        .iter()
        .enumerate()
        {
            let day = parse_due_date(name).unwrap();
            assert_eq!(day.weekday().num_days_from_monday() as usize, index);
            let ahead = (day - today).num_days();
            assert!(
                (0..=6).contains(&ahead),
                "{name} resolved {ahead} days ahead"
            );
        }
    }

    #[test]
    fn due_date_rejects_unknown_phrases() {
        assert!(parse_due_date("someday").is_err());
        assert!(parse_due_date("  ").is_err());
    }
}
