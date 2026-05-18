use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;

pub const SESSION_REGISTRY_FILE: &str = "session-registry.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRegistry {
    #[serde(default)]
    pub sessions: Vec<AgentSessionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionRecord {
    pub issue_id: Option<String>,
    pub issue_identifier: Option<String>,
    pub issue_title: Option<String>,
    pub lane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_value: Option<String>,
    pub actor_role: Option<String>,
    pub actor_label: Option<String>,
    pub git_author: Option<String>,
    pub profile_id: Option<String>,
    pub instance_name: Option<String>,
    pub worktree: PathBuf,
    pub branch: Option<String>,
    pub backend: String,
    pub session_name: String,
    pub pane_target: String,
    pub prompt_artifact_path: PathBuf,
    pub log_path: PathBuf,
    pub attach_command: String,
    pub attempt: u32,
    pub status: SessionStatus,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Starting,
    Running,
    WaitingForTrust,
    WaitingForApproval,
    WaitingForHumanInput,
    UsageLimited,
    Failed,
    Completed,
    Recorded,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionStatusProbe {
    pub status: SessionStatus,
    pub source: SessionStatusSource,
    pub evidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatusSource {
    Pane,
    Log,
    Registry,
    None,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::WaitingForTrust => "waiting_for_trust",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::WaitingForHumanInput => "waiting_for_human_input",
            Self::UsageLimited => "usage_limited",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Recorded => "recorded",
            Self::Stale => "stale",
            Self::Unknown => "unknown",
        }
    }
}

impl SessionStatusSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pane => "pane",
            Self::Log => "log",
            Self::Registry => "registry",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionRegistryError {
    #[error("session registry io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session registry serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn session_registry_path(config: &RuntimeConfig) -> PathBuf {
    let mut path = config.artifacts.root.clone();
    if let Some(namespace) = &config.artifacts.namespace {
        path = path.join(namespace);
    }
    path.join("default")
        .join("sessions")
        .join(SESSION_REGISTRY_FILE)
}

pub fn load_session_registry(path: &Path) -> Result<SessionRegistry, SessionRegistryError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SessionRegistry {
            sessions: Vec::new(),
        }),
        Err(error) => Err(error.into()),
    }
}

pub fn save_session_record(
    path: &Path,
    record: AgentSessionRecord,
) -> Result<(), SessionRegistryError> {
    let mut registry = load_session_registry(path)?;
    if let Some(existing) = registry
        .sessions
        .iter_mut()
        .find(|existing| existing.session_name == record.session_name)
    {
        *existing = record;
    } else {
        registry.sessions.push(record);
    }
    save_session_registry(path, &registry)
}

pub fn save_session_registry(
    path: &Path,
    registry: &SessionRegistry,
) -> Result<(), SessionRegistryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(registry)?;
    fs::write(&temp_path, content)?;
    fs::rename(temp_path, path)?;
    Ok(())
}

pub fn deterministic_session_name(
    prefix: &str,
    lane: &str,
    issue_identifier: Option<&str>,
    attempt: u32,
    slug: Option<&str>,
) -> String {
    let issue = issue_identifier
        .and_then(issue_number)
        .unwrap_or_else(|| safe_component(issue_identifier, 24));
    format!(
        "{}-{}-{}-attempt-{}-{}",
        safe_component(Some(prefix), 24),
        safe_component(Some(lane), 16),
        issue,
        attempt.max(1),
        safe_component(slug, 48)
    )
}

pub fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

pub fn capture_tmux_pane_tail(
    tmux_command: &str,
    target: &str,
    max_lines: usize,
) -> Result<String, String> {
    let start = format!("-{}", max_lines.clamp(1, 500));
    match Command::new(tmux_command)
        .args(["capture-pane", "-p", "-t", target, "-S", &start])
        .output()
    {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        }
        Ok(output) => Err(format!(
            "tmux capture-pane exited with status {}",
            output.status.code().unwrap_or(-1)
        )),
        Err(error) => Err(format!("tmux capture-pane failed: {error}")),
    }
}

pub fn read_log_tail(
    path: &Path,
    max_lines: usize,
) -> Result<Option<String>, SessionRegistryError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(tail_lines(&content, max_lines))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn classify_session_record(
    record: &AgentSessionRecord,
    pane_tail: Option<&str>,
    log_tail: Option<&str>,
    now_ms: u64,
    stale_after_ms: u64,
) -> SessionStatusProbe {
    if let Some(probe) =
        pane_tail.and_then(|tail| classify_session_output(tail, SessionStatusSource::Pane))
    {
        return probe;
    }

    if let Some(probe) =
        log_tail.and_then(|tail| classify_session_output(tail, SessionStatusSource::Log))
    {
        return probe;
    }

    if stale_after_ms > 0
        && now_ms.saturating_sub(record.updated_at_ms) > stale_after_ms
        && !matches!(
            record.status,
            SessionStatus::Completed | SessionStatus::Failed | SessionStatus::Recorded
        )
    {
        return SessionStatusProbe {
            status: SessionStatus::Stale,
            source: SessionStatusSource::Registry,
            evidence: format!(
                "registry record has not updated for {}ms",
                now_ms.saturating_sub(record.updated_at_ms)
            ),
        };
    }

    if let Some(tail) = pane_tail
        .filter(|tail| !tail.trim().is_empty())
        .or_else(|| log_tail.filter(|tail| !tail.trim().is_empty()))
    {
        return SessionStatusProbe {
            status: SessionStatus::Unknown,
            source: if pane_tail.is_some() {
                SessionStatusSource::Pane
            } else {
                SessionStatusSource::Log
            },
            evidence: compact_session_evidence(tail),
        };
    }

    SessionStatusProbe {
        status: record.status.clone(),
        source: SessionStatusSource::Registry,
        evidence: format!("registry status {}", record.status.as_str()),
    }
}

