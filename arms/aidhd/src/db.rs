use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, NaiveTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use thiserror::Error;
use uuid::Uuid;

use crate::model::{
    Decision, DecisionKind, NewDecision, NewTask, Priority, Recurrence, Reschedule, Run, Scheduled,
    Stats, Status, Task, TaskFilter, TaskUpdate,
};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid stored value: {0}")]
    Invalid(String),
    #[error("stale due date for task {task_id}: expected {expected}, found {actual}")]
    Conflict {
        task_id: String,
        expected: String,
        actual: String,
    },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    id                TEXT PRIMARY KEY NOT NULL,
    title             TEXT NOT NULL,
    description       TEXT,
    notes             TEXT,
    status            TEXT NOT NULL DEFAULT 'pending'
                      CHECK (status IN ('pending','completed','cancelled')),
    priority          TEXT NOT NULL DEFAULT 'medium'
                      CHECK (priority IN ('high','medium','low')),
    category          TEXT,
    due_date          TEXT,
    due_time          TEXT,
    recurrence_pattern TEXT
                      CHECK (recurrence_pattern IS NULL OR recurrence_pattern IN ('daily','weekly','monthly')),
    parent_task_id    TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    notified_at       TEXT,
    completed_at      TEXT,
    last_completed_at TEXT,
    decomposed_at     TEXT,
    pinned            INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_due ON tasks(due_date);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON tasks(parent_task_id);

CREATE TABLE IF NOT EXISTS task_tags (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (task_id, tag)
);
CREATE INDEX IF NOT EXISTS idx_task_tags_tag ON task_tags(tag);

CREATE TABLE IF NOT EXISTS decisions (
    id           TEXT PRIMARY KEY NOT NULL,
    task_id      TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL DEFAULT 'rescheduled'
                 CHECK (kind IN ('rescheduled','skipped','scheduled','decomposed')),
    old_due_date TEXT,
    new_due_date TEXT,
    reason       TEXT NOT NULL,
    details      TEXT,
    created_at   TEXT NOT NULL,
    undone_at    TEXT,
    run_id       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_decisions_task ON decisions(task_id);
CREATE INDEX IF NOT EXISTS idx_decisions_run ON decisions(run_id);
CREATE INDEX IF NOT EXISTS idx_decisions_created ON decisions(created_at);

CREATE TABLE IF NOT EXISTS secretary_runs (
    id          TEXT PRIMARY KEY NOT NULL,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    decisions   INTEGER NOT NULL DEFAULT 0,
    error       TEXT
);
"#;

/// Resolve the arm-owned database path. `OCTX_AIDHD_DB` overrides it for tests.
pub fn default_db_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OCTX_AIDHD_DB") {
        return Ok(PathBuf::from(path));
    }
    let base = dirs::data_dir().ok_or_else(|| DbError::Invalid("no data directory".into()))?;
    Ok(base.join("octx").join("aidhd").join("tasks.db"))
}

/// Open a database, create its parent directory, and apply connection pragmas.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Open a fresh in-memory database with the same schema and pragmas.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    init_schema(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

/// Create tables and indexes if they are absent.
pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|e| DbError::Invalid(format!("due_date {value:?}: {e}")))
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let status: String = row.get("status")?;
    let priority: String = row.get("priority")?;
    let recurrence: Option<String> = row.get("recurrence_pattern")?;
    let due: Option<String> = row.get("due_date")?;
    let due_t: Option<String> = row.get("due_time")?;
    Ok(Task {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        notes: row.get("notes")?,
        status: Status::from_str(&status).unwrap_or(Status::Pending),
        priority: Priority::from_str(&priority).unwrap_or(Priority::Medium),
        category: row.get("category")?,
        due_date: due
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        due_time: due_t
            .as_deref()
            .and_then(|t| NaiveTime::parse_from_str(t, "%H:%M:%S").ok()),
        recurrence_pattern: recurrence.as_deref().and_then(Recurrence::from_str),
        parent_task_id: row.get("parent_task_id")?,
        notified_at: row.get("notified_at")?,
        completed_at: row.get("completed_at")?,
        last_completed_at: row.get("last_completed_at")?,
        decomposed_at: row.get("decomposed_at")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        tags: Vec::new(),
    })
}

fn load_tags(conn: &Connection, task_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM task_tags WHERE task_id = ?1 ORDER BY tag")?;
    let rows = stmt.query_map([task_id], |row| row.get::<_, String>(0))?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

fn attach_tags(conn: &Connection, task: &mut Task) -> Result<()> {
    task.tags = load_tags(conn, &task.id)?;
    Ok(())
}

/// Insert a task and its tags, returning the stored task.
pub fn add_task(conn: &mut Connection, new: &NewTask) -> Result<Task> {
    let title = new.title.trim();
    if title.is_empty() {
        return Err(DbError::Invalid("title must not be empty".into()));
    }
    if let Some(parent) = &new.parent_task_id {
        get_task(conn, parent)?;
    }
    let id = Uuid::new_v4().to_string();
    let timestamp = now();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO tasks (
            id, title, description, notes, status, priority, category,
            due_date, due_time, recurrence_pattern, parent_task_id, pinned, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![
            id,
            title,
            new.description,
            new.notes,
            new.priority.unwrap_or(Priority::Medium).as_str(),
            new.category,
            new.due_date.map(|d| d.format("%Y-%m-%d").to_string()),
            new.due_time.map(|t| t.format("%H:%M:%S").to_string()),
            new.recurrence_pattern.map(Recurrence::as_str),
            new.parent_task_id,
            i64::from(new.pinned),
            timestamp,
        ],
    )?;
    for tag in &new.tags {
        tx.execute(
            "INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)",
            params![id, tag],
        )?;
    }
    tx.commit()?;
    get_task(conn, &id)
}

/// Fetch one task by id, including tags.
pub fn get_task(conn: &Connection, id: &str) -> Result<Task> {
    let mut task = conn
        .query_row("SELECT * FROM tasks WHERE id = ?1", [id], row_to_task)
        .optional()?
        .ok_or_else(|| DbError::NotFound(id.to_string()))?;
    attach_tags(conn, &mut task)?;
    Ok(task)
}

/// List tasks matching all supplied filters.
pub fn list_tasks(conn: &Connection, filter: &TaskFilter) -> Result<Vec<Task>> {
    let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
    let mut values: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(status) = filter.status {
        sql.push_str(" AND status = ?");
        values.push(status.as_str().to_string().into());
    }
    if let Some(category) = &filter.category {
        sql.push_str(" AND category = ?");
        values.push(category.clone().into());
    }
    if let Some(parent) = &filter.parent_task_id {
        sql.push_str(" AND parent_task_id = ?");
        values.push(parent.clone().into());
    }
    if let Some(search) = &filter.search {
        sql.push_str(" AND (title LIKE ? OR description LIKE ? OR notes LIKE ?)");
        let pattern = format!("%{search}%");
        values.push(pattern.clone().into());
        values.push(pattern.clone().into());
        values.push(pattern.into());
    }
    if let Some(date) = filter.due_on_or_before {
        sql.push_str(" AND due_date IS NOT NULL AND due_date <= ?");
        values.push(date.format("%Y-%m-%d").to_string().into());
    }
    sql.push_str(
        " ORDER BY CASE WHEN due_date IS NULL THEN 1 ELSE 0 END, due_date,
          CASE priority WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END, created_at",
    );
    if let Some(limit) = filter.limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), row_to_task)?;
    let mut tasks = Vec::new();
    for row in rows {
        let mut task = row?;
        attach_tags(conn, &mut task)?;
        tasks.push(task);
    }
    Ok(tasks)
}

