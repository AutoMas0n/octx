//! Endpoints, timings, and the test hooks that point them somewhere else.

use std::path::PathBuf;
use std::time::Duration;

/// Everything about the outside world the arm needs to be told.
///
/// Credentials are deliberately absent: those come from the shared store by
/// host, never from here.
#[derive(Clone, Debug)]
pub struct Config {
    /// ListenBrainz API root (the `/1` API).
    pub lb_api: String,
    /// The player endpoint that expands recording MBIDs into JSPF. It sits
    /// outside the API root, at the site level.
    pub lb_player: String,
    /// Last.fm API root.
    pub lastfm_api: String,
    /// Qobuz API root.
    pub qobuz_api: String,
    /// Minimum spacing between ListenBrainz requests: the service asks for no
    /// more than one per second.
    pub min_interval: Duration,
    /// Minimum spacing for services that state no guidance (Qobuz, Last.fm).
    /// The one-second rule is ListenBrainz's, not a property of HTTP.
    pub other_interval: Duration,
    /// First backoff delay; doubled per retry.
    pub backoff: Duration,
    /// How many times a `429` is retried before the run fails.
    pub max_retries: u32,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Where the credential store lives, when overridden for a test.
    pub store_dir: Option<PathBuf>,
    /// Where the discovery memory lives. Defaults to `{data_dir}/octx/jam`, the
    /// octx data directory's per-arm subdirectory. It is deliberately outside
    /// the release-owned `storage/` mirror, which sync replaces wholesale.
    pub state_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lb_api: "https://api.listenbrainz.org".to_string(),
            lb_player: "https://listenbrainz.org/player".to_string(),
            lastfm_api: "https://ws.audioscrobbler.com/2.0/".to_string(),
            qobuz_api: "https://www.qobuz.com/api.json/0.2".to_string(),
            // ListenBrainz asks for at most one request per second.
            min_interval: Duration::from_secs(1),
            other_interval: Duration::from_millis(250),
            backoff: Duration::from_secs(2),
            max_retries: 3,
            timeout: Duration::from_secs(30),
            store_dir: None,
            state_dir: default_state_dir(),
        }
    }
}

/// The default discovery memory directory: `{data_dir}/octx/jam`.
///
/// Falls back to a relative path when the platform's data directory cannot be
/// determined, so constructing a `Config` never fails.
fn default_state_dir() -> PathBuf {
    dirs::data_dir()
        .map(|dir| dir.join("octx").join("jam"))
        .unwrap_or_else(|| PathBuf::from("octx").join("jam"))
}

impl Config {
    /// Apply the test hooks.
    ///
    /// These point the arm at a mock service and a scratch credential store so
    /// the integration tests can drive the real binary. They carry no secret:
    /// a credential value is still read only from the store, by host.
    #[must_use]
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        let set = |key: &str, slot: &mut String| {
            if let Ok(value) = std::env::var(key)
                && !value.is_empty()
            {
                *slot = value;
            }
        };
        set("OCTX_JAM_LB_API", &mut cfg.lb_api);
        set("OCTX_JAM_LB_PLAYER", &mut cfg.lb_player);
        set("OCTX_JAM_LASTFM_API", &mut cfg.lastfm_api);
        set("OCTX_JAM_QOBUZ_API", &mut cfg.qobuz_api);

        if let Ok(ms) = std::env::var("OCTX_JAM_INTERVAL_MS")
            && let Ok(ms) = ms.parse::<u64>()
        {
            cfg.min_interval = Duration::from_millis(ms);
        }
        if let Ok(ms) = std::env::var("OCTX_JAM_OTHER_INTERVAL_MS")
            && let Ok(ms) = ms.parse::<u64>()
        {
            cfg.other_interval = Duration::from_millis(ms);
        }
        if let Ok(ms) = std::env::var("OCTX_JAM_BACKOFF_MS")
            && let Ok(ms) = ms.parse::<u64>()
        {
            cfg.backoff = Duration::from_millis(ms);
        }
        if let Ok(n) = std::env::var("OCTX_JAM_MAX_RETRIES")
            && let Ok(n) = n.parse::<u32>()
        {
            cfg.max_retries = n;
        }
        if let Ok(dir) = std::env::var("OCTX_JAM_STORE_DIR")
            && !dir.is_empty()
        {
            cfg.store_dir = Some(PathBuf::from(dir));
        }
        if let Ok(dir) = std::env::var("OCTX_JAM_STATE_DIR")
            && !dir.is_empty()
        {
            cfg.state_dir = PathBuf::from(dir);
        }
        cfg
    }

    /// Join a path onto the ListenBrainz API root.
    #[must_use]
    pub fn lb(&self, path: &str) -> String {
        format!("{}{path}", self.lb_api)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_state_dir_ends_with_octx_jam() {
        let dir = default_state_dir();
        assert!(
            dir.ends_with("octx/jam"),
            "state dir should end with `octx/jam`: {dir:?}"
        );
    }

    #[test]
    fn the_state_dir_override_wins() {
        // A scratch value that cannot collide with a real path.
        let scratch = "/tmp/octx-jam-state-override-test";
        // SAFETY: this is the only test that touches this variable, and it
        // restores it before returning.
        unsafe { std::env::set_var("OCTX_JAM_STATE_DIR", scratch) };
        let cfg = Config::from_env();
        unsafe { std::env::remove_var("OCTX_JAM_STATE_DIR") };
        assert_eq!(cfg.state_dir, PathBuf::from(scratch));
    }
}
