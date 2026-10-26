use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::AiConfig;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("credential error: {0}")]
    Credential(#[from] octx_creds::CredsError),
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("API error: {0}")]
    ApiError(String),
}

pub type Result<T> = std::result::Result<T, LlmError>;

#[derive(Debug, Clone)]
pub struct LlmClient {
    config: AiConfig,
    client: Client,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct RequestBody {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseBody {
    choices: Vec<Choice>,
}

impl LlmClient {
    pub fn new(config: AiConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client with valid configuration");
        Self { config, client }
    }

    pub fn config(&self) -> &AiConfig {
        &self.config
    }

    /// Send a two-message completion request and return the assistant content.
    pub fn complete(&self, system: &str, user: &str) -> Result<String> {
        let store = octx_creds::Store::open_default()?;
        let secret = store.get(&self.config.credential_host)?;
        let auth_header = secret.bearer()?;

        let body = RequestBody {
            model: self.config.model.clone(),
            messages: vec![
                Message {
                    role: "system".into(),
                    content: system.into(),
                },
                Message {
                    role: "user".into(),
                    content: user.into(),
                },
            ],
            temperature: 0.2,
            reasoning: self
                .config
                .thinking
                .then(|| serde_json::json!({"enabled": true})),
        };

        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            return Err(LlmError::ApiError(format!("{status}: {text}")));
        }

        let parsed: ResponseBody = response.json()?;
        let choice = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("response contained no choices".into()))?;
        Ok(choice.message.content.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    #[test]
    fn request_omits_reasoning_when_thinking_disabled() {
        let config = AiConfig {
            provider: Provider::OpenRouter,
            model: "deepseek/deepseek-v4.1-flash".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            credential_host: "openrouter".into(),
            thinking: false,
        };
        let body = RequestBody {
            model: config.model.clone(),
            messages: vec![Message {
                role: "user".into(),
                content: "hi".into(),
            }],
            temperature: 0.2,
            reasoning: config
                .thinking
                .then(|| serde_json::json!({"enabled": true})),
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(!json.contains("reasoning"));
    }

    #[test]
    fn request_includes_reasoning_when_thinking_enabled() {
        let json = serde_json::to_string(&RequestBody {
            model: "m".into(),
            messages: vec![],
            temperature: 0.2,
            reasoning: Some(serde_json::json!({"enabled": true})),
        })
        .unwrap();
        assert!(json.contains("reasoning"));
    }
}