/// Apply a partial update. A due-date change clears the notification marker.
pub fn update_task(conn: &Connection, id: &str, update: &TaskUpdate) -> Result<Task> {
    get_task(conn, id)?;
    let mut assignments: Vec<String> = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(title) = &update.title {
        if title.trim().is_empty() {
            return Err(DbError::Invalid("title must not be empty".into()));
        }
        assignments.push("title = ?".into());
        values.push(title.trim().to_string().into());
    }
    if let Some(description) = &update.description {
        assignments.push("description = ?".into());
        values.push(description.clone().into());
    }
    if let Some(notes) = &update.notes {
        assignments.push("notes = ?".into());
        values.push(notes.clone().into());
    }
    if let Some(status) = update.status {
        assignments.push("status = ?".into());
        values.push(status.as_str().to_string().into());
    }
    if let Some(priority) = update.priority {
        assignments.push("priority = ?".into());
        values.push(priority.as_str().to_string().into());
    }
    if let Some(category) = &update.category {
        assignments.push("category = ?".into());
        values.push(category.clone().into());
    }
    if let Some(due_date) = &update.due_date {
        assignments.push("due_date = ?".into());
        values.push(due_date.map(|d| d.format("%Y-%m-%d").to_string()).into());
        assignments.push("notified_at = NULL".into());
    }
    if let Some(due_time) = &update.due_time {
        assignments.push("due_time = ?".into());
        values.push(due_time.map(|t| t.format("%H:%M:%S").to_string()).into());
        assignments.push("notified_at = NULL".into());
    }
    if let Some(pinned) = update.pinned {
        assignments.push("pinned = ?".into());
        values.push(i64::from(pinned).into());
    }
    if let Some(decomposed_at) = &update.decomposed_at {
        assignments.push("decomposed_at = ?".into());
        values.push(decomposed_at.clone().into());
    }

    assignments.push("updated_at = ?".into());
    values.push(now().into());
    values.push(id.to_string().into());

    let sql = format!("UPDATE tasks SET {} WHERE id = ?", assignments.join(", "));
    conn.execute(&sql, params_from_iter(values.iter()))?;
    get_task(conn, id)
}

/// Mark a task complete. A recurring task creates its next open occurrence.
pub fn complete_task(conn: &mut Connection, id: &str) -> Result<Option<Task>> {
    complete_task_on(conn, id, Local::now().date_naive())
}

/// Mark a task complete as of `today`. A recurring task's next occurrence is
/// anchored to the completion date, not the old due date, so completing a late
/// task does not create a fresh backlog of missed occurrences.
pub fn complete_task_on(conn: &mut Connection, id: &str, today: NaiveDate) -> Result<Option<Task>> {
    let task = get_task(conn, id)?;
    let timestamp = now();
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE tasks SET status = 'completed', completed_at = ?2, last_completed_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, timestamp],
    )?;

    let next = if let (Some(pattern), Some(_due)) = (task.recurrence_pattern, task.due_date) {
        let next_due = pattern.advance(today);
        let next_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO tasks (
                id, title, description, notes, status, priority, category,
                due_date, recurrence_pattern, parent_task_id, pinned, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                next_id,
                task.title,
                task.description,
                task.notes,
                task.priority.as_str(),
                task.category,
                next_due.format("%Y-%m-%d").to_string(),
                pattern.as_str(),
                task.parent_task_id,
                i64::from(task.pinned),
                timestamp,
            ],
        )?;
        for tag in &task.tags {
            tx.execute(
                "INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)",
                params![next_id, tag],
            )?;
        }
        Some(next_id)
    } else {
        None
    };
    tx.commit()?;
    match next {
        Some(next_id) => get_task(conn, &next_id).map(Some),
        None => Ok(None),
    }
}

/// Reopen a completed or cancelled task and clear its notification marker.
pub fn reopen_task(conn: &Connection, id: &str) -> Result<Task> {
    get_task(conn, id)?;
    conn.execute(
        "UPDATE tasks SET status = 'pending', completed_at = NULL, notified_at = NULL, updated_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    get_task(conn, id)
}

/// Search titles, descriptions, and notes.
pub fn search_tasks(conn: &Connection, query: &str) -> Result<Vec<Task>> {
    let filter = TaskFilter {
        search: Some(query.to_string()),
        ..TaskFilter::default()
    };
    list_tasks(conn, &filter)
}

/// Count tasks by status.
pub fn stats(conn: &Connection) -> Result<Stats> {
    let mut pending = 0;
    let mut completed = 0;
    let mut cancelled = 0;
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM tasks GROUP BY status")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        match status.as_str() {
            "pending" => pending = count,
            "completed" => completed = count,
            "cancelled" => cancelled = count,
            _ => {}
        }
    }
    Ok(Stats {
        pending,
        completed,
        cancelled,
        total: pending + completed + cancelled,
    })
}

/// Open pending tasks with a due date on or before `end`.
pub fn due_tasks(conn: &Connection, end: NaiveDate) -> Result<Vec<Task>> {
    let filter = TaskFilter {
        status: Some(Status::Pending),
        due_on_or_before: Some(end),
        ..TaskFilter::default()
    };
    list_tasks(conn, &filter)
}

