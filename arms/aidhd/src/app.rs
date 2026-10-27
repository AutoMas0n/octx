use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, bail};
use chrono::{Duration, Local, NaiveDate};
use rusqlite::Connection;

use crate::cli::{Cli, Commands};
use crate::config;
use crate::db;
use crate::export;
use crate::llm::{LlmClient, LlmError};
use crate::model::{NewTask, Priority, Recurrence, Status, Task, TaskFilter, TaskUpdate};
use crate::sink::{PlatformSink, Sink};

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
                parsed.due_date = Some(value);
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
            let task = db::add_task(conn, &parsed)?;
            println!("{}", short_id(&task.id));
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
                    due_date: due.map(Some),
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
            let task = resolve(conn, &id)?;
            let next = db::complete_task(conn, &task.id)?;
            match next {
                Some(next) => println!(
                    "completed {}; next occurrence {} due {}",
                    short_id(&task.id),
                    short_id(&next.id),
                    next.due_date
                        .map(|d| d.format("%Y-%m-%d").to_string())
                        .unwrap_or_default()
                ),
                None => println!("completed {}", short_id(&task.id)),
            }
            Ok(())
        }
        Commands::Search { text } => {
            for task in db::search_tasks(conn, &text)? {
                println!("{}", format_task(&task, 0));
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
        Commands::Next => {
            match db::next_task(conn, Local::now().date_naive())? {
                Some(task) => println!("{}", action_prompt(&task)),
                None => println!("No open tasks."),
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
        Commands::Voice => voice(conn, sink),
        Commands::Ask { question } => {
            let answer = match ask_with_ai(llm, conn, &question)? {
                Some(answer) => answer,
                None => deterministic_ask(conn, &question)?,
            };
            println!("{answer}");
            Ok(())
        }
        Commands::Suggest { id } => {
            let task = resolve(conn, &id)?;
            let suggestion =
                suggest_with_ai(llm, &task)?.unwrap_or_else(|| deterministic_suggest(&task));
            println!("{suggestion}");
            Ok(())
        }
    }
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
    Ok(db::find_task_by_prefix(conn, id)?)
}

pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
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

fn voice(conn: &mut Connection, sink: &dyn Sink) -> anyhow::Result<()> {
    let transcript = sink
        .listen()
        .map_err(|e| anyhow::anyhow!("voice unavailable: {e}"))?;
    eprintln!("transcript: {transcript}");
    let parsed = deterministic_parse(&transcript);
    if parsed.title.is_empty() {
        bail!("could not parse a task from the transcript");
    }
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
    use crate::sink::{Result as SinkResult, Sink};
    use std::cell::RefCell;

    struct MockSink {
        notified: RefCell<Vec<(String, String)>>,
        spoken: RefCell<Vec<String>>,
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
            Ok(self.transcript.clone())
        }
    }

    fn mock(transcript: &str) -> MockSink {
        MockSink {
            notified: RefCell::new(Vec::new()),
            spoken: RefCell::new(Vec::new()),
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

    #[test]
    fn voice_saves_without_prompt_and_speaks() {
        let mut conn = db::open_in_memory().unwrap();
        let sink = mock("add task buy milk");
        voice(&mut conn, &sink).unwrap();
        let tasks = db::list_tasks(&conn, &TaskFilter::default()).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "buy milk");
        assert_eq!(sink.spoken.borrow().len(), 1);
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
}
