use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    OpenRouter,
    OpenAi,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::OpenRouter => "openrouter",
            Provider::OpenAi => "openai",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "openrouter" => Some(Provider::OpenRouter),
            "openai" => Some(Provider::OpenAi),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub provider: Provider,
    pub model: String,
    pub base_url: String,
    pub credential_host: String,
    pub thinking: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: Provider::OpenRouter,
            model: "deepseek/deepseek-v4.1-flash".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            credential_host: "openrouter".into(),
            thinking: false,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    credential_host: Option<String>,
    thinking: Option<bool>,
}

/// Path to the optional TOML configuration file.
pub fn config_path() -> anyhow::Result<PathBuf> {
    let base = dirs::data_dir().context("no data directory")?;
    Ok(base.join("octx").join("aidhd").join("config.toml"))
}

/// Resolve configuration from defaults, then the config file, then environment overrides.
pub fn resolve() -> anyhow::Result<AiConfig> {
    resolve_from(&config_path()?)
}

pub fn resolve_from(path: &Path) -> anyhow::Result<AiConfig> {
    let mut config = AiConfig::default();
    if path.exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading AI config {}", path.display()))?;
        let file: FileConfig = toml::from_str(&text)
            .with_context(|| format!("parsing AI config {}", path.display()))?;
        if let Some(provider) = file.provider {
            config.provider = Provider::from_str(&provider)
                .with_context(|| format!("unknown AI provider: {provider}"))?;
            config.credential_host = config.provider.as_str().into();
            config.base_url = match config.provider {
                Provider::OpenRouter => "https://openrouter.ai/api/v1".into(),
                Provider::OpenAi => "https://api.openai.com/v1".into(),
            };
        }
        if let Some(model) = file.model {
            config.model = model;
        }
        if let Some(base_url) = file.base_url {
            config.base_url = base_url;
        }
        if let Some(host) = file.credential_host {
            config.credential_host = host;
        }
        if let Some(thinking) = file.thinking {
            config.thinking = thinking;
        }
    }
    apply_env(&mut config)?;
    Ok(config)
}

fn apply_env(config: &mut AiConfig) -> anyhow::Result<()> {
    if let Ok(provider) = std::env::var("AIDHD_AI_PROVIDER") {
        config.provider = Provider::from_str(&provider)
            .with_context(|| format!("unknown AIDHD_AI_PROVIDER: {provider}"))?;
    }
    if let Ok(model) = std::env::var("AIDHD_AI_MODEL") {
        config.model = model;
    }
    if let Ok(base_url) = std::env::var("AIDHD_AI_BASE_URL") {
        config.base_url = base_url;
    }
    if let Ok(host) = std::env::var("AIDHD_AI_CREDENTIAL_HOST") {
        config.credential_host = host;
    }
    if let Ok(thinking) = std::env::var("AIDHD_AI_THINKING") {
        config.thinking = match thinking.to_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => true,
            "0" | "false" | "off" | "no" => false,
            _ => bail!("invalid AIDHD_AI_THINKING: {thinking}"),
        };
    }
    Ok(())
}

/// Tunables for the secretary pass. Every field has a default and an
/// environment override, so the command works with no configuration at all.
#[derive(Debug, Clone)]
pub struct SecretaryConfig {
    /// Maximum pending tasks allowed on one day.
    pub daily_cap: usize,
    /// How many days ahead rescheduling may search.
    pub horizon_days: i64,
    /// Quiet hours as (start, end) in 24h local time; the range may wrap midnight.
    pub quiet_hours: (u8, u8),
    /// A task overdue longer than this many days counts as stuck.
    pub stuck_after_days: i64,
    /// A task rescheduled this many times without completion counts as stuck.
    pub stuck_after_moves: usize,
    /// Maximum decompositions in a single pass.
    pub max_decompositions: usize,
    /// Expected cadence, used only by the health check.
    pub cadence: String,
    /// Whether tasks added without a due date get scheduled automatically.
    pub auto_schedule: bool,
}