/// Count pending tasks per due date within `[start, end]`, inclusive.
///
/// Days with no pending tasks are absent from the map rather than zero, so
/// callers decide how to treat an empty day. Pinned and recurring tasks are
/// included here on purpose; the policy layer decides what to reschedule.
pub fn load_by_day(
    conn: &Connection,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<BTreeMap<NaiveDate, usize>> {
    let mut stmt = conn.prepare(
        "SELECT due_date, COUNT(*) FROM tasks
         WHERE status = 'pending' AND due_date IS NOT NULL
           AND decomposed_at IS NULL
           AND due_date >= ?1 AND due_date <= ?2
         GROUP BY due_date",
    )?;
    let rows = stmt.query_map(
        params![
            start.format("%Y-%m-%d").to_string(),
            end.format("%Y-%m-%d").to_string()
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let mut counts = BTreeMap::new();
    for row in rows {
        let (date, count) = row?;
        let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map_err(|e| DbError::Invalid(format!("stored due_date {date}: {e}")))?;
        counts.insert(date, count as usize);
    }
    Ok(counts)
}

/// Count open children per parent, for the `[N steps]` list marker.
pub fn open_child_counts(conn: &Connection) -> Result<BTreeMap<String, usize>> {
    let mut stmt = conn.prepare(
        "SELECT parent_task_id, COUNT(*) FROM tasks
         WHERE status = 'pending' AND parent_task_id IS NOT NULL
         GROUP BY parent_task_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut counts = BTreeMap::new();
    for row in rows {
        let (parent, count) = row?;
        counts.insert(parent, count as usize);
    }
    Ok(counts)
}

/// Record that the given tasks were notified.
pub fn mark_notified(conn: &Connection, ids: &[String]) -> Result<()> {
    let timestamp = now();
    for id in ids {
        conn.execute(
            "UPDATE tasks SET notified_at = ?2 WHERE id = ?1",
            params![id, timestamp],
        )?;
    }
    Ok(())
}

/// Resolve a task by unique id prefix. Ambiguous prefixes are rejected.
pub fn find_task_by_prefix(conn: &Connection, prefix: &str) -> Result<Task> {
    if prefix.is_empty() {
        return Err(DbError::Invalid("task id must not be empty".into()));
    }
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id LIKE ?1 LIMIT 2")?;
    let pattern = format!("{prefix}%");
    let mut rows = stmt.query_map([pattern], row_to_task)?;
    let first = rows.next().transpose()?;
    let second = rows.next().transpose()?;
    match (first, second) {
        (Some(_), Some(_)) => Err(DbError::Invalid(format!(
            "ambiguous task id prefix: {prefix}"
        ))),
        (Some(mut task), None) => {
            attach_tags(conn, &mut task)?;
            Ok(task)
        }
        (None, _) => Err(DbError::NotFound(prefix.to_string())),
    }
}

/// Select the most urgent open task: overdue first, then priority, then nearest due date.
pub fn next_task(conn: &Connection, today: NaiveDate) -> Result<Option<Task>> {
    let mut task = conn
        .query_row(
            "SELECT * FROM tasks WHERE status = 'pending' AND decomposed_at IS NULL
             ORDER BY CASE WHEN due_date IS NOT NULL AND due_date <= ?1 THEN 0 ELSE 1 END,
                      CASE priority WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END,
                      CASE WHEN due_date IS NULL THEN 1 ELSE 0 END,
                      due_date,
                      created_at
             LIMIT 1",
            [today.format("%Y-%m-%d").to_string()],
            row_to_task,
        )
        .optional()?;
    if let Some(task) = &mut task {
        attach_tags(conn, task)?;
    }
    Ok(task)
}

/// Parse a stored due date, exposed for tests and CLI formatting.
pub fn parse_stored_date(value: &str) -> Result<NaiveDate> {
    parse_date(value)
}

fn row_to_decision(row: &rusqlite::Row<'_>) -> rusqlite::Result<Decision> {
    let kind: String = row.get("kind")?;
    let old_due: Option<String> = row.get("old_due_date")?;
    let new_due: Option<String> = row.get("new_due_date")?;
    Ok(Decision {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        kind: DecisionKind::from_str(&kind).unwrap_or(DecisionKind::Rescheduled),
        old_due_date: old_due
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        new_due_date: new_due
            .as_deref()
            .and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
        reason: row.get("reason")?,
        details: row.get("details")?,
        created_at: row.get("created_at")?,
        undone_at: row.get("undone_at")?,
        run_id: row.get("run_id")?,
    })
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: row.get("id")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        decisions: row.get("decisions")?,
        error: row.get("error")?,
    })
}

/// Record a decision and return it.
pub fn add_decision(conn: &Connection, new: &NewDecision) -> Result<Decision> {
    get_task(conn, &new.task_id)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO decisions (
            id, task_id, kind, old_due_date, new_due_date, reason, details,
            created_at, undone_at, run_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9)",
        params![
            id,
            new.task_id,
            new.kind.as_str(),
            new.old_due_date.map(|d| d.format("%Y-%m-%d").to_string()),
            new.new_due_date.map(|d| d.format("%Y-%m-%d").to_string()),
            new.reason,
            new.details,
            now(),
            new.run_id,
        ],
    )?;
    get_decision(conn, &id)
}

/// Apply a set of reschedules as one all-or-nothing transaction.
///
/// Each item moves the task's due date and records a matching decision row.
/// The write is refused when the stored due date no longer matches the date
/// the policy observed, so a manual edit made during a run is never silently
/// overwritten. Any failure aborts the whole batch: partial reschedules would
/// leave day counts inconsistent with the decision log, so the pass either
/// lands entirely or not at all. Returns the number of reschedules applied.
pub fn apply_reschedules(
    conn: &mut Connection,
    items: &[Reschedule],
    run_id: &str,
) -> Result<usize> {
    let tx = conn.transaction()?;
    let timestamp = now();
    for item in items {
        // Outer option marks a missing row; inner marks a NULL due date.
        let found: Option<Option<String>> = tx
            .query_row(
                "SELECT due_date FROM tasks WHERE id = ?1",
                params![item.task_id],
                |row| row.get(0),
            )
            .optional()?;
        let current = found.ok_or_else(|| DbError::NotFound(item.task_id.clone()))?;
        let expected = item.from.format("%Y-%m-%d").to_string();
        match current {
            Some(stored) if stored == expected => {}
            Some(stored) => {
                return Err(DbError::Conflict {
                    task_id: item.task_id.clone(),
                    expected,
                    actual: stored,
                });
            }
            None => {
                return Err(DbError::Conflict {
                    task_id: item.task_id.clone(),
                    expected,
                    actual: "none".into(),
                });
            }
        }
        tx.execute(
            "UPDATE tasks SET due_date = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                item.task_id,
                item.to.format("%Y-%m-%d").to_string(),
                timestamp,
            ],
        )?;
        tx.execute(
            "INSERT INTO decisions (
                id, task_id, kind, old_due_date, new_due_date, reason, details,
                created_at, undone_at, run_id
             ) VALUES (?1, ?2, 'rescheduled', ?3, ?4, ?5, NULL, ?6, NULL, ?7)",
            params![
                Uuid::new_v4().to_string(),
                item.task_id,
                expected,
                item.to.format("%Y-%m-%d").to_string(),
                item.reason,
                timestamp,
                run_id,
            ],
        )?;
    }
    tx.commit()?;
    Ok(items.len())
}

