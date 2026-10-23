//! A tiny HTTP server for the integration tests.
//!
//! The tests drive the real binary, so the only way to observe what it asked
//! for is to give it something to ask. This speaks just enough HTTP/1.1 to
//! answer `reqwest`, records every request it receives, and answers with
//! whatever the test tells it to — which is what makes the assertions here
//! about the arm rather than about a mock.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// One request the arm made.
#[derive(Debug, Clone)]
pub struct Request {
    /// Method, uppercase.
    pub method: String,
    /// Path including any query string.
    pub path: String,
    /// Headers, in the order they arrived.
    pub headers: Vec<(String, String)>,
    /// Request body.
    pub body: String,
    /// When it arrived, for spacing assertions.
    pub at: Instant,
}

impl Request {
    /// The path without its query string.
    #[must_use]
    pub fn route(&self) -> &str {
        self.path.split('?').next().unwrap_or(&self.path)
    }

    /// A query parameter, percent-decoded.
    #[must_use]
    pub fn query(&self, key: &str) -> Option<String> {
        let query = self.path.split_once('?')?.1;
        for pair in query.split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            if name == key {
                return Some(decode(value));
            }
        }
        None
    }

    /// A header value, by case-insensitive name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The JSON body, parsed.
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// What the server answers with.
#[derive(Debug, Clone)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// `Content-Type`.
    pub content_type: String,
    /// Body.
    pub body: String,
}

impl Response {
    /// A `200` with a JSON body.
    pub fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            content_type: "application/json".to_string(),
            body: body.into(),
        }
    }

    /// A response with an explicit status.
    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "application/json".to_string(),
            body: body.into(),
        }
    }

    /// A `204 No Content`.
    pub fn no_content() -> Self {
        Self {
            status: 204,
            content_type: "text/plain".to_string(),
            body: String::new(),
        }
    }
}

/// A running mock server.
pub struct Mock {
    /// Base URL to point the arm at.
    pub base: String,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Mock {
    /// Start a server that answers every request with `handler`.
    ///
    /// The listener thread outlives the test on purpose: a test binary has
    /// nothing to gain from joining a thread blocked in `accept`.
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(&Request) -> Response + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a test port");
        let port = listener.local_addr().expect("local addr").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(request) = read_request(&mut stream) else {
                    continue;
                };
                let response = handler(&request);
                recorded.lock().expect("requests lock").push(request);
                write_response(&mut stream, &response);
            }
        });

        Self {
            base: format!("http://127.0.0.1:{port}"),
            requests,
        }
    }

    /// Every request received so far, in order.
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("requests lock").clone()
    }

    /// How many requests have been received.
    #[must_use]
    pub fn count(&self) -> usize {
        self.requests.lock().expect("requests lock").len()
    }

    /// The requests whose route is `route`.
    #[must_use]
    pub fn route(&self, route: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|request| request.route() == route)
            .collect()
    }
}

/// Read one request: the request line, headers, and a body if there is one.
fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);

    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if line.trim().is_empty() {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body).ok()?;
    }

    Some(Request {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
        at: Instant::now(),
    })
}

/// Write one response and close the connection.
fn write_response(stream: &mut TcpStream, response: &Response) {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason(response.status),
        response.content_type,
        response.body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(response.body.as_bytes());
    let _ = stream.flush();
}

/// The reason phrase for a status we use.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// Percent-decode a query value.
fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A mock that fails the test when anything reaches it.
pub fn no_requests() -> Mock {
    Mock::start(|request| Response::status(500, format!("unexpected request to {}", request.path)))
}

/// A scratch credential store holding one `jam` document.
pub struct Store {
    /// The directory the arm is pointed at.
    pub dir: tempfile::TempDir,
}

impl Store {
    /// Create a store with the given document, or an empty one when `None`.
    pub fn with_document(document: Option<&str>) -> Self {
        let dir = tempfile::tempdir().expect("store dir");
        if let Some(document) = document {
            let store = octx_creds::Store::open(dir.path(), octx_creds::machine_id().expect("id"));
            store.add("jam", document).expect("store the document");
        }
        Self { dir }
    }