impl Default for SecretaryConfig {
    fn default() -> Self {
        Self {
            daily_cap: 3,
            horizon_days: 14,
            quiet_hours: (22, 7),
            stuck_after_days: 7,
            stuck_after_moves: 3,
            max_decompositions: 2,
            cadence: "daily".into(),
            auto_schedule: true,
        }
    }
}

/// Resolve secretary configuration from defaults, then environment overrides.
pub fn resolve_secretary() -> anyhow::Result<SecretaryConfig> {
    let mut config = SecretaryConfig::default();
    if let Ok(value) = std::env::var("AIDHD_DAILY_CAP") {
        config.daily_cap = value
            .parse()
            .with_context(|| format!("invalid AIDHD_DAILY_CAP: {value}"))?;
    }
    if let Ok(value) = std::env::var("AIDHD_HORIZON_DAYS") {
        config.horizon_days = value
            .parse()
            .with_context(|| format!("invalid AIDHD_HORIZON_DAYS: {value}"))?;
    }
    if let Ok(value) = std::env::var("AIDHD_QUIET_HOURS") {
        let (start, end) = value
            .split_once('-')
            .with_context(|| format!("AIDHD_QUIET_HOURS must be HH-HH, got {value}"))?;
        let start: u8 = start
            .parse()
            .with_context(|| format!("invalid AIDHD_QUIET_HOURS start: {start}"))?;
        let end: u8 = end
            .parse()
            .with_context(|| format!("invalid AIDHD_QUIET_HOURS end: {end}"))?;
        if start > 23 || end > 23 {
            bail!("AIDHD_QUIET_HOURS hours must be 0-23, got {start}-{end}");
        }
        config.quiet_hours = (start, end);
    }
    if let Ok(value) = std::env::var("AIDHD_STUCK_AFTER_DAYS") {
        config.stuck_after_days = value
            .parse()
            .with_context(|| format!("invalid AIDHD_STUCK_AFTER_DAYS: {value}"))?;
    }
    if let Ok(value) = std::env::var("AIDHD_STUCK_AFTER_MOVES") {
        config.stuck_after_moves = value
            .parse()
            .with_context(|| format!("invalid AIDHD_STUCK_AFTER_MOVES: {value}"))?;
    }
    if let Ok(value) = std::env::var("AIDHD_MAX_DECOMPOSITIONS") {
        config.max_decompositions = value
            .parse()
            .with_context(|| format!("invalid AIDHD_MAX_DECOMPOSITIONS: {value}"))?;
    }
    if let Ok(value) = std::env::var("AIDHD_SECRETARY_CADENCE") {
        config.cadence = value;
    }
    if let Ok(value) = std::env::var("AIDHD_AUTO_SCHEDULE") {
        config.auto_schedule = match value.to_lowercase().as_str() {
            "1" | "true" | "on" | "yes" => true,
            "0" | "false" | "off" | "no" => false,
            _ => bail!("invalid AIDHD_AUTO_SCHEDULE: {value}"),
        };
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_openrouter_deepseek_without_thinking() {
        let config = AiConfig::default();
        assert_eq!(config.provider, Provider::OpenRouter);
        assert_eq!(config.model, "deepseek/deepseek-v4.1-flash");
        assert!(!config.thinking);
        assert_eq!(config.credential_host, "openrouter");
    }

    #[test]
    fn file_overrides_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "provider = \"openai\"\nmodel = \"gpt-4o-mini\"\nthinking = true\n",
        )
        .unwrap();
        let config = resolve_from(&path).unwrap();
        assert_eq!(config.provider, Provider::OpenAi);
        assert_eq!(config.model, "gpt-4o-mini");
        assert!(config.thinking);
        assert_eq!(config.credential_host, "openai");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn unknown_provider_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "provider = \"mistral\"\n").unwrap();
        assert!(resolve_from(&path).is_err());
    }
}