pub fn classify_session_output(
    text: &str,
    source: SessionStatusSource,
) -> Option<SessionStatusProbe> {
    let evidence = compact_session_evidence(text);
    if evidence.is_empty() {
        return Some(SessionStatusProbe {
            status: SessionStatus::Starting,
            source,
            evidence: "no pane or log output yet".into(),
        });
    }

    let normalized = normalized_session_text(text);
    let status = if contains_any(
        &normalized,
        &[
            "usage limit",
            "rate limit",
            "quota exceeded",
            "too many requests",
            "429",
        ],
    ) {
        SessionStatus::UsageLimited
    } else if contains_any(
        &normalized,
        &[
            "do you trust the contents of this directory",
            "do you trust the files in this directory",
            "do you trust the files in this folder",
        ],
    ) || (normalized.contains("trust")
        && normalized.contains("directory")
        && normalized.contains("codex"))
    {
        SessionStatus::WaitingForTrust
    } else if (normalized.contains("approval")
        || normalized.contains("approve")
        || normalized.contains("allow"))
        && contains_any(
            &normalized,
            &[
                "command",
                "permission",
                "escalation",
                "sandbox",
                "execute",
                "action",
            ],
        )
    {
        SessionStatus::WaitingForApproval
    } else if contains_any(
        &normalized,
        &[
            "need human input",
            "needs human input",
            "waiting for human",
            "requires human input",
            "clarification needed",
            "please provide",
        ],
    ) {
        SessionStatus::WaitingForHumanInput
    } else if contains_any(
        &normalized,
        &[
            "completed successfully",
            "task complete",
            "status=complete",
            "final answer",
            "done; verification",
        ],
    ) {
        SessionStatus::Completed
    } else if contains_any(
        &normalized,
        &[
            "exited with status",
            "fatal:",
            "panic",
            "thread '",
            "error:",
            "operation not permitted",
            "permission denied",
        ],
    ) {
        SessionStatus::Failed
    } else if contains_any(
        &normalized,
        &[
            "starting",
            "launching",
            "loading",
            "checking workspace",
            "preparing",
        ],
    ) {
        SessionStatus::Starting
    } else if contains_any(
        &normalized,
        &[
            "thinking",
            "working",
            "running",
            "executing",
            "applying patch",
            "tokens",
            "codex",
        ],
    ) || text.contains('›')
        || text.contains('▌')
    {
        SessionStatus::Running
    } else {
        return None;
    };

    Some(SessionStatusProbe {
        status,
        source,
        evidence,
    })
}