/// Break a parent task into scheduled subtasks in one transaction.
///
/// The parent is cleared and marked decomposed *before* day loads are read,
/// so it no longer occupies a slot while its children are placed. Each child
/// inherits the parent's category, tags and priority, is linked by
/// `parent_task_id`, and is scheduled against the running day counts so the
/// children do not silently overfill a day. A child that finds no open day
/// within the horizon is skipped rather than pushed into the past.
///
/// Records one `decomposed` decision whose `details` JSON lists the child ids,
/// which is what undo later cancels. Returns the decision and child ids.
/// Apply a set of auto-scheduling decisions as one transaction.
///
/// A task whose due date is no longer empty was edited during the run; the
/// batch aborts rather than overwrite a manual date. Returns the number of
/// tasks scheduled.
pub fn apply_scheduled(conn: &mut Connection, items: &[Scheduled], run_id: &str) -> Result<usize> {
    let tx = conn.transaction()?;
    let timestamp = now();
    for item in items {
        let found: Option<Option<String>> = tx
            .query_row(
                "SELECT due_date FROM tasks WHERE id = ?1",
                params![item.task_id],
                |row| row.get(0),
            )
            .optional()?;
        let current = found.ok_or_else(|| DbError::NotFound(item.task_id.clone()))?;
        if let Some(stored) = current {
            return Err(DbError::Conflict {
                task_id: item.task_id.clone(),
                expected: "none".into(),
                actual: stored,
            });
        }
        tx.execute(
            "UPDATE tasks SET due_date = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                item.task_id,
                item.due_date.format("%Y-%m-%d").to_string(),
                timestamp,
            ],
        )?;
        tx.execute(
            "INSERT INTO decisions (
                id, task_id, kind, old_due_date, new_due_date, reason, details,
                created_at, undone_at, run_id
             ) VALUES (?1, ?2, 'scheduled', NULL, ?3, ?4, NULL, ?5, NULL, ?6)",
            params![
                Uuid::new_v4().to_string(),
                item.task_id,
                item.due_date.format("%Y-%m-%d").to_string(),
                item.reason,
                timestamp,
                run_id,
            ],
        )?;
    }
    tx.commit()?;
    Ok(items.len())
}

pub fn apply_decomposition(
    conn: &mut Connection,
    parent_id: &str,
    steps: &[String],
    run_id: &str,
    today: NaiveDate,
    cap: usize,
    horizon_days: i64,
) -> Result<(Decision, Vec<String>)> {
    let parent = get_task(conn, parent_id)?;
    if parent.status != Status::Pending {
        return Err(DbError::Invalid(
            "only pending tasks can be decomposed".into(),
        ));
    }
    if parent.pinned {
        return Err(DbError::Invalid("pinned tasks are never decomposed".into()));
    }
    if steps.is_empty() || steps.len() > crate::secretary::MAX_SUBTASKS {
        return Err(DbError::Invalid("invalid subtask count".into()));
    }
    let old_due = parent.due_date;
    let timestamp = now();

    let tx = conn.transaction()?;
    // Clear the parent first so `load_by_day` no longer counts it.
    tx.execute(
        "UPDATE tasks SET due_date = NULL, due_time = NULL, decomposed_at = ?2,
             updated_at = ?2 WHERE id = ?1",
        params![parent_id, timestamp],
    )?;

    let horizon_end = today + chrono::Duration::days(horizon_days);
    let mut counts = load_by_day(&tx, today, horizon_end)?;
    let mut child_ids: Vec<String> = Vec::new();
    for step in steps {
        let Some(day) = crate::secretary::next_open_day(today, today, &counts, cap, horizon_days)
        else {
            break;
        };
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO tasks (
                id, title, description, notes, status, priority, category,
                due_date, due_time, recurrence_pattern, parent_task_id,
                notified_at, completed_at, last_completed_at, decomposed_at,
                pinned, created_at, updated_at
             ) VALUES (?1, ?2, NULL, NULL, 'pending', ?3, ?4, ?5, NULL, NULL,
                       ?6, NULL, NULL, NULL, NULL, 0, ?7, ?7)",
            params![
                id,
                step,
                parent.priority.as_str(),
                parent.category,
                day.format("%Y-%m-%d").to_string(),
                parent_id,
                timestamp,
            ],
        )?;
        for tag in &parent.tags {
            tx.execute(
                "INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }
        *counts.entry(day).or_insert(0) += 1;
        child_ids.push(id);
    }

    let decision_id = Uuid::new_v4().to_string();
    let details = serde_json::json!({ "subtasks": child_ids }).to_string();
    tx.execute(
        "INSERT INTO decisions (
            id, task_id, kind, old_due_date, new_due_date, reason, details,
            created_at, undone_at, run_id
         ) VALUES (?1, ?2, 'decomposed', ?3, NULL, ?4, ?5, ?6, NULL, ?7)",
        params![
            decision_id,
            parent_id,
            old_due.map(|d| d.format("%Y-%m-%d").to_string()),
            "split into smaller steps",
            details,
            timestamp,
            run_id,
        ],
    )?;
    tx.commit()?;
    let decision = get_decision(conn, &decision_id)?;
    Ok((decision, child_ids))
}

