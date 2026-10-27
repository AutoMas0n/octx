//! Loopback-only dashboard and JSON API.
//!
//! The server deliberately binds `127.0.0.1` and offers no authentication: it
//! is a local convenience layer over the same database the CLI uses. Every
//! request opens its own connection, so the CLI can keep running concurrently.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use chrono::{Local, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::config;
use crate::db;
use crate::model::{NewTask, Priority, Status, TaskFilter};

type WebResponse = Response<std::io::Cursor<Vec<u8>>>;

/// Serve the dashboard on `127.0.0.1:<port>` until the process is stopped.
pub fn serve(port: u16) -> anyhow::Result<()> {
    let db_path = db::default_db_path()?;
    let server = bind(port)?;
    let address = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("web server is not bound to an IP address"))?;
    eprintln!("aidhd web listening on http://{address}");
    serve_on(server, db_path)
}

/// Bind a loopback server. Exposed so tests can use an ephemeral port.
pub fn bind(port: u16) -> anyhow::Result<Arc<Server>> {
    let server = Server::http(("127.0.0.1", port))
        .map_err(|error| anyhow::anyhow!("could not bind 127.0.0.1:{port}: {error}"))?;
    Ok(Arc::new(server))
}

/// Accept requests on an already-bound server. Exposed for tests.
pub fn serve_on(server: Arc<Server>, db_path: PathBuf) -> anyhow::Result<()> {
    let db_path = Arc::new(db_path);
    for mut request in server.incoming_requests() {
        let db_path = Arc::clone(&db_path);
        thread::spawn(move || {
            let response = match handle(&mut request, &db_path) {
                Ok(response) => response,
                Err(error) => error.response(),
            };
            if let Err(error) = request.respond(response) {
                eprintln!("aidhd web: could not send response: {error}");
            }
        });
    }
    Ok(())
}

struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: 404,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            message: message.into(),
        }
    }

    fn response(self) -> WebResponse {
        json_response(self.status, &json!({ "error": self.message })).unwrap_or_else(|_| {
            Response::from_string("internal server error").with_status_code(500)
        })
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl From<db::DbError> for ApiError {
    fn from(error: db::DbError) -> Self {
        let message = error.to_string();
        match error {
            db::DbError::NotFound(_) => Self::not_found(message),
            db::DbError::Sqlite(_) | db::DbError::Io(_) => Self::internal(message),
            _ => Self::bad_request(message),
        }
    }
}

fn content_type(value: &str) -> Result<Header, ApiError> {
    Header::from_bytes("Content-Type", value)
        .map_err(|_| ApiError::internal("could not build Content-Type header"))
}

fn json_response(status: u16, value: &Value) -> Result<WebResponse, ApiError> {
    let body = serde_json::to_string(value)
        .map_err(|error| ApiError::internal(format!("could not serialize response: {error}")))?;
    Ok(Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(content_type("application/json; charset=utf-8")?))
}

fn html_response(body: &'static str) -> Result<WebResponse, ApiError> {
    Ok(Response::from_string(body)
        .with_status_code(StatusCode(200))
        .with_header(content_type("text/html; charset=utf-8")?))
}

fn handle(request: &mut Request, db_path: &Path) -> Result<WebResponse, ApiError> {
    let url = request.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    match request.method() {
        Method::Get => match path {
            "/" => html_response(include_str!("web/index.html")),
            "/api/tasks" => list_tasks(db_path),
            "/api/stats" => stats(db_path),
            "/api/next" => next(db_path),
            "/api/decisions" => decisions(db_path, query),
            _ => Err(ApiError::not_found(format!("unknown route: {path}"))),
        },
        Method::Post => match path {
            "/api/tasks" => add_task(request, db_path),
            "/api/secretary" => secretary(request, db_path),
            _ if path.starts_with("/api/tasks/") && path.ends_with("/done") => {
                complete_task(db_path, path)
            }
            _ => Err(ApiError::not_found(format!("unknown route: {path}"))),
        },
        _ => Err(ApiError {
            status: 405,
            message: "method not allowed".into(),
        }),
    }
}

fn list_tasks(db_path: &Path) -> Result<WebResponse, ApiError> {
    let conn = db::open(db_path)?;
    let filter = TaskFilter {
        status: Some(Status::Pending),
        ..TaskFilter::default()
    };
    let tasks = db::list_tasks(&conn, &filter)?;
    json_response(
        200,
        &serde_json::to_value(tasks).map_err(|error| ApiError::internal(error.to_string()))?,
    )
}

fn stats(db_path: &Path) -> Result<WebResponse, ApiError> {
    let conn = db::open(db_path)?;
    let stats = db::stats(&conn)?;
    json_response(
        200,
        &serde_json::to_value(stats).map_err(|error| ApiError::internal(error.to_string()))?,
    )
}

fn next(db_path: &Path) -> Result<WebResponse, ApiError> {
    let conn = db::open(db_path)?;
    let next = db::next_task(&conn, Local::now().date_naive())?;
    json_response(
        200,
        &serde_json::to_value(next).map_err(|error| ApiError::internal(error.to_string()))?,
    )
}

fn decisions(db_path: &Path, query: &str) -> Result<WebResponse, ApiError> {
    let limit = match query_value(query, "limit") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| ApiError::bad_request("limit must be a non-negative integer"))?,
        None => 20,
    };
    let conn = db::open(db_path)?;
    let decisions = db::list_decisions(&conn, None, None, Some(limit))?;
    json_response(
        200,
        &serde_json::to_value(decisions).map_err(|error| ApiError::internal(error.to_string()))?,
    )
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then_some(value)
    })
}

