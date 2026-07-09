//! Compile-time contract for Symphony's rebuildable SQLite read model.
//!
//! These descriptors freeze names, columns, keys, indexes, and serialized enum
//! values for later migration and projection work. This module opens no database
//! and performs no I/O. SQLite is an acceleration and observability surface: it
//! may cache tracker facts and Temporal execution metadata, but it cannot
//! authorize Workflow progression, tracker transitions, PR linkage, or merging.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Stable repository identity used in local read-model keys.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Tracker adapter that owns the authoritative external issue state.
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Tracker-scoped issue identity used by local cache rows.
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

    /// Formats the short tracker reference used in operator surfaces.
    pub fn display_ref(&self) -> String {
        format!("#{}", self.number)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
/// Application-assigned Temporal Workflow ID stored by the local read model.
///
/// This is distinct from Temporal's Run ID and remains stable for the identity
/// of one Workflow execution. Construction does not validate naming policy.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Lifecycle classification projected into the local Workflow index.
///
/// These values describe local knowledge and reconciliation outcomes; Temporal
/// history and tracker state remain authoritative.
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

    /// Returns whether this status participates in the local duplicate-start guard.
    ///
    /// This is a local safety check only; it does not replace Temporal start
    /// semantics or tracker eligibility checks.
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

/// Statuses treated as locally active for duplicate-start projection guards.
pub const ACTIVE_WORKFLOW_STATUSES: &[WorkflowIndexStatus] =
    &[WorkflowIndexStatus::Starting, WorkflowIndexStatus::Running];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Freshness of a cached or projected local-state value.
pub enum Freshness {
    /// The projection is current within its configured freshness policy.
    Fresh,
    /// The projection is known to be older than its freshness policy permits.
    Stale,
    /// A refresh has started but no authoritative result has been committed yet.
    Refreshing,
    /// The last refresh attempt failed and diagnostics should be consulted.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// SQLite storage affinity required by a schema column.
pub enum ColumnKind {
    /// UTF-8 text or text-encoded identifier/timestamp data.
    Text,
    /// Integer data stored with SQLite integer affinity.
    Integer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Name and storage affinity of one local-state table column.
pub struct ColumnDescriptor {
    /// Stable SQL column name.
    pub name: &'static str,
    /// SQLite storage affinity expected by the migration.
    pub kind: ColumnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Named primary-key contract for a local-state table.
pub struct PrimaryKeyDescriptor {
    /// Stable SQL constraint name.
    pub name: &'static str,
    /// Ordered columns that make up the primary key.
    pub columns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Named non-primary index contract for a local-state table.
pub struct IndexDescriptor {
    /// Stable SQL index name.
    pub name: &'static str,
    /// Ordered columns included in the index key.
    pub columns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Proposed local duplicate-start guard for active Workflow index rows.
///
/// This descriptor is migration input, not an implemented lock or an authority
/// for starting Workflows.
pub struct ActiveWorkflowGuardDescriptor {
    /// Human-readable invariant the future SQL index must enforce locally.
    pub intent: &'static str,
    /// Stable name proposed for the future partial unique index.
    pub proposed_index_name: &'static str,
    /// Issue identity columns constrained by the guard.
    pub columns: &'static [&'static str],
    /// Projected statuses to which the guard applies.
    pub active_statuses: &'static [WorkflowIndexStatus],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Complete compile-time contract for one local-state table.
pub struct TableDescriptor {
    /// Stable SQL table name.
    pub name: &'static str,
    /// Ordered columns expected in the initial migration.
    pub columns: &'static [ColumnDescriptor],
    /// Primary-key contract for the table.
    pub primary_key: PrimaryKeyDescriptor,
    /// Secondary indexes required by known read paths.
    pub indexes: &'static [IndexDescriptor],
    /// Optional local duplicate-start guard metadata.
    pub active_workflow_guard: Option<ActiveWorkflowGuardDescriptor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Ordered set of table descriptors for the initial local-state schema.
pub struct LocalStateSchema {
    /// Tables in stable migration/documentation order.
    pub tables: &'static [TableDescriptor],
}

impl LocalStateSchema {
    /// Finds a table descriptor by its stable SQL name.
    pub fn table(&self, name: &str) -> Option<&'static TableDescriptor> {
        self.tables.iter().find(|table| table.name == name)
    }
}

const WORKFLOW_INDEX_COLUMNS: &[ColumnDescriptor] = &[
    text_column("workflow_id"),
    text_column("run_id"),
    text_column("repo_id"),
    text_column("issue_ref"),
    text_column("from_state"),
    text_column("target_kind"),
    text_column("current_state"),
    text_column("active_step"),
    text_column("waiting_kind"),
    text_column("source_ref"),
    text_column("started_at"),
    text_column("last_progress_at"),
    text_column("status"),
    text_column("terminal_outcome"),
    text_column("freshness"),
    text_column("updated_at"),
];

const WORKFLOW_INDEX_INDEXES: &[IndexDescriptor] = &[
    IndexDescriptor {
        name: "idx_workflow_index_repo_issue_status",
        columns: &["repo_id", "issue_ref", "status"],
    },
    IndexDescriptor {
        name: "idx_workflow_index_current_state_waiting_kind",
        columns: &["current_state", "waiting_kind"],
    },
];

const ARTIFACT_INDEX_COLUMNS: &[ColumnDescriptor] = &[
    text_column("artifact_id"),
    text_column("workflow_id"),
    text_column("repo_id"),
    text_column("issue_ref"),
    text_column("kind"),
    text_column("path"),
    text_column("summary"),
    text_column("created_by_step"),
    text_column("created_at"),
];

const ARTIFACT_INDEX_INDEXES: &[IndexDescriptor] = &[
    IndexDescriptor {
        name: "idx_artifact_index_workflow_id",
        columns: &["workflow_id"],
    },
    IndexDescriptor {
        name: "idx_artifact_index_repo_issue",
        columns: &["repo_id", "issue_ref"],
    },
];

const TRACKER_CACHE_COLUMNS: &[ColumnDescriptor] = &[
    text_column("repo_id"),
    text_column("issue_ref"),
    text_column("tracker_backend"),
    text_column("tracker_state"),
    text_column("title"),
    integer_column("pr_number"),
    text_column("pr_state"),
    text_column("pr_relation_confirmed_at"),
    text_column("updated_at"),
    text_column("freshness"),
];

const TRACKER_CACHE_INDEXES: &[IndexDescriptor] = &[IndexDescriptor {
    name: "idx_tracker_cache_freshness",
    columns: &["freshness"],
}];

const ACTIVITY_PROGRESS_COLUMNS: &[ColumnDescriptor] = &[
    text_column("workflow_id"),
    text_column("activity_id"),
    text_column("activity_kind"),
    text_column("target_ref"),
    text_column("mutation_id"),
    text_column("outcome"),
    text_column("status"),
    integer_column("attempt_count"),
    text_column("last_heartbeat_at"),
    text_column("next_retry_at"),
    text_column("summary"),
];

const ACTIVITY_PROGRESS_INDEXES: &[IndexDescriptor] = &[IndexDescriptor {
    name: "idx_activity_progress_workflow_mutation",
    columns: &["workflow_id", "mutation_id"],
}];

const META_COLUMNS: &[ColumnDescriptor] = &[text_column("key"), text_column("value")];

const META_INDEXES: &[IndexDescriptor] = &[];

const WORKFLOW_INDEX_TABLE: TableDescriptor = TableDescriptor {
    name: "workflow_index",
    columns: WORKFLOW_INDEX_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_workflow_index",
        columns: &["workflow_id"],
    },
    indexes: WORKFLOW_INDEX_INDEXES,
    active_workflow_guard: Some(ActiveWorkflowGuardDescriptor {
        intent:
            "one active workflow row per repo_id and issue_ref for local duplicate-start guarding",
        proposed_index_name: "uq_workflow_index_active_issue",
        columns: &["repo_id", "issue_ref"],
        active_statuses: ACTIVE_WORKFLOW_STATUSES,
    }),
};

const ARTIFACT_INDEX_TABLE: TableDescriptor = TableDescriptor {
    name: "artifact_index",
    columns: ARTIFACT_INDEX_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_artifact_index",
        columns: &["artifact_id"],
    },
    indexes: ARTIFACT_INDEX_INDEXES,
    active_workflow_guard: None,
};

const TRACKER_CACHE_TABLE: TableDescriptor = TableDescriptor {
    name: "tracker_cache",
    columns: TRACKER_CACHE_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_tracker_cache",
        columns: &["repo_id", "issue_ref"],
    },
    indexes: TRACKER_CACHE_INDEXES,
    active_workflow_guard: None,
};

const ACTIVITY_PROGRESS_TABLE: TableDescriptor = TableDescriptor {
    name: "activity_progress",
    columns: ACTIVITY_PROGRESS_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_activity_progress",
        columns: &["workflow_id", "activity_id"],
    },
    indexes: ACTIVITY_PROGRESS_INDEXES,
    active_workflow_guard: None,
};

const META_TABLE: TableDescriptor = TableDescriptor {
    name: "meta",
    columns: META_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_meta",
        columns: &["key"],
    },
    indexes: META_INDEXES,
    active_workflow_guard: None,
};

/// Initial compile-time schema contract for Symphony's local read model.
///
/// The descriptor order is stable for migrations, documentation, and tests.
/// Possessing this value does not imply that a SQLite database exists or is
/// fresh, and it never authorizes Workflow or tracker mutation.
pub const LOCAL_STATE_SCHEMA: LocalStateSchema = LocalStateSchema {
    // Keep descriptor order stable so migrations, docs, and tests speak about
    // the same initial schema without needing a live database.
    tables: &[
        WORKFLOW_INDEX_TABLE,
        ARTIFACT_INDEX_TABLE,
        TRACKER_CACHE_TABLE,
        ACTIVITY_PROGRESS_TABLE,
        META_TABLE,
    ],
};

const fn text_column(name: &'static str) -> ColumnDescriptor {
    ColumnDescriptor {
        name,
        kind: ColumnKind::Text,
    }
}

const fn integer_column(name: &'static str) -> ColumnDescriptor {
    ColumnDescriptor {
        name,
        kind: ColumnKind::Integer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str) -> &'static TableDescriptor {
        LOCAL_STATE_SCHEMA.table(name).unwrap_or_else(|| {
            panic!("missing local state table descriptor: {name}");
        })
    }

    fn column_names(table: &TableDescriptor) -> Vec<&'static str> {
        table.columns.iter().map(|column| column.name).collect()
    }

    fn assert_columns(table_name: &str, expected: &[&str]) {
        let table = table(table_name);
        assert_eq!(column_names(table), expected);
    }

    fn assert_index(table_name: &str, index_name: &str, columns: &[&str]) {
        let table = table(table_name);
        let index = table
            .indexes
            .iter()
            .find(|index| index.name == index_name)
            .unwrap_or_else(|| panic!("missing index {index_name} on {table_name}"));
        assert_eq!(index.columns, columns);
    }

    #[test]
    fn identity_dtos_are_serializable() {
        let repo_id = RepoId::new("github.com", "Alive24", "shea-symphony");
        let issue_ref = IssueRef::new(TrackerBackend::GithubProjectV2, repo_id, 477);
        let workflow_id = WorkflowId::new(
            "issue:alive24-shea-symphony:477:pulse:todo-to-main:20260709-1810Z:loop",
        );

        assert_eq!(
            issue_ref.repo_id.database_key(),
            "github.com/Alive24/shea-symphony"
        );
        assert_eq!(issue_ref.display_ref(), "#477");
        assert_eq!(
            workflow_id.as_str(),
            "issue:alive24-shea-symphony:477:pulse:todo-to-main:20260709-1810Z:loop"
        );

        let value = serde_json::to_value(&issue_ref).unwrap();
        assert_eq!(value["tracker_backend"], "github_project_v2");
        assert_eq!(value["number"], 477);

        let workflow_json = serde_json::to_value(&workflow_id).unwrap();
        assert_eq!(workflow_json, workflow_id.as_str());
    }

    #[test]
    fn workflow_index_status_serialization_and_active_classification_are_fixed() {
        let cases = [
            (WorkflowIndexStatus::Starting, "starting", true),
            (WorkflowIndexStatus::Running, "running", true),
            (WorkflowIndexStatus::Completed, "completed", false),
            (WorkflowIndexStatus::Failed, "failed", false),
            (WorkflowIndexStatus::StartFailed, "start_failed", false),
            (WorkflowIndexStatus::StaleStart, "stale_start", false),
            (WorkflowIndexStatus::StaleMissing, "stale_missing", false),
            (WorkflowIndexStatus::ClosedUnknown, "closed_unknown", false),
        ];

        for (status, serialized, active) in cases {
            assert_eq!(status.as_str(), serialized);
            assert_eq!(status.is_active(), active);
            assert_eq!(serde_json::to_value(status).unwrap(), serialized);
            assert_eq!(
                serde_json::from_value::<WorkflowIndexStatus>(serialized.into()).unwrap(),
                status
            );
        }

        assert_eq!(
            ACTIVE_WORKFLOW_STATUSES,
            &[WorkflowIndexStatus::Starting, WorkflowIndexStatus::Running]
        );
    }

    #[test]
    fn freshness_serialization_is_fixed() {
        let cases = [
            (Freshness::Fresh, "fresh"),
            (Freshness::Stale, "stale"),
            (Freshness::Refreshing, "refreshing"),
            (Freshness::Failed, "failed"),
            (Freshness::Unknown, "unknown"),
        ];

        for (freshness, serialized) in cases {
            assert_eq!(freshness.as_str(), serialized);
            assert_eq!(serde_json::to_value(freshness).unwrap(), serialized);
            assert_eq!(
                serde_json::from_value::<Freshness>(serialized.into()).unwrap(),
                freshness
            );
        }
    }

    #[test]
    fn schema_names_all_initial_tables() {
        let table_names: Vec<_> = LOCAL_STATE_SCHEMA
            .tables
            .iter()
            .map(|table| table.name)
            .collect();

        assert_eq!(
            table_names,
            [
                "workflow_index",
                "artifact_index",
                "tracker_cache",
                "activity_progress",
                "meta"
            ]
        );
    }

    #[test]
    fn workflow_index_descriptor_matches_contract() {
        assert_columns(
            "workflow_index",
            &[
                "workflow_id",
                "run_id",
                "repo_id",
                "issue_ref",
                "from_state",
                "target_kind",
                "current_state",
                "active_step",
                "waiting_kind",
                "source_ref",
                "started_at",
                "last_progress_at",
                "status",
                "terminal_outcome",
                "freshness",
                "updated_at",
            ],
        );

        let table = table("workflow_index");
        assert_eq!(table.primary_key.name, "pk_workflow_index");
        assert_eq!(table.primary_key.columns, ["workflow_id"]);
        assert_index(
            "workflow_index",
            "idx_workflow_index_repo_issue_status",
            &["repo_id", "issue_ref", "status"],
        );
        assert_index(
            "workflow_index",
            "idx_workflow_index_current_state_waiting_kind",
            &["current_state", "waiting_kind"],
        );

        let guard = table.active_workflow_guard.unwrap();
        assert_eq!(guard.proposed_index_name, "uq_workflow_index_active_issue");
        assert_eq!(guard.columns, ["repo_id", "issue_ref"]);
        assert_eq!(
            guard.active_statuses,
            &[WorkflowIndexStatus::Starting, WorkflowIndexStatus::Running]
        );
        assert!(guard.intent.contains("duplicate-start"));
    }

    #[test]
    fn artifact_index_descriptor_matches_contract() {
        assert_columns(
            "artifact_index",
            &[
                "artifact_id",
                "workflow_id",
                "repo_id",
                "issue_ref",
                "kind",
                "path",
                "summary",
                "created_by_step",
                "created_at",
            ],
        );

        let table = table("artifact_index");
        assert_eq!(table.primary_key.name, "pk_artifact_index");
        assert_eq!(table.primary_key.columns, ["artifact_id"]);
        assert_index(
            "artifact_index",
            "idx_artifact_index_workflow_id",
            &["workflow_id"],
        );
        assert_index(
            "artifact_index",
            "idx_artifact_index_repo_issue",
            &["repo_id", "issue_ref"],
        );
        assert!(table.active_workflow_guard.is_none());
    }

    #[test]
    fn tracker_cache_descriptor_matches_contract() {
        assert_columns(
            "tracker_cache",
            &[
                "repo_id",
                "issue_ref",
                "tracker_backend",
                "tracker_state",
                "title",
                "pr_number",
                "pr_state",
                "pr_relation_confirmed_at",
                "updated_at",
                "freshness",
            ],
        );

        let table = table("tracker_cache");
        assert_eq!(table.primary_key.name, "pk_tracker_cache");
        assert_eq!(table.primary_key.columns, ["repo_id", "issue_ref"]);
        assert_index(
            "tracker_cache",
            "idx_tracker_cache_freshness",
            &["freshness"],
        );
        assert_eq!(
            table
                .columns
                .iter()
                .find(|column| column.name == "pr_number")
                .unwrap()
                .kind,
            ColumnKind::Integer
        );
    }

    #[test]
    fn activity_progress_descriptor_matches_contract() {
        assert_columns(
            "activity_progress",
            &[
                "workflow_id",
                "activity_id",
                "activity_kind",
                "target_ref",
                "mutation_id",
                "outcome",
                "status",
                "attempt_count",
                "last_heartbeat_at",
                "next_retry_at",
                "summary",
            ],
        );

        let table = table("activity_progress");
        assert_eq!(table.primary_key.name, "pk_activity_progress");
        assert_eq!(table.primary_key.columns, ["workflow_id", "activity_id"]);
        assert_index(
            "activity_progress",
            "idx_activity_progress_workflow_mutation",
            &["workflow_id", "mutation_id"],
        );
        assert_eq!(
            table
                .columns
                .iter()
                .find(|column| column.name == "attempt_count")
                .unwrap()
                .kind,
            ColumnKind::Integer
        );
    }

    #[test]
    fn meta_descriptor_matches_contract() {
        assert_columns("meta", &["key", "value"]);

        let table = table("meta");
        assert_eq!(table.primary_key.name, "pk_meta");
        assert_eq!(table.primary_key.columns, ["key"]);
        assert!(table.indexes.is_empty());
    }
}