    /// The directory as a string, for the child process.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }
}

/// A JSON account document for the tests.
#[must_use]
pub fn document(accounts: &str, default: Option<&str>) -> String {
    let default = default
        .map(|name| format!("\"default\": \"{name}\", "))
        .unwrap_or_default();
    format!(
        "{{\"version\": 1, {default}\"lastfm\": {{\"api_key\": \"shared-app-key\"}}, \
         \"accounts\": {{{accounts}}}}}"
    )
}

/// Run the arm with a mock service and a scratch store.
pub fn jam(store: &Store, endpoints: &[(&str, String)], args: &[&str]) -> Output {
    jam_with_env(store, endpoints, &[], args)
}

/// Run the arm with extra environment, applied after the defaults so a test
/// can override them (a longer throttle interval, for instance).
pub fn jam_with_env(
    store: &Store,
    endpoints: &[(&str, String)],
    env: &[(&str, &str)],
    args: &[&str],
) -> Output {
    let mut command = command(store, endpoints, env, args);
    let output = command.output().expect("run jam");
    Output::from(output)
}

/// Run the arm with `input` on standard input.
pub fn jam_stdin(
    store: &Store,
    endpoints: &[(&str, String)],
    args: &[&str],
    input: &str,
) -> Output {
    let mut command = command(store, endpoints, &[], args);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().expect("spawn jam");
    {
        let mut stdin = child.stdin.take().expect("stdin");
        stdin.write_all(input.as_bytes()).expect("write stdin");
    }
    Output::from(child.wait_with_output().expect("wait for jam"))
}

/// The command with the test environment applied.
fn command(
    store: &Store,
    endpoints: &[(&str, String)],
    env: &[(&str, &str)],
    args: &[&str],
) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_jam"));
    command.env("OCTX_JAM_STORE_DIR", store.path());
    // Keep the discovery memory inside the scratch store, so a test that
    // delivers a playlist never writes into the developer's real state dir.
    // A test that needs its own location overrides this through `env` below.
    command.env("OCTX_JAM_STATE_DIR", store.path().join("state"));
    command.env("OCTX_JAM_INTERVAL_MS", "0");
    command.env("OCTX_JAM_BACKOFF_MS", "10");
    for (key, value) in endpoints {
        command.env(key, value);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    command.args(args);
    command
}

/// Write NDJSON to a file the arm can read.
pub fn ndjson(contents: &str) -> tempfile::NamedTempFile {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().expect("input file");
    file.write_all(contents.as_bytes()).expect("write input");
    file.flush().expect("flush input");
    file
}

/// What a run produced.
#[derive(Debug)]
pub struct Output {
    /// Exit code.
    pub status: Option<i32>,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl From<std::process::Output> for Output {
    fn from(output: std::process::Output) -> Self {
        Self {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Output {
    /// Assert the run succeeded.
    pub fn ok(&self) -> &Self {
        assert_eq!(
            self.status,
            Some(0),
            "expected success\nstdout: {}\nstderr: {}",
            self.stdout,
            self.stderr
        );
        self
    }

    /// Assert the run failed.
    pub fn failed(&self) -> &Self {
        assert_ne!(
            self.status,
            Some(0),
            "expected failure\nstdout: {}\nstderr: {}",
            self.stdout,
            self.stderr
        );
        self
    }

    /// The stdout parsed as JSON.
    #[must_use]
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|error| {
            panic!("stdout is not JSON ({error}):\n{}", self.stdout);
        })
    }

    /// Assert that neither stream contains `needle`.
    pub fn asserts_absent(&self, needle: &str) -> &Self {
        assert!(
            !self.stdout.contains(needle),
            "`{needle}` appeared on stdout:\n{}",
            self.stdout
        );
        assert!(
            !self.stderr.contains(needle),
            "`{needle}` appeared on stderr:\n{}",
            self.stderr
        );
        self
    }

    /// Assert that stderr contains `needle`.
    pub fn stderr_has(&self, needle: &str) -> &Self {
        assert!(
            self.stderr.contains(needle),
            "stderr does not contain `{needle}`:\n{}",
            self.stderr
        );
        self
    }
}
