use serde::{Deserialize, Serialize};

use crate::model::AgentEvent;

pub const BACKEND_NAME: &str = "codex-app-server";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexAppServerEvent {
    pub event: CodexAppServerEventKind,
    pub method: Option<String>,
    pub session_id: Option<String>,
    pub message: String,
    pub agent_event: Option<AgentEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodexAppServerEventKind {
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
    TurnInputRequired,
    TokenUsage,
    Notification,
    OtherMessage,
    Malformed,
}

pub fn normalize_json_rpc_line(line: &str, session_id: Option<&str>) -> CodexAppServerEvent {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return malformed_event(trimmed, session_id, "empty app-server line");
    }

    let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return malformed_event(trimmed, session_id, "malformed app-server JSON line");
    };

    let Some(method) = payload.get("method").and_then(serde_json::Value::as_str) else {
        return CodexAppServerEvent {
            event: CodexAppServerEventKind::OtherMessage,
            method: None,
            session_id: session_id.map(str::to_string),
            message: "app-server message without method".into(),
            agent_event: Some(AgentEvent::Message {
                backend: BACKEND_NAME.into(),
                session_id: session_id.map(str::to_string),
                text: compact_json(&payload),
            }),
        };
    };

    match method {
        "turn/completed" => completed_event(method, session_id, &payload),
        "turn/failed" => failed_event(
            CodexAppServerEventKind::TurnFailed,
            method,
            session_id,
            &payload,
        ),
        "turn/cancelled" => failed_event(
            CodexAppServerEventKind::TurnCancelled,
            method,
            session_id,
            &payload,
        ),
        "thread/tokenUsage/updated" => token_usage_event(method, session_id, &payload),
        method if input_required_method(method) => {
            input_required_event(method, session_id, &payload)
        }
        _ => notification_event(method, session_id, &payload),
    }
}

fn completed_event(
    method: &str,
    session_id: Option<&str>,
    payload: &serde_json::Value,
) -> CodexAppServerEvent {
    CodexAppServerEvent {
        event: CodexAppServerEventKind::TurnCompleted,
        method: Some(method.into()),
        session_id: session_id.map(str::to_string),
        message: "Codex app-server turn completed.".into(),
        agent_event: Some(AgentEvent::Completed {
            backend: BACKEND_NAME.into(),
            session_id: session_id.map(str::to_string),
            summary: turn_status(payload)
                .map(|status| format!("Codex app-server turn completed with status {status}."))
                .unwrap_or_else(|| "Codex app-server turn completed.".into()),
        }),
    }
}

fn failed_event(
    event: CodexAppServerEventKind,
    method: &str,
    session_id: Option<&str>,
    payload: &serde_json::Value,
) -> CodexAppServerEvent {
    let reason = error_message(payload).unwrap_or_else(|| compact_json(payload));
    CodexAppServerEvent {
        event,
        method: Some(method.into()),
        session_id: session_id.map(str::to_string),
        message: reason.clone(),
        agent_event: Some(AgentEvent::Failed {
            backend: BACKEND_NAME.into(),
            error: format!("{method}: {reason}"),
        }),
    }
}

fn input_required_event(
    method: &str,
    session_id: Option<&str>,
    payload: &serde_json::Value,
) -> CodexAppServerEvent {
    let message = format!("{method}: app-server turn requires unavailable user input");
    CodexAppServerEvent {
        event: CodexAppServerEventKind::TurnInputRequired,
        method: Some(method.into()),
        session_id: session_id.map(str::to_string),
        message: message.clone(),
        agent_event: Some(AgentEvent::Failed {
            backend: BACKEND_NAME.into(),
            error: format!("{message}: {}", compact_json(payload)),
        }),
    }
}

fn token_usage_event(
    method: &str,
    session_id: Option<&str>,
    payload: &serde_json::Value,
) -> CodexAppServerEvent {
    let usage = payload.get("params").unwrap_or(payload);
    let input_tokens = json_u64_any(usage, &["input_tokens", "inputTokens", "input"]);
    let output_tokens = json_u64_any(usage, &["output_tokens", "outputTokens", "output"]);
    let total_tokens =
        json_u64_any(usage, &["total_tokens", "totalTokens", "total"]).or_else(|| {
            input_tokens
                .zip(output_tokens)
                .map(|(input, output)| input + output)
        });

    let agent_event = match (input_tokens, output_tokens, total_tokens) {
        (Some(input_tokens), Some(output_tokens), Some(total_tokens)) => {
            Some(AgentEvent::TokenUsage {
                backend: BACKEND_NAME.into(),
                input_tokens,
                output_tokens,
                total_tokens,
            })
        }
        _ => Some(AgentEvent::Message {
            backend: BACKEND_NAME.into(),
            session_id: session_id.map(str::to_string),
            text: compact_json(payload),
        }),
    };

    CodexAppServerEvent {
        event: CodexAppServerEventKind::TokenUsage,
        method: Some(method.into()),
        session_id: session_id.map(str::to_string),
        message: "Codex app-server token usage updated.".into(),
        agent_event,
    }
}

