use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};

use thiserror::Error;

use crate::model::RuntimeSnapshot;

#[derive(Debug, Error)]
pub enum ObservabilityApiError {
    #[error("observability API I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("observability API payload failed: {0}")]
    Payload(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityApiResponse {
    pub status_code: u16,
    pub reason: &'static str,
    pub content_type: &'static str,
    pub body: String,
}

impl ObservabilityApiResponse {
    pub fn to_http_response(&self) -> String {
        format!(
            "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            self.status_code,
            self.reason,
            self.content_type,
            self.body.len(),
            self.body
        )
    }
}

pub fn status_response(
    snapshot: &RuntimeSnapshot,
) -> Result<ObservabilityApiResponse, ObservabilityApiError> {
    Ok(ObservabilityApiResponse {
        status_code: 200,
        reason: "OK",
        content_type: "application/json",
        body: serde_json::to_string_pretty(snapshot)?,
    })
}

pub fn route_request(
    request: &str,
    snapshot: &RuntimeSnapshot,
) -> Result<ObservabilityApiResponse, ObservabilityApiError> {
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    match path {
        "/status" | "/status.json" => status_response(snapshot),
        "/health" => Ok(ObservabilityApiResponse {
            status_code: 200,
            reason: "OK",
            content_type: "application/json",
            body: "{\"status\":\"ok\"}".into(),
        }),
        _ => Ok(ObservabilityApiResponse {
            status_code: 404,
            reason: "Not Found",
            content_type: "application/json",
            body: "{\"error\":\"not found\"}".into(),
        }),
    }
}

pub fn serve_once(
    bind: SocketAddr,
    snapshot: &RuntimeSnapshot,
) -> Result<SocketAddr, ObservabilityApiError> {
    let listener = TcpListener::bind(bind)?;
    let local_addr = listener.local_addr()?;
    let (mut stream, _) = listener.accept()?;
    let mut buffer = [0_u8; 4096];
    let bytes_read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let response = route_request(&request, snapshot)?.to_http_response();
    stream.write_all(response.as_bytes())?;
    Ok(local_addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_status_request_to_snapshot_json() {
        let snapshot = RuntimeSnapshot {
            event_log_path: Some("/tmp/events.jsonl".into()),
            integration_gaps: vec!["gap".into()],
            ..Default::default()
        };

        let response = route_request("GET /status.json HTTP/1.1\r\n\r\n", &snapshot).unwrap();
        let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(
            body.get("event_log_path")
                .and_then(serde_json::Value::as_str),
            Some("/tmp/events.jsonl")
        );
        assert_eq!(
            body.pointer("/integration_gaps/0")
                .and_then(serde_json::Value::as_str),
            Some("gap")
        );
    }

    #[test]
    fn routes_health_and_unknown_paths() {
        let snapshot = RuntimeSnapshot::default();

        let health = route_request("GET /health HTTP/1.1\r\n\r\n", &snapshot).unwrap();
        assert_eq!(health.status_code, 200);
        assert_eq!(health.body, "{\"status\":\"ok\"}");

        let missing = route_request("GET /missing HTTP/1.1\r\n\r\n", &snapshot).unwrap();
        assert_eq!(missing.status_code, 404);
    }
}
