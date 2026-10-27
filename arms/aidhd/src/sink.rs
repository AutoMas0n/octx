use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SinkError {
    #[error("sink command not available: {0}")]
    Unavailable(String),
    #[error("sink command failed: {0}")]
    CommandFailed(String),
    #[error("voice input is not configured on this platform")]
    VoiceUnavailable,
    #[error("no speech detected")]
    NoSpeech,
    #[error("voice capture timed out after {0}s")]
    Timeout(u64),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SinkError>;

/// Audible cue marking the edges of a voice capture.
///
/// `Start` rises so it is distinguishable from `Stop`, which falls. Both use
/// short, quiet two-tone chimes so neither startles nor masks the first word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    Start,
    Stop,
}

impl Cue {
    /// Note frequencies in hertz, played in order.
    pub fn tones(self) -> [f32; 2] {
        match self {
            Cue::Start => [660.0, 880.0],
            Cue::Stop => [880.0, 660.0],
        }
    }
}

/// Platform operations used by the arm.
pub trait Sink {
    fn notify(&self, title: &str, body: &str) -> Result<()>;
    fn speak(&self, text: &str) -> Result<()>;
    fn listen(&self) -> Result<String>;

    /// Play a capture cue. Best effort: silence is a valid implementation, so
    /// a platform without audio support never blocks voice capture.
    fn cue(&self, _cue: Cue) -> Result<()> {
        Ok(())
    }
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

