use serde::{Deserialize, Serialize};

/// Stable repository identity used in local read-model keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepoId {
    /// Source host, such as `github.com`.
    pub host: String,
    /// Repository owner or organization on the source host.
    pub owner: String,
    /// Repository name on the source host.
    pub repo: String,
}

impl RepoId {
    /// Builds a repository identity without performing remote validation.
    pub fn new(host: impl Into<String>, owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    /// Returns the canonical slash-delimited key stored by local projections.
    pub fn database_key(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo)
    }
}

/// Stable machine-local runtime identity used to scope repo-owned rows.
///
/// The caller persists this value across App restarts. It must not be derived
/// from a PID, timestamp, transient worktree path, or Temporal worker identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkspaceRuntimeId(String);

impl WorkspaceRuntimeId {
    /// Wraps an already selected stable workspace runtime identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the stable spelling stored in SQLite rows.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Tracker adapter that owns the authoritative external issue state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerBackend {
    /// GitHub Projects v2 and GitHub Issues.
    GithubProjectV2,
    /// Linear issue tracking.
    Linear,
}

impl TrackerBackend {
    /// Returns the stable serialized/storage spelling of this backend.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GithubProjectV2 => "github_project_v2",
            Self::Linear => "linear",
        }
    }
}

/// Tracker-scoped issue identity used by local cache rows.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IssueRef {
    /// Tracker adapter that resolves this issue.
    pub tracker_backend: TrackerBackend,
    /// Repository containing the issue.
    pub repo_id: RepoId,
    /// Tracker-native numeric issue identifier.
    pub number: u64,
}

impl IssueRef {
    /// Builds an issue reference without reading the tracker.
    pub fn new(tracker_backend: TrackerBackend, repo_id: RepoId, number: u64) -> Self {
        Self {
            tracker_backend,
            repo_id,
            number,
        }
    }

    /// Returns the full tracker, repository, and issue key stored in SQLite.
    pub fn database_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.tracker_backend.as_str(),
            self.repo_id.database_key(),
            self.number
        )
    }

    /// Formats the short tracker reference used only in operator surfaces.
    pub fn display_ref(&self) -> String {
        format!("#{}", self.number)
    }
}

/// Application-assigned Temporal Workflow ID stored by the local read model.
///
/// This is distinct from Temporal's Run ID and remains stable for one Workflow
/// execution identity. Construction does not validate naming policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowId(String);

impl WorkflowId {
    /// Wraps an already validated application Workflow ID.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrows the exact ID sent to and read from Temporal.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Lifecycle classification projected into the local Workflow index.
///
/// These values describe local knowledge; Temporal and tracker state remain
/// authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowIndexStatus {
    /// A start has been admitted locally but not yet observed as running.
    Starting,
    /// The Workflow execution is currently observed as open and running.
    Running,
    /// The Workflow execution completed successfully.
    Completed,
    /// The Workflow execution reached a known failure.
    Failed,
    /// Temporal rejected or failed the Workflow start request.
    StartFailed,
    /// A locally recorded start was not confirmed within its freshness window.
    StaleStart,
    /// An expected execution can no longer be found during reconciliation.
    StaleMissing,
    /// The execution is closed but its terminal classification is unavailable.
    ClosedUnknown,
}

impl WorkflowIndexStatus {
    /// Returns the stable serialized/storage spelling of this status.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::StartFailed => "start_failed",
            Self::StaleStart => "stale_start",
            Self::StaleMissing => "stale_missing",
            Self::ClosedUnknown => "closed_unknown",
        }
    }

    /// Returns whether this status participates in the local active-row guard.
    ///
    /// This is a local safety check only; it cannot authorize a Workflow start.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

/// Statuses constrained by the machine-wide local active-execution index.
pub const ACTIVE_WORKFLOW_STATUSES: &[WorkflowIndexStatus] =
    &[WorkflowIndexStatus::Starting, WorkflowIndexStatus::Running];

/// Freshness of a cached or projected local-state value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    /// The projection is current within its configured freshness policy.
    Fresh,
    /// The projection is known to be older than its freshness policy permits.
    Stale,
    /// A refresh has started but no authoritative result has been committed.
    Refreshing,
    /// The last refresh attempt failed.
    Failed,
    /// Freshness cannot currently be established.
    Unknown,
}

impl Freshness {
    /// Returns the stable serialized/storage spelling of this freshness value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Stale => "stale",
            Self::Refreshing => "refreshing",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_storage_spellings_are_stable() {
        let repo = RepoId::new("github.com", "Alive24", "shea-symphony");
        let issue = IssueRef::new(TrackerBackend::GithubProjectV2, repo, 479);
        let runtime = WorkspaceRuntimeId::new("alive24-shea-symphony");

        assert_eq!(
            issue.database_key(),
            "github_project_v2|github.com/Alive24/shea-symphony|479"
        );
        assert_eq!(issue.display_ref(), "#479");
        assert_eq!(runtime.as_str(), "alive24-shea-symphony");
        assert_eq!(serde_json::to_value(issue).unwrap()["number"], 479);
    }

    #[test]
    fn enum_storage_spellings_are_stable() {
        assert_eq!(
            TrackerBackend::GithubProjectV2.as_str(),
            "github_project_v2"
        );
        assert_eq!(WorkflowIndexStatus::Starting.as_str(), "starting");
        assert!(WorkflowIndexStatus::Running.is_active());
        assert!(!WorkflowIndexStatus::Completed.is_active());
        assert_eq!(Freshness::Refreshing.as_str(), "refreshing");
    }
}