/// Reverse a decomposition without deleting anything.
///
/// Only the children named in the decision's `details` are cancelled, so a
/// step the user added by hand survives. The parent is reopened for
/// scheduling, its original due date is restored, and `decomposed_at` is
/// cleared so it can be broken down again. The decision is stamped undone
/// rather than removed, preserving the audit trail.
pub fn undo_decomposition(conn: &mut Connection, decision_id: &str) -> Result<()> {
    let decision = get_decision(conn, decision_id)?;
    if decision.kind != DecisionKind::Decomposed {
        return Err(DbError::Invalid(
            "only a decomposed decision can be undone this way".into(),
        ));
    }
    if decision.undone_at.is_some() {
        return Err(DbError::Invalid("decision already undone".into()));
    }
    let subtasks: Vec<String> = decision
        .details
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|value| {
            value.get("subtasks")?.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_default();

    let timestamp = now();
    let tx = conn.transaction()?;
    for child in &subtasks {
        tx.execute(
            "UPDATE tasks SET status = 'cancelled', updated_at = ?2
             WHERE id = ?1 AND parent_task_id = ?3",
            params![child, timestamp, decision.task_id],
        )?;
    }
    tx.execute(
        "UPDATE tasks SET due_date = ?2, decomposed_at = NULL, updated_at = ?3
         WHERE id = ?1",
        params![
            decision.task_id,
            decision
                .old_due_date
                .map(|d| d.format("%Y-%m-%d").to_string()),
            timestamp,
        ],
    )?;
    tx.execute(
        "UPDATE decisions SET undone_at = ?2 WHERE id = ?1",
        params![decision_id, timestamp],
    )?;
    tx.commit()?;
    Ok(())
}

/// Fetch one decision by id.
pub fn get_decision(conn: &Connection, id: &str) -> Result<Decision> {
    conn.query_row(
        "SELECT * FROM decisions WHERE id = ?1",
        [id],
        row_to_decision,
    )
    .optional()?
    .ok_or_else(|| DbError::NotFound(format!("decision {id}")))
}

/// List decisions newest first, optionally filtered by task or run.
pub fn list_decisions(
    conn: &Connection,
    task_id: Option<&str>,
    run_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Decision>> {
    let mut sql = String::from("SELECT * FROM decisions WHERE 1=1");
    let mut values: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(task_id) = task_id {
        sql.push_str(" AND task_id = ?");
        values.push(rusqlite::types::Value::Text(task_id.to_string()));
    }
    if let Some(run_id) = run_id {
        sql.push_str(" AND run_id = ?");
        values.push(rusqlite::types::Value::Text(run_id.to_string()));
    }
    sql.push_str(" ORDER BY created_at DESC, rowid DESC");
    if let Some(limit) = limit {
        sql.push_str(&format!(" LIMIT {limit}"));
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values), row_to_decision)?;
    let mut decisions = Vec::new();
    for row in rows {
        decisions.push(row?);
    }
    Ok(decisions)
}

/// Mark a decision undone. Returns the updated row.
pub fn mark_decision_undone(conn: &Connection, id: &str) -> Result<Decision> {
    let affected = conn.execute(
        "UPDATE decisions SET undone_at = ?2 WHERE id = ?1 AND undone_at IS NULL",
        params![id, now()],
    )?;
    if affected == 0 {
        get_decision(conn, id)?;
        return Err(DbError::Invalid(format!("decision already undone: {id}")));
    }
    get_decision(conn, id)
}

/// Delete one decision. Refuses when a later decision was undone against it.
pub fn delete_decision(conn: &Connection, id: &str) -> Result<bool> {
    let referenced: i64 = conn.query_row(
        "SELECT COUNT(*) FROM decisions WHERE undone_at IS NOT NULL AND task_id = 
           (SELECT task_id FROM decisions WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )?;
    if referenced > 0 {
        return Err(DbError::Invalid(
            "decision is referenced by an undone decision".into(),
        ));
    }
    Ok(conn.execute("DELETE FROM decisions WHERE id = ?1", [id])? > 0)
}

/// Start a secretary run and return its id.
pub fn start_run(conn: &Connection) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO secretary_runs (id, started_at, finished_at, decisions, error)
         VALUES (?1, ?2, NULL, 0, NULL)",
        params![id, now()],
    )?;
    Ok(id)
}

/// Finish a run, recording the decision count or an error.
pub fn finish_run(conn: &Connection, id: &str, decisions: i64, error: Option<&str>) -> Result<()> {
    let affected = conn.execute(
        "UPDATE secretary_runs SET finished_at = ?2, decisions = ?3, error = ?4 WHERE id = ?1",
        params![id, now(), decisions, error],
    )?;
    if affected == 0 {
        return Err(DbError::NotFound(format!("run {id}")));
    }
    Ok(())
}

/// Fetch a run by id.
pub fn get_run(conn: &Connection, id: &str) -> Result<Run> {
    conn.query_row(
        "SELECT * FROM secretary_runs WHERE id = ?1",
        [id],
        row_to_run,
    )
    .optional()?
    .ok_or_else(|| DbError::NotFound(format!("run {id}")))
}