#[derive(Deserialize)]
struct NewTaskBody {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<Priority>,
    #[serde(default)]
    due_date: Option<NaiveDate>,
    #[serde(default)]
    category: Option<String>,
}

fn add_task(request: &mut Request, db_path: &Path) -> Result<WebResponse, ApiError> {
    let body = read_body(request)?;
    let input: NewTaskBody = serde_json::from_str(&body)
        .map_err(|error| ApiError::bad_request(format!("invalid JSON: {error}")))?;
    let title = input.title.trim();
    if title.is_empty() {
        return Err(ApiError::bad_request("title must not be empty"));
    }
    let new = NewTask {
        title: title.to_string(),
        description: input.description,
        priority: input.priority,
        due_date: input.due_date,
        category: input.category,
        ..NewTask::default()
    };
    let mut conn = db::open(db_path)?;
    let task = db::add_task(&mut conn, &new)?;
    json_response(
        201,
        &serde_json::to_value(task).map_err(|error| ApiError::internal(error.to_string()))?,
    )
}

fn complete_task(db_path: &Path, path: &str) -> Result<WebResponse, ApiError> {
    let id = path
        .strip_prefix("/api/tasks/")
        .and_then(|rest| rest.strip_suffix("/done"))
        .filter(|id| !id.is_empty())
        .ok_or_else(|| ApiError::bad_request("task id is required"))?;
    let mut conn = db::open(db_path)?;
    db::complete_task(&mut conn, id)?;
    let task = db::get_task(&conn, id)?;
    json_response(
        200,
        &serde_json::to_value(task).map_err(|error| ApiError::internal(error.to_string()))?,
    )
}

fn read_body(request: &mut Request) -> Result<String, ApiError> {
    let mut body = String::new();
    request
        .as_reader()
        .read_to_string(&mut body)
        .map_err(|error| ApiError::bad_request(format!("could not read body: {error}")))?;
    Ok(body)
}

fn secretary(request: &mut Request, db_path: &Path) -> Result<WebResponse, ApiError> {
    let body = read_body(request)?;
    let dry_run = if body.trim().is_empty() {
        false
    } else {
        let value: Value = serde_json::from_str(&body)
            .map_err(|error| ApiError::bad_request(format!("invalid JSON: {error}")))?;
        match value.get("dry_run") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err(ApiError::bad_request("dry_run must be a boolean")),
        }
    };
    let (summary, decisions) = if dry_run {
        let temp = std::env::temp_dir().join(format!(
            "aidhd-web-secretary-dry-{}-{}.db",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&temp);
        {
            let conn = db::open(db_path)?;
            conn.execute("VACUUM INTO ?1", [temp.to_string_lossy().to_string()])
                .map_err(db::DbError::from)?;
        }
        let mut copy = db::open(&temp)?;
        let summary = db::run_secretary_pass(
            &mut copy,
            &config::resolve_secretary()?,
            Local::now().date_naive(),
            None,
        )?;
        let run = db::latest_run(&copy)?;
        let decisions = match &run {
            Some(run) => db::list_decisions(&copy, None, Some(&run.id), None)?,
            None => Vec::new(),
        };
        drop(copy);
        let _ = std::fs::remove_file(&temp);
        (summary, decisions)
    } else {
        let mut conn = db::open(db_path)?;
        let summary = db::run_secretary_pass(
            &mut conn,
            &config::resolve_secretary()?,
            Local::now().date_naive(),
            None,
        )?;
        let run = db::latest_run(&conn)?;
        let decisions = match &run {
            Some(run) => db::list_decisions(&conn, None, Some(&run.id), None)?,
            None => Vec::new(),
        };
        (summary, decisions)
    };
    let value = json!({
        "decomposed": summary.decomposed,
        "scheduled": summary.scheduled,
        "rescheduled": summary.rescheduled,
        "skipped": summary.skipped,
        "decisions": decisions,
    });
    json_response(200, &value)
}
