//! The tool execution plane: filesystem access and ACP terminals.
//!
//! Filesystem requests are confined to the session working directory; a path
//! that escapes it is rejected and reported back to the agent as an error.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, EnvVariable, KillTerminalResponse,
    ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalResponse, TerminalExitStatus,
    TerminalId, TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};

const READ_CHUNK: usize = 8192;
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Normalize a path lexically (resolving `.` and `..`) without touching the filesystem.
#[must_use]
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve `requested` against `cwd` and reject anything that escapes `cwd`.
///
/// `cwd` is expected to be canonical. Existing targets are canonicalized so a
/// symlink cannot be used to escape; for not-yet-existing targets the parent is
/// canonicalized instead.
pub fn confine(cwd: &Path, requested: &Path) -> Result<PathBuf, String> {
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        cwd.join(requested)
    };
    let normalized = lexical_normalize(&candidate);
    if !normalized.starts_with(cwd) {
        return Err(format!(
            "path `{}` escapes the working directory `{}`",
            requested.display(),
            cwd.display()
        ));
    }
    if let Ok(real) = normalized.canonicalize() {
        if !real.starts_with(cwd) {
            return Err(format!(
                "path `{}` escapes the working directory `{}`",
                requested.display(),
                cwd.display()
            ));
        }
        return Ok(real);
    }
    if let Some(parent) = normalized.parent()
        && let Ok(real) = parent.canonicalize()
        && !real.starts_with(cwd)
    {
        return Err(format!(
            "path `{}` escapes the working directory `{}`",
            requested.display(),
            cwd.display()
        ));
    }
    Ok(normalized)
}

#[derive(Default)]
struct TerminalState {
    text: String,
    truncated: bool,
    exit: Option<(Option<u32>, Option<String>)>,
}

struct Terminal {
    output: StdMutex<TerminalState>,
    done: Notify,
    child: Mutex<Option<Child>>,
    limit: u64,
}

impl Terminal {
    fn new(limit: u64) -> Self {
        Self {
            output: StdMutex::new(TerminalState::default()),
            done: Notify::new(),
            child: Mutex::new(None),
            limit: limit.max(1),
        }
    }

    fn append(&self, bytes: &[u8]) {
        let mut state = self.output.lock().expect("terminal state poisoned");
        if state.truncated {
            return;
        }
        let remaining = self.limit.saturating_sub(state.text.len() as u64) as usize;
        if bytes.len() > remaining {
            state
                .text
                .push_str(&String::from_utf8_lossy(&bytes[..remaining]));
            state.truncated = true;
        } else {
            state.text.push_str(&String::from_utf8_lossy(bytes));
        }
    }

    fn finish(&self, status: std::process::ExitStatus) {
        #[cfg(unix)]
        let signal = {
            use std::os::unix::process::ExitStatusExt as _;
            status.signal().map(|s| s.to_string())
        };
        #[cfg(not(unix))]
        let signal = None;
        {
            let mut state = self.output.lock().expect("terminal state poisoned");
            state.exit = Some((status.code().map(|c| c as u32), signal));
        }
        self.done.notify_waiters();
    }

    fn exit_status(&self) -> Option<TerminalExitStatus> {
        let state = self.output.lock().expect("terminal state poisoned");
        state.exit.as_ref().map(|(code, signal)| {
            let mut status = TerminalExitStatus::new();
            status.exit_code = *code;
            status.signal = signal.clone();
            status
        })
    }

