//! The NDJSON event vocabulary streamed to the orchestration script.
//!
//! This vocabulary is the agent arm's own stable contract: ACP updates are
//! mapped into these shapes rather than passed through, so protocol churn stops
//! here (see the "NDJSON event vocabulary is a stable contract" design decision).

use serde::Serialize;

/// Usage figures reported with a completed turn.
#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    /// Tokens sent as input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Tokens produced as output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

/// One event on the agent arm -> script stream.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Sent once, after the ACP session exists and a script is connected.
    Ready {
        /// ACP session id.
        session_id: String,
    },
    /// A chunk of assistant-visible text.
    Text {
        /// The text delta.
        delta: String,
    },
    /// A tool-call lifecycle event.
    Tool {
        /// Tool name, when the agent reports one.
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// One of `pending`, `running`, `done`, `failed`, `cancelled`.
        status: String,
        /// ACP tool-call id.
        id: String,
        /// Captured output, when available.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Exit code, when available.
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    /// A completed prompt turn.
    TurnDone {
        /// ACP stop reason (e.g. `end_turn`, `cancelled`).
        stop_reason: String,
        /// Token usage, when the agent reports it.
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<UsageInfo>,
    },
    /// A run-level error surfaced to the script.
    Error {
        /// Human-readable message.
        message: String,
    },
}

impl Event {
    /// Serialize to a single NDJSON line (without the trailing newline).
    #[must_use]
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"type\":\"error\"}".to_string())
    }

    /// Merge a newer tool-call update into an older one: the latest status wins,
    /// and a field the newer update omits keeps its previous value. Used by the
    /// `--format json` summary so each tool call appears once, not once per update.
    pub fn merge_tool_update(&mut self, newer: &Self) {
        let (
            Self::Tool {
                name,
                status,
                output,
                exit_code,
                ..
            },
            Self::Tool {
                name: new_name,
                status: new_status,
                output: new_output,
                exit_code: new_exit_code,
                ..
            },
        ) = (self, newer)
        else {
            return;
        };
        if new_name.is_some() {
            *name = new_name.clone();
        }
        *status = new_status.clone();
        if new_output.is_some() {
            *output = new_output.clone();
        }
        if new_exit_code.is_some() {
            *exit_code = *new_exit_code;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: Option<&str>, status: &str, id: &str) -> Event {
        Event::Tool {
            name: name.map(str::to_string),
            status: status.to_string(),
            id: id.to_string(),
            output: None,
            exit_code: None,
        }
    }

    #[test]
    fn merge_keeps_latest_status_and_earliest_name() {
        let mut first = tool(Some("bash"), "pending", "tc_1");
        first.merge_tool_update(&tool(None, "running", "tc_1"));
        first.merge_tool_update(&tool(None, "done", "tc_1"));
        match first {
            Event::Tool { name, status, .. } => {
                assert_eq!(name.as_deref(), Some("bash"));
                assert_eq!(status, "done");
            }
            other => panic!("expected tool, got {other:?}"),
        }
    }
}