fn notification_event(
    method: &str,
    session_id: Option<&str>,
    payload: &serde_json::Value,
) -> CodexAppServerEvent {
    CodexAppServerEvent {
        event: CodexAppServerEventKind::Notification,
        method: Some(method.into()),
        session_id: session_id.map(str::to_string),
        message: method.into(),
        agent_event: Some(AgentEvent::Message {
            backend: BACKEND_NAME.into(),
            session_id: session_id.map(str::to_string),
            text: compact_json(payload),
        }),
    }
}

fn malformed_event(raw: &str, session_id: Option<&str>, message: &str) -> CodexAppServerEvent {
    CodexAppServerEvent {
        event: CodexAppServerEventKind::Malformed,
        method: None,
        session_id: session_id.map(str::to_string),
        message: message.into(),
        agent_event: Some(AgentEvent::Failed {
            backend: BACKEND_NAME.into(),
            error: if raw.is_empty() {
                message.into()
            } else {
                format!("{message}: {raw}")
            },
        }),
    }
}

fn input_required_method(method: &str) -> bool {
    matches!(
        method,
        "item/tool/requestUserInput" | "tool/requestUserInput"
    )
}

fn turn_status(payload: &serde_json::Value) -> Option<String> {
    payload
        .pointer("/params/turn/status")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn error_message(payload: &serde_json::Value) -> Option<String> {
    payload
        .pointer("/params/error/message")
        .or_else(|| payload.pointer("/params/message"))
        .or_else(|| payload.pointer("/error/message"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn json_u64_any(payload: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(serde_json::Value::as_u64))
}

fn compact_json(payload: &serde_json::Value) -> String {
    serde_json::to_string(payload).unwrap_or_else(|_| "<unserializable app-server payload>".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_turn_completed_to_completed_agent_event() {
        let event = normalize_json_rpc_line(
            r#"{"method":"turn/completed","params":{"turn":{"status":"completed"}}}"#,
            Some("thread-1-turn-1"),
        );

        assert_eq!(event.event, CodexAppServerEventKind::TurnCompleted);
        assert!(matches!(
            event.agent_event,
            Some(AgentEvent::Completed {
                session_id: Some(ref session_id),
                ..
            }) if session_id == "thread-1-turn-1"
        ));
    }

    #[test]
    fn maps_turn_failed_and_cancelled_to_failed_agent_events() {
        let failed = normalize_json_rpc_line(
            r#"{"method":"turn/failed","params":{"error":{"message":"boom"}}}"#,
            Some("s1"),
        );
        let cancelled = normalize_json_rpc_line(
            r#"{"method":"turn/cancelled","params":{"message":"operator stopped"}}"#,
            Some("s1"),
        );

        assert_eq!(failed.event, CodexAppServerEventKind::TurnFailed);
        assert!(matches!(
            failed.agent_event,
            Some(AgentEvent::Failed { ref error, .. }) if error.contains("boom")
        ));
        assert_eq!(cancelled.event, CodexAppServerEventKind::TurnCancelled);
        assert!(matches!(
            cancelled.agent_event,
            Some(AgentEvent::Failed { ref error, .. }) if error.contains("operator stopped")
        ));
    }

    #[test]
    fn maps_input_required_to_failed_agent_event() {
        let event = normalize_json_rpc_line(
            r#"{"method":"item/tool/requestUserInput","id":7,"params":{"prompt":"Continue?"}}"#,
            Some("s1"),
        );

        assert_eq!(event.event, CodexAppServerEventKind::TurnInputRequired);
        assert!(matches!(
            event.agent_event,
            Some(AgentEvent::Failed { ref error, .. }) if error.contains("requires unavailable user input")
        ));
    }

    #[test]
    fn maps_token_usage_to_token_usage_agent_event() {
        let event = normalize_json_rpc_line(
            r#"{"method":"thread/tokenUsage/updated","params":{"inputTokens":12,"outputTokens":8,"totalTokens":20}}"#,
            Some("s1"),
        );

        assert_eq!(event.event, CodexAppServerEventKind::TokenUsage);
        assert!(matches!(
            event.agent_event,
            Some(AgentEvent::TokenUsage {
                input_tokens: 12,
                output_tokens: 8,
                total_tokens: 20,
                ..
            })
        ));
    }

    #[test]
    fn maps_notifications_and_methodless_messages() {
        let notification = normalize_json_rpc_line(
            r#"{"method":"item/agentMessage/delta","params":{"delta":"hello"}}"#,
            Some("s1"),
        );
        let methodless = normalize_json_rpc_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#, None);

        assert_eq!(notification.event, CodexAppServerEventKind::Notification);
        assert_eq!(
            notification.method.as_deref(),
            Some("item/agentMessage/delta")
        );
        assert_eq!(methodless.event, CodexAppServerEventKind::OtherMessage);
    }

    #[test]
    fn malformed_protocol_line_is_failed_event() {
        let event = normalize_json_rpc_line("{not-json", Some("s1"));

        assert_eq!(event.event, CodexAppServerEventKind::Malformed);
        assert!(matches!(
            event.agent_event,
            Some(AgentEvent::Failed { ref error, .. }) if error.contains("malformed")
        ));
    }
}