    async fn wait_for_exit(&self) -> TerminalExitStatus {
        loop {
            if let Some(status) = self.exit_status() {
                return status;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

/// Shared tool plane state, one per agent-arm run.
pub struct ToolPlane {
    cwd: PathBuf,
    output_byte_limit: u64,
    counter: AtomicU64,
    terminals: Mutex<HashMap<String, Arc<Terminal>>>,
}

impl ToolPlane {
    /// Create a tool plane rooted at the canonicalized working directory.
    pub fn new(cwd: &Path, output_byte_limit: u64) -> Result<Self, String> {
        let cwd = cwd
            .canonicalize()
            .map_err(|e| format!("cannot resolve working directory `{}`: {e}", cwd.display()))?;
        Ok(Self {
            cwd,
            output_byte_limit,
            counter: AtomicU64::new(0),
            terminals: Mutex::new(HashMap::new()),
        })
    }

    /// The canonical working directory.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    async fn get(&self, id: &str) -> Result<Arc<Terminal>, String> {
        self.terminals
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| format!("unknown terminal `{id}`"))
    }

    /// Read a text file, confined to the working directory.
    pub fn read_file(&self, req: &ReadTextFileRequest) -> Result<ReadTextFileResponse, String> {
        let path = confine(&self.cwd, &req.path)?;
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read `{}`: {e}", path.display()))?;
        let content = match (req.line, req.limit) {
            (None, None) => content,
            (line, limit) => {
                let skip = line.unwrap_or(0) as usize;
                let take = limit.map_or(usize::MAX, |l| l as usize);
                content
                    .lines()
                    .skip(skip)
                    .take(take)
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        Ok(ReadTextFileResponse::new(content))
    }

    /// Write a text file, confined to the working directory.
    pub fn write_file(&self, req: &WriteTextFileRequest) -> Result<WriteTextFileResponse, String> {
        let path = confine(&self.cwd, &req.path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create `{}`: {e}", parent.display()))?;
        }
        std::fs::write(&path, &req.content)
            .map_err(|e| format!("failed to write `{}`: {e}", path.display()))?;
        Ok(WriteTextFileResponse::new())
    }

    /// Spawn a terminal command and return its id.
    pub async fn create_terminal(
        &self,
        req: &CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse, String> {
        let cwd = match &req.cwd {
            Some(dir) => confine(&self.cwd, dir)?,
            None => self.cwd.clone(),
        };
        let mut cmd = Command::new(&req.command);
        cmd.args(&req.args);
        cmd.current_dir(&cwd);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);
        for EnvVariable { name, value, .. } in &req.env {
            cmd.env(name, value);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn `{}`: {e}", req.command))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let limit = req.output_byte_limit.unwrap_or(self.output_byte_limit);
        let terminal = Arc::new(Terminal::new(limit));

        for pipe in [stdout.map(Pipe::Out), stderr.map(Pipe::Err)]
            .into_iter()
            .flatten()
        {
            let terminal = Arc::clone(&terminal);
            tokio::spawn(async move {
                let mut reader = Box::new(pipe);
                let mut buf = vec![0u8; READ_CHUNK];
                loop {
                    match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => terminal.append(&buf[..n]),
                    }
                }
            });
        }

        *terminal.child.lock().await = Some(child);
        let waiter = Arc::clone(&terminal);
        tokio::spawn(async move {
            loop {
                {
                    let mut guard = waiter.child.lock().await;
                    let Some(child) = guard.as_mut() else { break };
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            waiter.finish(status);
                            *guard = None;
                            break;
                        }
                        Ok(None) => {}
                        Err(_) => {
                            *guard = None;
                            break;
                        }
                    }
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        });

        let id = format!("term-{}", self.counter.fetch_add(1, Ordering::Relaxed));
        self.terminals.lock().await.insert(id.clone(), terminal);
        Ok(CreateTerminalResponse::new(TerminalId::new(id)))
    }

    /// Current output and exit status of a terminal.
    pub async fn terminal_output(
        &self,
        req: &TerminalOutputRequest,
    ) -> Result<TerminalOutputResponse, String> {
        let terminal = self.get(&req.terminal_id.0).await?;
        let (text, truncated) = {
            let state = terminal.output.lock().expect("terminal state poisoned");
            (state.text.clone(), state.truncated)
        };
        let mut response = TerminalOutputResponse::new(text, truncated);
        response.exit_status = terminal.exit_status();
        Ok(response)
    }

    /// Wait for a terminal to exit.
    pub async fn wait_for_exit(
        &self,
        req: &WaitForTerminalExitRequest,
    ) -> Result<WaitForTerminalExitResponse, String> {
        let terminal = self.get(&req.terminal_id.0).await?;
        Ok(WaitForTerminalExitResponse::new(
            terminal.wait_for_exit().await,
        ))
    }

    /// Kill a terminal's command.
    pub async fn kill_terminal(&self, id: &str) -> Result<KillTerminalResponse, String> {
        let terminal = self.get(id).await?;
        if let Some(child) = terminal.child.lock().await.as_mut() {
            let _ = child.start_kill();
        }
        Ok(KillTerminalResponse::new())
    }

    /// Release a terminal, killing any running command and forgetting it.
    pub async fn release_terminal(&self, id: &str) -> Result<ReleaseTerminalResponse, String> {
        if let Some(terminal) = self.terminals.lock().await.remove(id)
            && let Some(child) = terminal.child.lock().await.as_mut()
        {
            let _ = child.start_kill();
        }
        Ok(ReleaseTerminalResponse::new())
    }
}

/// A stdout/stderr pipe, so both can be drained by the same loop.
enum Pipe {
    Out(tokio::process::ChildStdout),
    Err(tokio::process::ChildStderr),
}

impl tokio::io::AsyncRead for Pipe {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Pipe::Out(inner) => std::pin::Pin::new(inner).poll_read(cx, buf),
            Pipe::Err(inner) => std::pin::Pin::new(inner).poll_read(cx, buf),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalize_resolves_parent_dir() {
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
    }

    #[test]
    fn confine_rejects_escapes_and_allows_descendants() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().canonicalize().unwrap();
        assert!(confine(&cwd, Path::new("../outside")).is_err());
        assert!(confine(&cwd, Path::new("/etc/passwd")).is_err());
        assert!(confine(&cwd, Path::new("sub/file.txt")).is_ok());
    }
}