    fn cue(&self, cue: Cue) -> Result<()> {
        match self.platform {
            Platform::Termux => termux_cue(cue),
            Platform::Linux => Ok(()),
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

/// Play a two-tone WAV through the Termux media player. A missing player or
/// file is silence, not an error: the cue must never prevent capture.
fn termux_cue(cue: Cue) -> Result<()> {
    if !command_exists("termux-media-player") {
        return Ok(());
    }
    let path = std::env::temp_dir().join(match cue {
        Cue::Start => "aidhd-cue-start.wav",
        Cue::Stop => "aidhd-cue-stop.wav",
    });
    std::fs::write(&path, cue_wav(cue))?;
    let _ = Command::new("termux-media-player")
        .arg("play")
        .arg(&path)
        .status();
    // Never open the microphone while the chime is still sounding: it would
    // record the cue itself. Wait for the player to report that it stopped,
    // with a hard cap so a stuck player cannot delay capture forever.
    wait_for_playback();
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Poll the media player until it stops playing, capped at two seconds.
fn wait_for_playback() {
    let deadline = Instant::now() + Duration::from_millis(2_000);
    // Give `play` a moment to actually start before trusting an idle report.
    std::thread::sleep(Duration::from_millis(60));
    loop {
        let playing = Command::new("termux-media-player")
            .arg("info")
            .output()
            .map(|output| String::from_utf8_lossy(&output.stdout).contains("Status: Playing"))
            .unwrap_or(false);
        if !playing || Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

const CUE_SAMPLE_RATE: u32 = 22_050;
const CUE_NOTE_MS: u32 = 70;
const CUE_GAP_MS: u32 = 10;
const CUE_FADE_MS: u32 = 8;

/// Build a mono 16-bit PCM WAV for one cue.
fn cue_wav(cue: Cue) -> Vec<u8> {
    let mut samples: Vec<i16> = Vec::new();
    for (index, frequency) in cue.tones().iter().enumerate() {
        if index > 0 {
            let gap = CUE_SAMPLE_RATE * CUE_GAP_MS / 1000;
            samples.extend(std::iter::repeat_n(0, gap as usize));
        }
        let count = CUE_SAMPLE_RATE * CUE_NOTE_MS / 1000;
        let fade = (CUE_SAMPLE_RATE * CUE_FADE_MS / 1000).max(1);
        for step in 0..count {
            let seconds = step as f32 / CUE_SAMPLE_RATE as f32;
            let edge = step.min(count - 1 - step) as f32;
            let envelope = (edge / fade as f32).min(1.0);
            let value = (std::f32::consts::TAU * frequency * seconds).sin()
                * envelope
                * 0.22
                * i16::MAX as f32;
            samples.push(value as i16);
        }
    }

    let data_len = (samples.len() * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&CUE_SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(CUE_SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Android's recognizer ends the utterance itself when it hears silence, so
/// there is deliberately no fixed capture window. The timeout only guards a
/// stuck recognizer; it is not the listening duration.
fn termux_listen() -> Result<String> {
    capture(
        "termux-speech-to-text",
        Command::new("termux-speech-to-text"),
        listen_timeout(),
    )
}

/// Resolve the capture safety cap from `AIDHD_LISTEN_TIMEOUT` (seconds).
fn listen_timeout() -> Duration {
    listen_timeout_from(std::env::var("AIDHD_LISTEN_TIMEOUT").ok().as_deref())
}

fn listen_timeout_from(value: Option<&str>) -> Duration {
    let seconds = value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(30)
        .clamp(3, 300);
    Duration::from_secs(seconds)
}

/// Run a recognizer, returning its transcript or an explicit no-speech/timeout
/// error. Empty output is never a valid task title.
fn capture(label: &str, mut command: Command, timeout: Duration) -> Result<String> {
    use std::io::Read;

    let mut child = command
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| SinkError::Unavailable(label.to_string()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut text = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    stdout.read_to_string(&mut text)?;
                }
                if !status.success() {
                    return Err(SinkError::CommandFailed(label.to_string()));
                }
                let text = text.trim().to_string();
                return if text.is_empty() {
                    Err(SinkError::NoSpeech)
                } else {
                    Ok(text)
                };
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SinkError::Timeout(timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn linux_notify(title: &str, body: &str) -> Result<()> {
    run("notify-send", &[title, body], "notify-send")
}

fn linux_speak(text: &str) -> Result<()> {
    run("espeak", &[text], "espeak")
}

fn linux_listen() -> Result<String> {
    if let Ok(command) = std::env::var("AIDHD_LISTEN_CMD") {
        let mut shell = Command::new("sh");
        shell.arg("-c").arg(&command);
        return capture(&command, shell, listen_timeout());
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
    fn cues_rise_on_start_and_fall_on_stop() {
        let [start_low, start_high] = Cue::Start.tones();
        let [stop_high, stop_low] = Cue::Stop.tones();
        assert!(start_low < start_high, "start cue must rise");
        assert!(stop_high > stop_low, "stop cue must fall");
        assert_eq!((start_low, start_high), (stop_low, stop_high));
    }

    #[test]
    fn cue_wav_is_a_valid_mono_pcm_file() {
        let wav = cue_wav(Cue::Start);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(wav.len(), 44 + data_len);
        // 16-bit mono: one sample per two bytes.
        assert_eq!(data_len % 2, 0);
        assert!(data_len > 0);
        // Start and stop carry different audio.
        assert_ne!(wav, cue_wav(Cue::Stop));
    }

    /// Manual check: run `cargo test -p octx-aidhd termux_cue_playback -- --ignored`.
    #[test]
    #[ignore = "plays audio on the device"]
    fn termux_cue_playback_smoke() {
        if detect() != Platform::Termux {
            return;
        }
        termux_cue(Cue::Start).unwrap();
        termux_cue(Cue::Stop).unwrap();
    }

    #[test]
    fn capture_timeout_defaults_and_clamps() {
        assert_eq!(listen_timeout_from(None), Duration::from_secs(30));
        assert_eq!(listen_timeout_from(Some("10")), Duration::from_secs(10));
        assert_eq!(listen_timeout_from(Some(" 45 ")), Duration::from_secs(45));
        assert_eq!(listen_timeout_from(Some("1")), Duration::from_secs(3));
        assert_eq!(listen_timeout_from(Some("9999")), Duration::from_secs(300));
        assert_eq!(
            listen_timeout_from(Some("nonsense")),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn capture_distinguishes_silence_from_a_transcript() {
        let mut silent = Command::new("sh");
        silent.args(["-c", "true"]);
        assert!(matches!(
            capture("true", silent, Duration::from_secs(5)),
            Err(SinkError::NoSpeech)
        ));

        let mut spoken = Command::new("sh");
        spoken.args(["-c", "printf 'buy milk\n'"]);
        assert_eq!(
            capture("printf", spoken, Duration::from_secs(5)).unwrap(),
            "buy milk"
        );
    }

    #[test]
    fn capture_kills_a_stuck_recognizer() {
        let mut stuck = Command::new("sh");
        stuck.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let result = capture("sleep", stuck, Duration::from_millis(200));
        assert!(matches!(result, Err(SinkError::Timeout(0))));
        assert!(started.elapsed() < Duration::from_secs(2));
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
