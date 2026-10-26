use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use thiserror::Error;
use uuid::Uuid;

use crate::model::{NewTask, Priority, Recurrence, Stats, Status, Task, TaskFilter, TaskUpdate};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("task not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid stored value: {0}")]
    Invalid(String),
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
    recurrence_pattern TEXT
                      CHECK (recurrence_pattern IS NULL OR recurrence_pattern IN ('daily','weekly','monthly')),
    parent_task_id    TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    notified_at       TEXT,
    completed_at      TEXT,
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
        recurrence_pattern: recurrence.as_deref().and_then(Recurrence::from_str),
        parent_task_id: row.get("parent_task_id")?,
        notified_at: row.get("notified_at")?,
        completed_at: row.get("completed_at")?,
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
            due_date, recurrence_pattern, parent_task_id, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            id,
            title,
            new.description,
            new.notes,
            new.priority.unwrap_or(Priority::Medium).as_str(),
            new.category,
            new.due_date.map(|d| d.format("%Y-%m-%d").to_string()),
            new.recurrence_pattern.map(Recurrence::as_str),
            new.parent_task_id,
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
        "UPDATE tasks SET status = 'completed', completed_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, timestamp],
    )?;

    let next = if let (Some(pattern), Some(_due)) = (task.recurrence_pattern, task.due_date) {
        let next_due = pattern.advance(today);
        let next_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO tasks (
                id, title, description, notes, status, priority, category,
                due_date, recurrence_pattern, parent_task_id, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
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
            "SELECT * FROM tasks WHERE status = 'pending'
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
}
