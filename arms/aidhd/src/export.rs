use std::io::{self, Write};
use std::path::Path;

use rusqlite::Connection;

use crate::db::{DbError, Result};
use crate::model::{Task, TaskFilter};

/// Write all tasks as a JSON array, including tags. With no path, print to stdout.
pub fn export_json(conn: &Connection, path: Option<&Path>) -> Result<()> {
    let tasks = crate::db::list_tasks(conn, &TaskFilter::default())?;
    let json = serde_json::to_string_pretty(&tasks)?;
    match path {
        Some(path) => std::fs::write(path, json)?,
        None => println!("{json}"),
    }
    Ok(())
}

/// Write all tasks as CSV with a header row. Tags are joined with `;`. With no path, print to stdout.
pub fn export_csv(conn: &Connection, path: Option<&Path>) -> Result<()> {
    let tasks = crate::db::list_tasks(conn, &TaskFilter::default())?;
    match path {
        Some(path) => {
            let mut file = std::fs::File::create(path)?;
            write_csv(&mut file, &tasks)?;
            file.flush()?;
        }
        None => {
            let mut stdout = io::stdout().lock();
            write_csv(&mut stdout, &tasks)?;
            stdout.flush()?;
        }
    }
    Ok(())
}

fn write_csv<W: Write>(writer: &mut W, tasks: &[Task]) -> Result<()> {
    writeln!(
        writer,
        "id,title,description,notes,status,priority,category,due_date,recurrence_pattern,parent_task_id,notified_at,completed_at,created_at,updated_at,tags"
    )?;
    for task in tasks {
        write_csv_row(writer, task)?;
    }
    Ok(())
}

fn write_csv_row<W: Write>(writer: &mut W, task: &Task) -> Result<()> {
    let fields = [
        task.id.clone(),
        task.title.clone(),
        task.description.clone().unwrap_or_default(),
        task.notes.clone().unwrap_or_default(),
        task.status.as_str().to_string(),
        task.priority.as_str().to_string(),
        task.category.clone().unwrap_or_default(),
        task.due_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        task.recurrence_pattern
            .map(|r| r.as_str().to_string())
            .unwrap_or_default(),
        task.parent_task_id.clone().unwrap_or_default(),
        task.notified_at.clone().unwrap_or_default(),
        task.completed_at.clone().unwrap_or_default(),
        task.created_at.clone(),
        task.updated_at.clone(),
        task.tags.join(";"),
    ];
    writeln!(
        writer,
        "{}",
        fields
            .iter()
            .map(|field| escape_csv(field))
            .collect::<Vec<_>>()
            .join(",")
    )?;
    Ok(())
}

fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

impl From<serde_json::Error> for DbError {
    fn from(error: serde_json::Error) -> Self {
        DbError::Invalid(format!("JSON error: {error}"))
    }
}
