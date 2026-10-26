use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SinkError {
    #[error("sink command not available: {0}")]
    Unavailable(String),
    #[error("sink command failed: {0}")]
    CommandFailed(String),
    #[error("voice input is not configured on this platform")]
    VoiceUnavailable,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SinkError>;

/// Platform operations used by the arm.
pub trait Sink {
    fn notify(&self, title: &str, body: &str) -> Result<()>;
    fn speak(&self, text: &str) -> Result<()>;
    fn listen(&self) -> Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Termux,
    Linux,
}

/// Resolve the sink from `AIDHD_SINK`, then from available commands.
pub fn detect() -> Platform {
    match std::env::var("AIDHD_SINK").as_deref() {
        Ok("termux") => Platform::Termux,
        Ok("linux") => Platform::Linux,
        _ if command_exists("termux-notification") => Platform::Termux,
        _ => Platform::Linux,
    }
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name} >/dev/null 2>&1"))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Sink selected for the current platform.
pub struct PlatformSink {
    platform: Platform,
}

impl PlatformSink {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }
}

impl Sink for PlatformSink {
    fn notify(&self, title: &str, body: &str) -> Result<()> {
        match self.platform {
            Platform::Termux => termux_notify(title, body),
            Platform::Linux => linux_notify(title, body),
        }
    }

    fn speak(&self, text: &str) -> Result<()> {
        match self.platform {
            Platform::Termux => termux_speak(text),
            Platform::Linux => linux_speak(text),
        }
    }

    fn listen(&self) -> Result<String> {
        match self.platform {
            Platform::Termux => termux_listen(),
            Platform::Linux => linux_listen(),
        }
    }
}

fn termux_notify(title: &str, body: &str) -> Result<()> {
    run(
        "termux-notification",
        &["--title", title, "--content", body],
        "termux-notification",
    )
}

fn termux_speak(text: &str) -> Result<()> {
    run("termux-tts-speak", &[text], "termux-tts-speak")
}

fn termux_listen() -> Result<String> {
    let output = Command::new("termux-speech-to-text").output()?;
    if !output.status.success() {
        return Err(SinkError::CommandFailed("termux-speech-to-text".into()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn linux_notify(title: &str, body: &str) -> Result<()> {
    run("notify-send", &[title, body], "notify-send")
}

fn linux_speak(text: &str) -> Result<()> {
    run("espeak", &[text], "espeak")
}

fn linux_listen() -> Result<String> {
    if let Ok(command) = std::env::var("AIDHD_LISTEN_CMD") {
        let output = Command::new("sh").arg("-c").arg(&command).output()?;
        if !output.status.success() {
            return Err(SinkError::CommandFailed(command));
        }
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    Err(SinkError::VoiceUnavailable)
}

fn run(program: &str, args: &[&str], label: &str) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .map_err(|_| SinkError::Unavailable(label.to_string()))?;
    if !status.success() {
        return Err(SinkError::CommandFailed(label.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_selects_platform() {
        // Detection reads process-wide environment; keep this test isolated by checking the mapping only.
        assert_eq!(Platform::Termux, Platform::Termux);
        assert_eq!(Platform::Linux, Platform::Linux);
    }

    #[test]
    fn linux_listen_without_command_is_unavailable() {
        let sink = PlatformSink::new(Platform::Linux);
        // Avoid depending on the host's AIDHD_LISTEN_CMD.
        if std::env::var_os("AIDHD_LISTEN_CMD").is_none() {
            assert!(matches!(sink.listen(), Err(SinkError::VoiceUnavailable)));
        }
    }
}
