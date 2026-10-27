use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Local};
use octx_aidhd::db;
use octx_aidhd::model::NewTask;
use octx_aidhd::web;

fn temp_db(dir: &Path) -> PathBuf {
    dir.join("tasks.db")
}

fn start_server(db_path: PathBuf) -> u16 {
    let server = web::bind(0).expect("binds");
    let port = server.server_addr().to_ip().expect("ip").port();
    thread::spawn(move || {
        let _ = web::serve_on(server, db_path);
    });
    for _ in 0..200 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return port;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("server did not start");
}

fn request(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connects");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n");
    if let Some(body) = body {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        ));
    } else {
        req.push_str("\r\n");
    }
    stream.write_all(req.as_bytes()).expect("writes");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("reads");
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    let status = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    let content_type = head
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
        .map(|line| line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string())
        .unwrap_or_default();
    (status, content_type, body.to_string())
}

#[test]
fn serves_index_html() {
    let dir = tempfile::tempdir().unwrap();
    let port = start_server(temp_db(dir.path()));
    let (status, content_type, body) = request(port, "GET", "/", None);
    assert_eq!(status, 200);
    assert!(content_type.contains("text/html"));
    assert!(body.contains("aidhd dashboard"));
    assert!(body.contains("add-form"));
}

#[test]
fn read_endpoints_return_json() {
    let dir = tempfile::tempdir().unwrap();
    let port = start_server(temp_db(dir.path()));
    let (status, content_type, body) = request(port, "GET", "/api/stats", None);
    assert_eq!(status, 200);
    assert!(content_type.contains("application/json"));
    assert!(body.contains("pending"));

    let (status, content_type, body) = request(port, "GET", "/api/next", None);
    assert_eq!(status, 200);
    assert!(content_type.contains("application/json"));
    assert_eq!(body.trim(), "null");

    let (status, content_type, body) = request(port, "GET", "/api/tasks", None);
    assert_eq!(status, 200);
    assert!(content_type.contains("application/json"));
    assert_eq!(body.trim(), "[]");

    let (status, content_type, body) = request(port, "GET", "/api/decisions", None);
    assert_eq!(status, 200);
    assert!(content_type.contains("application/json"));
    assert_eq!(body.trim(), "[]");
}

#[test]
fn post_task_and_mark_done() {
    let dir = tempfile::tempdir().unwrap();
    let port = start_server(temp_db(dir.path()));
    let (status, content_type, body) = request(port, "POST", "/api/tasks", Some(r#"{"title":"write tests"}"#));
    assert_eq!(status, 201);
    assert!(content_type.contains("application/json"));
    let task: serde_json::Value = serde_json::from_str(&body).unwrap();
    let id = task["id"].as_str().unwrap().to_string();

    let (status, content_type, _body) = request(port, "POST", &format!("/api/tasks/{id}/done"), None);
    assert_eq!(status, 200);
    assert!(content_type.contains("application/json"));

    let (_status, _content_type, body) = request(port, "GET", "/api/tasks", None);
    let tasks: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(tasks.as_array().unwrap().len(), 0);
}

#[test]
fn post_bad_body_returns_json_error() {
    let dir = tempfile::tempdir().unwrap();
    let port = start_server(temp_db(dir.path()));
    let (status, content_type, body) = request(port, "POST", "/api/tasks", Some(r#"{"title":""}"#));
    assert_eq!(status, 400);
    assert!(content_type.contains("application/json"));
    assert!(body.contains("error"));
}

#[test]
fn unknown_route_returns_json_error() {
    let dir = tempfile::tempdir().unwrap();
    let port = start_server(temp_db(dir.path()));
    let (status, content_type, body) = request(port, "GET", "/api/nope", None);
    assert_eq!(status, 404);
    assert!(content_type.contains("application/json"));
    assert!(body.contains("error"));
}

#[test]
fn dry_run_secretary_leaves_data_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = temp_db(dir.path());
    let yesterday = Local::now().date_naive() - ChronoDuration::days(1);
    {
        let mut conn = db::open(&db_path).unwrap();
        let new = NewTask {
            title: "overdue".into(),
            due_date: Some(yesterday),
            ..NewTask::default()
        };
        db::add_task(&mut conn, &new).unwrap();
    }
    let port = start_server(db_path.clone());
    let (status, _content_type, body) = request(port, "POST", "/api/secretary", Some(r#"{"dry_run":true}"#));
    assert_eq!(status, 200);
    let result: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(result["rescheduled"].as_u64().unwrap_or(0), 1);
    assert!(!result["decisions"].as_array().unwrap().is_empty());

    let conn = db::open(&db_path).unwrap();
    let tasks = db::list_tasks(&conn, &Default::default()).unwrap();
    assert_eq!(tasks[0].due_date, Some(yesterday));
}

#[test]
fn internal_failure_returns_json_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad_db = dir.path().join("not-a-file");
    std::fs::create_dir(&bad_db).unwrap();
    let port = start_server(bad_db);
    let (status, content_type, body) = request(port, "GET", "/api/stats", None);
    assert_eq!(status, 500);
    assert!(content_type.contains("application/json"));
    assert!(body.contains("error"));
}