pub fn tail_lines(text: &str, max_lines: usize) -> String {
    let max_lines = max_lines.max(1);
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

pub fn compact_session_evidence(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_LEN: usize = 180;
    if compact.len() > MAX_LEN {
        format!("{}...", &compact[..MAX_LEN])
    } else {
        compact
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn normalized_session_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn issue_number(identifier: &str) -> Option<String> {
    let number = identifier
        .trim()
        .strip_prefix('#')
        .unwrap_or(identifier.trim());
    (!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
        .then(|| number.to_string())
}

fn safe_component(value: Option<&str>, max_len: usize) -> String {
    let safe = value
        .unwrap_or("run")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let safe = if safe.is_empty() { "run".into() } else { safe };
    safe.chars().take(max_len.max(1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowDefinition;

    fn config_with_artifact_root(root: &Path) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: memory\nartifacts:\n  root: {:?}\n  namespace: acme/project\n---\nPrompt",
                root.display().to_string()
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    #[test]
    fn session_name_includes_lane_issue_attempt_and_slug() {
        let name = deterministic_session_name(
            "jade",
            "main",
            Some("#225"),
            3,
            Some("Add durable tmux session registry and naming contract"),
        );

        assert!(name.starts_with("jade-main-225-attempt-3-add-durable-tmux"));
        assert!(name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
        }));
    }

    #[test]
    fn session_registry_path_uses_artifact_root_and_namespace() {
        let temp = tempfile::tempdir().unwrap();
        let config = config_with_artifact_root(temp.path());

        assert_eq!(
            session_registry_path(&config),
            temp.path()
                .join("acme/project")
                .join("default")
                .join("sessions")
                .join(SESSION_REGISTRY_FILE)
        );
    }

    #[test]
    fn saves_and_replaces_session_records() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(SESSION_REGISTRY_FILE);
        let mut record = AgentSessionRecord {
            issue_id: Some("I_225".into()),
            issue_identifier: Some("#225".into()),
            issue_title: Some("Add registry".into()),
            lane: "main".into(),
            run_id: Some("20260516T0415Z-issue225-main-0001".into()),
            thread: None,
            session_source: None,
            claim_value: None,
            actor_role: Some("implementation_agent".into()),
            actor_label: Some("Jade Symphony Agent".into()),
            git_author: None,
            profile_id: None,
            instance_name: None,
            worktree: PathBuf::from("/tmp/worktree"),
            branch: Some("feature/issue-225".into()),
            backend: "tmux".into(),
            session_name: "jade-main-225-attempt-1-add-registry".into(),
            pane_target: "jade-main-225-attempt-1-add-registry".into(),
            prompt_artifact_path: PathBuf::from("/tmp/prompt.md"),
            log_path: PathBuf::from("/tmp/session.log"),
            attach_command: "tmux attach-session -t jade-main-225-attempt-1-add-registry".into(),
            attempt: 1,
            status: SessionStatus::Running,
            started_at_ms: 10,
            updated_at_ms: 10,
        };

        save_session_record(&path, record.clone()).unwrap();
        record.updated_at_ms = 20;
        save_session_record(&path, record).unwrap();
        let loaded = load_session_registry(&path).unwrap();

        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].updated_at_ms, 20);
    }

    #[test]
    fn classifies_required_session_statuses_from_fixture_output() {
        let fixtures = [
            ("Starting Codex...", SessionStatus::Starting),
            (
                "Codex is thinking about the next edit",
                SessionStatus::Running,
            ),
            (
                "Do you trust the contents of this directory?",
                SessionStatus::WaitingForTrust,
            ),
            (
                "Approval required: allow this command to execute?",
                SessionStatus::WaitingForApproval,
            ),
            (
                "Need human input before continuing",
                SessionStatus::WaitingForHumanInput,
            ),
            (
                "usage limit reached; try again later",
                SessionStatus::UsageLimited,
            ),
            ("cargo clippy exited with status 101", SessionStatus::Failed),
            (
                "Task complete. Final answer ready.",
                SessionStatus::Completed,
            ),
        ];

        for (fixture, expected) in fixtures {
            let probe = classify_session_output(fixture, SessionStatusSource::Pane).unwrap();
            assert_eq!(probe.status, expected, "fixture={fixture}");
            assert_eq!(probe.source, SessionStatusSource::Pane);
        }
    }

    #[test]
    fn classifies_stale_when_registry_record_is_old_and_output_is_unknown() {
        let record = fixture_record();

        let probe = classify_session_record(&record, Some("unrecognized"), None, 10_500, 5_000);

        assert_eq!(probe.status, SessionStatus::Stale);
        assert_eq!(probe.source, SessionStatusSource::Registry);
    }

    #[test]
    fn classifies_unknown_when_bounded_output_is_inconclusive() {
        let record = fixture_record();

        let probe = classify_session_record(&record, Some("unrecognized"), None, 3_000, 5_000);

        assert_eq!(probe.status, SessionStatus::Unknown);
        assert_eq!(probe.source, SessionStatusSource::Pane);
    }

    fn fixture_record() -> AgentSessionRecord {
        AgentSessionRecord {
            issue_id: Some("I_226".into()),
            issue_identifier: Some("#226".into()),
            issue_title: Some("Classify status".into()),
            lane: "main".into(),
            run_id: None,
            thread: None,
            session_source: None,
            claim_value: None,
            actor_role: None,
            actor_label: None,
            git_author: None,
            profile_id: None,
            instance_name: None,
            worktree: PathBuf::from("/tmp/worktree"),
            branch: None,
            backend: "tmux".into(),
            session_name: "jade-main-226-attempt-1-classify".into(),
            pane_target: "jade-main-226-attempt-1-classify".into(),
            prompt_artifact_path: PathBuf::from("/tmp/prompt.md"),
            log_path: PathBuf::from("/tmp/session.log"),
            attach_command: "tmux attach-session -t jade-main-226-attempt-1-classify".into(),
            attempt: 1,
            status: SessionStatus::Running,
            started_at_ms: 1_000,
            updated_at_ms: 1_000,
        }
    }

    #[test]
    fn tails_bounded_log_lines() {
        assert_eq!(tail_lines("one\ntwo\nthree\nfour", 2), "three\nfour");
    }

    #[test]
    fn recorded_manual_evidence_does_not_become_stale() {
        let mut record = fixture_record();
        record.backend = "codex-app-manual".into();
        record.status = SessionStatus::Recorded;
        record.updated_at_ms = 1_000;

        let probe = classify_session_record(&record, None, None, 20_000, 10_000);

        assert_eq!(probe.status, SessionStatus::Recorded);
        assert_eq!(probe.source, SessionStatusSource::Registry);
        assert_eq!(probe.evidence, "registry status recorded");
    }
}
