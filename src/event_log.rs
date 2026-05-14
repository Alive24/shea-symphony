use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub event: String,
    pub issue_id: Option<String>,
    pub issue_identifier: Option<String>,
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker_mutation: Option<TrackerMutationAuditRecord>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerMutationAuditRecord {
    pub command: String,
    pub mutation_type: String,
    pub issue_ref: Option<String>,
    pub target: Option<String>,
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    pub reason: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerMutationAuditInput {
    pub command: String,
    pub mutation_type: String,
    pub issue_ref: Option<String>,
    pub target: Option<String>,
    pub from_state: Option<String>,
    pub to_state: Option<String>,
    pub reason: String,
    pub timestamp_ms: u64,
}

impl TrackerMutationAuditRecord {
    pub fn from_input(input: TrackerMutationAuditInput) -> Self {
        Self {
            command: input.command,
            mutation_type: input.mutation_type,
            issue_ref: input.issue_ref,
            target: input.target.map(redact_audit_text),
            from_state: input.from_state,
            to_state: input.to_state,
            reason: redact_audit_text(input.reason),
            timestamp_ms: input.timestamp_ms,
        }
    }
}

fn redact_audit_text(value: String) -> String {
    value
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if lower.contains("token=")
                || lower.contains("gh_token=")
                || lower.contains("github_token=")
                || lower.contains("authorization:")
            {
                "[redacted]"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventLogSummary {
    pub total_records: usize,
    pub events_by_name: BTreeMap<String, usize>,
    pub issue_identifiers: Vec<String>,
    pub session_ids: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EventLogError {
    #[error("event log io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("event log serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("event log parse error at line {line}: {source}")]
    ParseLine {
        line: usize,
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone)]
pub struct EventLog {
    path: PathBuf,
}

impl EventLog {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn append(&self, record: &EventRecord) -> Result<(), EventLogError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(record)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    pub fn read_records(&self) -> Result<Vec<EventRecord>, EventLogError> {
        let file = OpenOptions::new().read(true).open(&self.path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let record =
                serde_json::from_str(&line).map_err(|source| EventLogError::ParseLine {
                    line: index + 1,
                    source,
                })?;
            records.push(record);
        }

        Ok(records)
    }

    pub fn summarize(&self) -> Result<EventLogSummary, EventLogError> {
        Ok(EventLogSummary::from_records(&self.read_records()?))
    }
}

impl EventLogSummary {
    pub fn from_records(records: &[EventRecord]) -> Self {
        let mut events_by_name = BTreeMap::new();
        let mut issue_identifiers = BTreeSet::new();
        let mut session_ids = BTreeSet::new();

        for record in records {
            *events_by_name.entry(record.event.clone()).or_insert(0) += 1;
            if let Some(identifier) = &record.issue_identifier {
                issue_identifiers.insert(identifier.clone());
            }
            if let Some(session_id) = &record.session_id {
                session_ids.insert(session_id.clone());
            }
        }

        Self {
            total_records: records.len(),
            events_by_name,
            issue_identifiers: issue_identifiers.into_iter().collect(),
            session_ids: session_ids.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_records() {
        let temp = tempfile::tempdir().unwrap();
        let log = EventLog::new(temp.path().join("events.jsonl"));
        log.append(&EventRecord {
            event: "dispatch".into(),
            issue_id: Some("id".into()),
            issue_identifier: Some("#1".into()),
            session_id: None,
            profile_id: Some("codex-alpha".into()),
            instance_name: Some("Codex Alpha".into()),
            actor_role: Some("implementation_agent".into()),
            actor_label: Some("Jade Symphony Agent".into()),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            tracker_mutation: None,
            message: "queued".into(),
        })
        .unwrap();

        let content = std::fs::read_to_string(temp.path().join("events.jsonl")).unwrap();
        assert!(content.contains("\"event\":\"dispatch\""));
        assert!(content.contains("\"profile_id\":\"codex-alpha\""));
        assert!(content.contains("\"actor_role\":\"implementation_agent\""));
    }

    #[test]
    fn reads_records_and_summarizes_event_log() {
        let temp = tempfile::tempdir().unwrap();
        let log = EventLog::new(temp.path().join("events.jsonl"));
        let first = EventRecord {
            event: "dispatch".into(),
            issue_id: Some("id-1".into()),
            issue_identifier: Some("#1".into()),
            session_id: Some("session-1".into()),
            profile_id: None,
            instance_name: None,
            actor_role: None,
            actor_label: None,
            git_author: None,
            tracker_mutation: None,
            message: "queued".into(),
        };
        let second = EventRecord {
            event: "complete".into(),
            issue_id: Some("id-1".into()),
            issue_identifier: Some("#1".into()),
            session_id: Some("session-1".into()),
            profile_id: None,
            instance_name: None,
            actor_role: None,
            actor_label: None,
            git_author: None,
            tracker_mutation: None,
            message: "done".into(),
        };

        log.append(&first).unwrap();
        log.append(&second).unwrap();

        assert_eq!(log.read_records().unwrap(), vec![first, second]);

        let summary = log.summarize().unwrap();
        assert_eq!(summary.total_records, 2);
        assert_eq!(summary.events_by_name["dispatch"], 1);
        assert_eq!(summary.events_by_name["complete"], 1);
        assert_eq!(summary.issue_identifiers, vec!["#1"]);
        assert_eq!(summary.session_ids, vec!["session-1"]);
    }

    #[test]
    fn malformed_jsonl_is_a_parse_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("events.jsonl");
        let valid = serde_json::to_string(&EventRecord {
            event: "ok".into(),
            issue_id: None,
            issue_identifier: None,
            session_id: None,
            profile_id: None,
            instance_name: None,
            actor_role: None,
            actor_label: None,
            git_author: None,
            tracker_mutation: None,
            message: "ok".into(),
        })
        .unwrap();
        std::fs::write(&path, format!("{valid}\nnot-json\n")).unwrap();
        let log = EventLog::new(path);

        let error = log.read_records().unwrap_err();
        assert!(matches!(error, EventLogError::ParseLine { line: 2, .. }));
    }

    #[test]
    fn tracker_mutation_records_redact_secret_like_text() {
        let record = TrackerMutationAuditRecord::from_input(TrackerMutationAuditInput {
            command: "run-loop".into(),
            mutation_type: "state_change".into(),
            issue_ref: Some("#1".into()),
            target: Some("Authorization: bearer secret".into()),
            from_state: Some("Todo".into()),
            to_state: Some("In Progress".into()),
            reason: "claim token=secret".into(),
            timestamp_ms: 42,
        });

        assert_eq!(record.command, "run-loop");
        assert_eq!(record.mutation_type, "state_change");
        assert_eq!(record.target.as_deref(), Some("[redacted] bearer secret"));
        assert_eq!(record.reason, "claim [redacted]");
    }
}
