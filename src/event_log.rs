use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub event: String,
    pub issue_id: Option<String>,
    pub issue_identifier: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub instance_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Error)]
pub enum EventLogError {
    #[error("event log io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("event log serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
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
            message: "queued".into(),
        })
        .unwrap();

        let content = std::fs::read_to_string(temp.path().join("events.jsonl")).unwrap();
        assert!(content.contains("\"event\":\"dispatch\""));
        assert!(content.contains("\"profile_id\":\"codex-alpha\""));
    }
}