/// The most recent run, if any.
pub fn latest_run(conn: &Connection) -> Result<Option<Run>> {
    Ok(conn
        .query_row(
            "SELECT * FROM secretary_runs ORDER BY started_at DESC, rowid DESC LIMIT 1",
            [],
            row_to_run,
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(title: &str) -> NewTask {
        NewTask {
            title: title.into(),
            ..NewTask::default()
        }
    }

    #[test]
    fn schema_creates_tables() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"tasks".to_string()));
        assert!(names.contains(&"task_tags".to_string()));
        assert!(names.contains(&"decisions".to_string()));
        assert!(names.contains(&"secretary_runs".to_string()));
    }

    #[test]
    fn pinned_and_decomposed_at_round_trip() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "pin me".into(),
                pinned: true,
                ..NewTask::default()
            },
        )
        .unwrap();
        assert!(task.pinned);
        let update = TaskUpdate {
            decomposed_at: Some(Some("2026-01-02T03:04:05Z".into())),
            ..TaskUpdate::default()
        };
        let updated = update_task(&conn, &task.id, &update).unwrap();
        assert_eq!(
            updated.decomposed_at.as_deref(),
            Some("2026-01-02T03:04:05Z")
        );
        assert!(updated.pinned);
    }

    #[test]
    fn complete_sets_last_completed_at_and_reopen_keeps_it() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("finish me")).unwrap();
        // A non-recurring completion yields no next occurrence.
        assert!(complete_task(&mut conn, &task.id).unwrap().is_none());
        let done = get_task(&conn, &task.id).unwrap();
        assert_eq!(done.status, Status::Completed);
        assert!(done.last_completed_at.is_some());
        let stamp = done.last_completed_at.clone();
        let reopened = reopen_task(&conn, &task.id).unwrap();
        assert_eq!(reopened.last_completed_at, stamp);
        assert_eq!(reopened.status, Status::Pending);
    }

    #[test]
    fn decision_round_trip_and_undone() {
        let conn = open_in_memory().unwrap();
        let mut conn = conn;
        let task = add_task(&mut conn, &sample("decide me")).unwrap();
        let run_id = start_run(&conn).unwrap();
        let decision = add_decision(
            &conn,
            &NewDecision {
                task_id: task.id.clone(),
                kind: DecisionKind::Rescheduled,
                old_due_date: NaiveDate::from_ymd_opt(2026, 1, 1),
                new_due_date: NaiveDate::from_ymd_opt(2026, 1, 3),
                reason: "overdue".into(),
                details: None,
                run_id: run_id.clone(),
            },
        )
        .unwrap();
        assert_eq!(decision.kind, DecisionKind::Rescheduled);
        assert!(!decision.is_undone());

        let listed = list_decisions(&conn, Some(&task.id), None, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].old_due_date, NaiveDate::from_ymd_opt(2026, 1, 1));
        assert_eq!(listed[0].new_due_date, NaiveDate::from_ymd_opt(2026, 1, 3));

        let undone = mark_decision_undone(&conn, &decision.id).unwrap();
        assert!(undone.is_undone());
        assert!(mark_decision_undone(&conn, &decision.id).is_err());
    }

    #[test]
    fn decision_requires_existing_task() {
        let conn = open_in_memory().unwrap();
        let run_id = start_run(&conn).unwrap();
        let result = add_decision(
            &conn,
            &NewDecision {
                task_id: "missing".into(),
                kind: DecisionKind::Skipped,
                old_due_date: None,
                new_due_date: None,
                reason: "nope".into(),
                details: None,
                run_id,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn run_lifecycle_and_latest() {
        let conn = open_in_memory().unwrap();
        assert!(latest_run(&conn).unwrap().is_none());
        let id = start_run(&conn).unwrap();
        let started = get_run(&conn, &id).unwrap();
        assert!(started.finished_at.is_none());
        finish_run(&conn, &id, 2, None).unwrap();
        let finished = latest_run(&conn).unwrap().unwrap();
        assert_eq!(finished.id, id);
        assert_eq!(finished.decisions, 2);
        assert!(finished.finished_at.is_some());
        assert!(finished.error.is_none());
        assert!(finish_run(&conn, "missing", 0, None).is_err());

        // A run that fails mid-pass records its error and zero decisions.
        let failed_id = start_run(&conn).unwrap();
        finish_run(&conn, &failed_id, 0, Some("credential missing")).unwrap();
        let failed = get_run(&conn, &failed_id).unwrap();
        assert_eq!(failed.decisions, 0);
        assert_eq!(failed.error.as_deref(), Some("credential missing"));
        assert!(failed.finished_at.is_some());
    }

    #[test]
    fn load_by_day_counts_pending_only_and_skips_empty_days() {
        let mut conn = open_in_memory().unwrap();
        let base = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        for i in 0..3 {
            add_task(
                &mut conn,
                &NewTask {
                    title: format!("same day {i}"),
                    due_date: Some(base),
                    ..NewTask::default()
                },
            )
            .unwrap();
        }
        // A completed task on the same day must not count.
        let done = add_task(
            &mut conn,
            &NewTask {
                title: "already done".into(),
                due_date: Some(base),
                ..NewTask::default()
            },
        )
        .unwrap();
        complete_task(&mut conn, &done.id).unwrap();
        // A task with no due date must not count.
        add_task(&mut conn, &sample("no date")).unwrap();
        // A task outside the range must not count.
        add_task(
            &mut conn,
            &NewTask {
                title: "too far".into(),
                due_date: NaiveDate::from_ymd_opt(2026, 2, 1),
                ..NewTask::default()
            },
        )
        .unwrap();

        let loads =
            load_by_day(&conn, base, NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()).unwrap();
        assert_eq!(loads.get(&base), Some(&3));
        // Days with no pending tasks are absent, not zero.
        assert!(!loads.contains_key(&NaiveDate::from_ymd_opt(2026, 1, 6).unwrap()));
        assert_eq!(loads.len(), 1);
    }

    #[test]
    fn decision_round_trips_every_kind() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("kinds")).unwrap();
        let run_id = start_run(&conn).unwrap();
        let kinds = [
            DecisionKind::Rescheduled,
            DecisionKind::Skipped,
            DecisionKind::Scheduled,
            DecisionKind::Decomposed,
        ];
        for kind in kinds {
            let decision = add_decision(
                &conn,
                &NewDecision {
                    task_id: task.id.clone(),
                    kind,
                    old_due_date: None,
                    new_due_date: None,
                    reason: "test".into(),
                    details: None,
                    run_id: run_id.clone(),
                },
            )
            .unwrap();
            let fetched = get_decision(&conn, &decision.id).unwrap();
            assert_eq!(fetched.kind, kind);
            assert_eq!(fetched.reason, "test");
        }
    }

    #[test]
    fn wal_and_foreign_keys_are_enabled() {
        let conn = open_in_memory().unwrap();
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal.to_lowercase(), "memory");
        assert_eq!(fk, 1);
    }

    #[test]
    fn add_and_get_round_trip() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("buy milk")).unwrap();
        assert_eq!(task.title, "buy milk");
        assert_eq!(task.status, Status::Pending);
        let fetched = get_task(&conn, &task.id).unwrap();
        assert_eq!(fetched, task);
    }

    #[test]
    fn add_stores_tags_and_priority() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "call mom".into(),
                priority: Some(Priority::High),
                tags: vec!["family".into(), "phone".into()],
                ..NewTask::default()
            },
        )
        .unwrap();
        assert_eq!(task.priority, Priority::High);
        assert_eq!(task.tags, vec!["family".to_string(), "phone".to_string()]);
    }

    #[test]
    fn empty_title_is_rejected() {
        let mut conn = open_in_memory().unwrap();
        assert!(add_task(&mut conn, &sample("   ")).is_err());
    }

    #[test]
    fn unknown_id_is_not_found() {
        let conn = open_in_memory().unwrap();
        assert!(matches!(
            get_task(&conn, "missing"),
            Err(DbError::NotFound(_))
        ));
    }

    #[test]
    fn update_changes_fields_and_clears_notification() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("old")).unwrap();
        mark_notified(&conn, std::slice::from_ref(&task.id)).unwrap();
        let updated = update_task(
            &conn,
            &task.id,
            &TaskUpdate {
                title: Some("new".into()),
                due_date: Some(Some(NaiveDate::from_ymd_opt(2026, 11, 1).unwrap())),
                ..TaskUpdate::default()
            },
        )
        .unwrap();
        assert_eq!(updated.title, "new");
        assert_eq!(updated.notified_at, None);
    }

    #[test]
    fn complete_sets_status_and_timestamp() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("finish")).unwrap();
        assert!(complete_task(&mut conn, &task.id).unwrap().is_none());
        let done = get_task(&conn, &task.id).unwrap();
        assert_eq!(done.status, Status::Completed);
        assert!(done.completed_at.is_some());
    }

    #[test]
    fn complete_recurring_creates_next_occurrence() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "water plants".into(),
                due_date: Some(NaiveDate::from_ymd_opt(2026, 1, 31).unwrap()),
                recurrence_pattern: Some(Recurrence::Monthly),
                ..NewTask::default()
            },
        )
        .unwrap();
        let next = complete_task_on(
            &mut conn,
            &task.id,
            NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
        )
        .unwrap()
        .expect("next task");
        assert_eq!(next.due_date, NaiveDate::from_ymd_opt(2026, 2, 28));
        assert_eq!(next.status, Status::Pending);
        assert_eq!(get_task(&conn, &task.id).unwrap().status, Status::Completed);
    }

    #[test]
    fn late_recurring_completion_skips_missed_occurrences() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "weekly review".into(),
                due_date: Some(NaiveDate::from_ymd_opt(2026, 10, 4).unwrap()),
                recurrence_pattern: Some(Recurrence::Weekly),
                ..NewTask::default()
            },
        )
        .unwrap();
        let next = complete_task_on(
            &mut conn,
            &task.id,
            NaiveDate::from_ymd_opt(2026, 10, 25).unwrap(),
        )
        .unwrap()
        .expect("next task");
        // Anchored to completion, not the three-weeks-past due date.
        assert_eq!(next.due_date, NaiveDate::from_ymd_opt(2026, 11, 1));
    }

    #[test]
    fn reopen_clears_completion_and_notification() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("again")).unwrap();
        complete_task(&mut conn, &task.id).unwrap();
        mark_notified(&conn, std::slice::from_ref(&task.id)).unwrap();
        let reopened = reopen_task(&conn, &task.id).unwrap();
        assert_eq!(reopened.status, Status::Pending);
        assert_eq!(reopened.completed_at, None);
        assert_eq!(reopened.notified_at, None);
    }

    #[test]
    fn filters_combine_and_search_matches() {
        let mut conn = open_in_memory().unwrap();
        add_task(
            &mut conn,
            &NewTask {
                title: "fix garage".into(),
                category: Some("home".into()),
                ..NewTask::default()
            },
        )
        .unwrap();
        add_task(
            &mut conn,
            &NewTask {
                title: "buy milk".into(),
                category: Some("errand".into()),
                ..NewTask::default()
            },
        )
        .unwrap();
        let filter = TaskFilter {
            category: Some("home".into()),
            search: Some("garage".into()),
            ..TaskFilter::default()
        };
        let tasks = list_tasks(&conn, &filter).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "fix garage");
    }

    #[test]
    fn stats_counts_each_status() {
        let mut conn = open_in_memory().unwrap();
        let a = add_task(&mut conn, &sample("a")).unwrap();
        add_task(&mut conn, &sample("b")).unwrap();
        complete_task(&mut conn, &a.id).unwrap();
        let s = stats(&conn).unwrap();
        assert_eq!(s.pending, 1);
        assert_eq!(s.completed, 1);
        assert_eq!(s.total, 2);
    }

    #[test]
    fn due_tasks_respects_window_and_status() {
        let mut conn = open_in_memory().unwrap();
        add_task(
            &mut conn,
            &NewTask {
                title: "overdue".into(),
                due_date: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
                ..NewTask::default()
            },
        )
        .unwrap();
        let done = add_task(
            &mut conn,
            &NewTask {
                title: "done".into(),
                due_date: Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
                ..NewTask::default()
            },
        )
        .unwrap();
        complete_task(&mut conn, &done.id).unwrap();
        let tasks = due_tasks(&conn, NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "overdue");
    }

    #[test]
    fn parent_and_subtask_linkage() {
        let mut conn = open_in_memory().unwrap();
        let parent = add_task(&mut conn, &sample("parent")).unwrap();
        let child = add_task(
            &mut conn,
            &NewTask {
                title: "child".into(),
                parent_task_id: Some(parent.id.clone()),
                ..NewTask::default()
            },
        )
        .unwrap();
        let children = list_tasks(
            &conn,
            &TaskFilter {
                parent_task_id: Some(parent.id.clone()),
                ..TaskFilter::default()
            },
        )
        .unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child.id);
        complete_task(&mut conn, &parent.id).unwrap();
        assert_eq!(get_task(&conn, &child.id).unwrap().status, Status::Pending);
    }

    #[test]
    fn concurrent_connections_persist_both_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.db");
        let mut first = open(&path).unwrap();
        let mut second = open(&path).unwrap();
        add_task(&mut first, &sample("first")).unwrap();
        add_task(&mut second, &sample("second")).unwrap();
        let tasks = list_tasks(&first, &TaskFilter::default()).unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn notification_marker_persists_and_resets_on_due_change() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("notify")).unwrap();
        mark_notified(&conn, std::slice::from_ref(&task.id)).unwrap();
        assert!(get_task(&conn, &task.id).unwrap().notified_at.is_some());
        update_task(
            &conn,
            &task.id,
            &TaskUpdate {
                due_date: Some(Some(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap())),
                ..TaskUpdate::default()
            },
        )
        .unwrap();
        assert!(get_task(&conn, &task.id).unwrap().notified_at.is_none());
    }

    #[test]
    fn apply_reschedules_moves_tasks_and_logs_decisions() {
        let mut conn = open_in_memory().unwrap();
        let base = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "move me".into(),
                due_date: Some(base),
                ..NewTask::default()
            },
        )
        .unwrap();
        let run_id = start_run(&conn).unwrap();
        let items = [Reschedule {
            task_id: task.id.clone(),
            from: base,
            to: NaiveDate::from_ymd_opt(2026, 1, 9).unwrap(),
            reason: "overdue".into(),
        }];
        let applied = apply_reschedules(&mut conn, &items, &run_id).unwrap();
        assert_eq!(applied, 1);
        let moved = get_task(&conn, &task.id).unwrap();
        assert_eq!(moved.due_date, NaiveDate::from_ymd_opt(2026, 1, 9));
        let decisions = list_decisions(&conn, Some(&task.id), None, None).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].kind, DecisionKind::Rescheduled);
        assert_eq!(
            decisions[0].new_due_date,
            NaiveDate::from_ymd_opt(2026, 1, 9)
        );
    }

    #[test]
    fn apply_reschedules_rolls_back_the_whole_batch_on_failure() {
        let mut conn = open_in_memory().unwrap();
        let base = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let first = add_task(
            &mut conn,
            &NewTask {
                title: "first".into(),
                due_date: Some(base),
                ..NewTask::default()
            },
        )
        .unwrap();
        let second = add_task(
            &mut conn,
            &NewTask {
                title: "second".into(),
                due_date: Some(base),
                ..NewTask::default()
            },
        )
        .unwrap();
        let run_id = start_run(&conn).unwrap();

        // The second item is stale: its stored due date is base, not Jan 4.
        let items = [
            Reschedule {
                task_id: first.id.clone(),
                from: base,
                to: NaiveDate::from_ymd_opt(2026, 1, 9).unwrap(),
                reason: "overdue".into(),
            },
            Reschedule {
                task_id: second.id.clone(),
                from: NaiveDate::from_ymd_opt(2026, 1, 4).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
                reason: "overdue".into(),
            },
        ];
        let error = apply_reschedules(&mut conn, &items, &run_id).unwrap_err();
        assert!(matches!(error, DbError::Conflict { .. }));

        // The first task must NOT have moved, and no decision may be logged.
        assert_eq!(get_task(&conn, &first.id).unwrap().due_date, Some(base));
        assert_eq!(get_task(&conn, &second.id).unwrap().due_date, Some(base));
        assert!(list_decisions(&conn, None, None, None).unwrap().is_empty());
    }

    #[test]
    fn apply_reschedules_refuses_unknown_task() {
        let mut conn = open_in_memory().unwrap();
        let run_id = start_run(&conn).unwrap();
        let items = [Reschedule {
            task_id: "missing".into(),
            from: NaiveDate::from_ymd_opt(2026, 1, 4).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
            reason: "overdue".into(),
        }];
        assert!(matches!(
            apply_reschedules(&mut conn, &items, &run_id),
            Err(DbError::NotFound(_))
        ));
    }

    #[test]
    fn skipped_decomposition_decision_is_recorded() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(&mut conn, &sample("unsplittable")).unwrap();
        let run_id = start_run(&conn).unwrap();
        add_decision(
            &conn,
            &NewDecision {
                task_id: task.id.clone(),
                kind: DecisionKind::Skipped,
                old_due_date: task.due_date,
                new_due_date: None,
                reason: "title has no natural split".into(),
                details: None,
                run_id,
            },
        )
        .unwrap();
        let decisions = list_decisions(&conn, Some(&task.id), None, None).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].kind, DecisionKind::Skipped);
    }

    #[test]
    fn decomposition_creates_linked_children_and_a_decision() {
        let mut conn = open_in_memory().unwrap();
        let base = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let parent = add_task(
            &mut conn,
            &NewTask {
                title: "big thing".into(),
                due_date: Some(base),
                priority: Some(Priority::High),
                category: Some("home".into()),
                tags: vec!["chore".into(), "weekend".into()],
                ..NewTask::default()
            },
        )
        .unwrap();
        let run = start_run(&conn).unwrap();
        let steps = vec!["step one".to_string(), "step two".to_string()];
        let (decision, children) =
            apply_decomposition(&mut conn, &parent.id, &steps, &run, base, 3, 14).unwrap();
        assert_eq!(children.len(), 2);

        let reloaded = get_task(&conn, &parent.id).unwrap();
        assert_eq!(reloaded.status, Status::Pending);
        assert!(reloaded.due_date.is_none());
        assert!(reloaded.decomposed_at.is_some());

        let kids = list_tasks(
            &conn,
            &TaskFilter {
                parent_task_id: Some(parent.id.clone()),
                ..TaskFilter::default()
            },
        )
        .unwrap();
        assert_eq!(kids.len(), 2);
        for kid in &kids {
            let full = get_task(&conn, &kid.id).unwrap();
            assert_eq!(full.priority, Priority::High);
            assert_eq!(full.category.as_deref(), Some("home"));
            assert_eq!(full.parent_task_id.as_deref(), Some(parent.id.as_str()));
            assert!(full.due_date.is_some());
            let mut tags = full.tags.clone();
            tags.sort();
            assert_eq!(tags, vec!["chore", "weekend"]);
        }

        assert_eq!(decision.kind, DecisionKind::Decomposed);
        assert_eq!(decision.old_due_date, Some(base));
        let details = decision.details.unwrap();
        for id in &children {
            assert!(details.contains(id));
        }

        // The parent stays open and visible in the pending list.
        let pending = list_tasks(
            &conn,
            &TaskFilter {
                status: Some(Status::Pending),
                ..TaskFilter::default()
            },
        )
        .unwrap();
        assert!(pending.iter().any(|task| task.id == parent.id));
    }

    #[test]
    fn undo_decomposition_cancels_only_listed_children() {
        let mut conn = open_in_memory().unwrap();
        let base = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let parent = add_task(
            &mut conn,
            &NewTask {
                title: "big thing".into(),
                due_date: Some(base),
                ..NewTask::default()
            },
        )
        .unwrap();
        let run = start_run(&conn).unwrap();
        let (decision, children) = apply_decomposition(
            &mut conn,
            &parent.id,
            &["one".into(), "two".into()],
            &run,
            base,
            3,
            14,
        )
        .unwrap();

        // A child the user added by hand is not part of the decision.
        let manual = add_task(
            &mut conn,
            &NewTask {
                title: "manual step".into(),
                parent_task_id: Some(parent.id.clone()),
                ..NewTask::default()
            },
        )
        .unwrap();

        undo_decomposition(&mut conn, &decision.id).unwrap();

        // Listed children are cancelled, never deleted.
        for id in &children {
            assert_eq!(get_task(&conn, id).unwrap().status, Status::Cancelled);
        }
        assert_eq!(get_task(&conn, &manual.id).unwrap().status, Status::Pending);

        let reloaded = get_task(&conn, &parent.id).unwrap();
        assert_eq!(reloaded.status, Status::Pending);
        assert!(reloaded.decomposed_at.is_none());
        assert_eq!(reloaded.due_date, Some(base));
        assert!(
            get_decision(&conn, &decision.id)
                .unwrap()
                .undone_at
                .is_some()
        );

        // The parent can be decomposed again after undo.
        let (again, steps) =
            apply_decomposition(&mut conn, &parent.id, &["again".into()], &run, base, 3, 14)
                .unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(again.kind, DecisionKind::Decomposed);
    }

    #[test]
    fn decomposed_parent_is_excluded_from_cap_and_next() {
        let mut conn = open_in_memory().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let parent = add_task(
            &mut conn,
            &NewTask {
                title: "parent".into(),
                due_date: Some(today),
                ..NewTask::default()
            },
        )
        .unwrap();
        update_task(
            &conn,
            &parent.id,
            &TaskUpdate {
                decomposed_at: Some(Some("2026-01-05T00:00:00Z".into())),
                ..TaskUpdate::default()
            },
        )
        .unwrap();

        let loads = load_by_day(&conn, today, today).unwrap();
        assert_eq!(loads.get(&today).copied().unwrap_or(0), 0);
        assert!(next_task(&conn, today).unwrap().is_none());
    }

    #[test]
    fn apply_scheduled_sets_due_date_and_records_decision() {
        let mut conn = open_in_memory().unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "imported later".into(),
                priority: Some(Priority::High),
                ..NewTask::default()
            },
        )
        .unwrap();
        let loaded = list_tasks(
            &conn,
            &TaskFilter {
                status: Some(Status::Pending),
                ..TaskFilter::default()
            },
        )
        .unwrap();
        let candidates = crate::secretary::undated_fill_candidates(&loaded);
        let mut counts = BTreeMap::new();
        let scheduled = crate::secretary::schedule_undated(&candidates, today, &mut counts, 3, 14);
        assert_eq!(scheduled.len(), 1);

        let run = start_run(&conn).unwrap();
        assert_eq!(apply_scheduled(&mut conn, &scheduled, &run).unwrap(), 1);

        let reloaded = get_task(&conn, &task.id).unwrap();
        assert_eq!(reloaded.due_date, Some(today));
        let decisions = list_decisions(&conn, None, None, None).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].kind, DecisionKind::Scheduled);
        assert_eq!(decisions[0].new_due_date, Some(today));
    }

    #[test]
    fn apply_scheduled_refuses_a_manually_dated_task() {
        let mut conn = open_in_memory().unwrap();
        let task = add_task(
            &mut conn,
            &NewTask {
                title: "edited mid run".into(),
                due_date: Some(NaiveDate::from_ymd_opt(2026, 1, 9).unwrap()),
                ..NewTask::default()
            },
        )
        .unwrap();
        let run = start_run(&conn).unwrap();
        let items = vec![Scheduled {
            task_id: task.id.clone(),
            due_date: NaiveDate::from_ymd_opt(2026, 1, 10).unwrap(),
            reason: "filled".into(),
        }];
        assert!(matches!(
            apply_scheduled(&mut conn, &items, &run),
            Err(DbError::Conflict { .. })
        ));
        assert_eq!(
            get_task(&conn, &task.id).unwrap().due_date,
            Some(NaiveDate::from_ymd_opt(2026, 1, 9).unwrap())
        );
    }
}
