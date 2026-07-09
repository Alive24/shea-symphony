use serde::{Deserialize, Serialize};

// Contract-only local read model schema. This module names the tables and DTO
// values that later migrations must implement; it must not open SQLite or
// authorize workflow/tracker progression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoId {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

impl RepoId {
    pub fn new(host: impl Into<String>, owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    pub fn database_key(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackerBackend {
    GithubProjectV2,
    Linear,
}

impl TrackerBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GithubProjectV2 => "github_project_v2",
            Self::Linear => "linear",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueRef {
    pub tracker_backend: TrackerBackend,
    pub repo_id: RepoId,
    pub number: u64,
}

impl IssueRef {
    pub fn new(tracker_backend: TrackerBackend, repo_id: RepoId, number: u64) -> Self {
        Self {
            tracker_backend,
            repo_id,
            number,
        }
    }

    pub fn display_ref(&self) -> String {
        format!("#{}", self.number)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowId(String);

impl WorkflowId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowIndexStatus {
    Starting,
    Running,
    Completed,
    Failed,
    StartFailed,
    StaleStart,
    StaleMissing,
    ClosedUnknown,
}

impl WorkflowIndexStatus {
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

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

pub const ACTIVE_WORKFLOW_STATUSES: &[WorkflowIndexStatus] =
    &[WorkflowIndexStatus::Starting, WorkflowIndexStatus::Running];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Fresh,
    Stale,
    Refreshing,
    Failed,
    Unknown,
}

impl Freshness {
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
pub enum ColumnKind {
    Text,
    Integer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnDescriptor {
    pub name: &'static str,
    pub kind: ColumnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimaryKeyDescriptor {
    pub name: &'static str,
    pub columns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexDescriptor {
    pub name: &'static str,
    pub columns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveWorkflowGuardDescriptor {
    // Describes the duplicate-start invariant before a later migration turns it
    // into SQL. Temporal/tracker state remains authoritative.
    pub intent: &'static str,
    pub proposed_index_name: &'static str,
    pub columns: &'static [&'static str],
    pub active_statuses: &'static [WorkflowIndexStatus],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableDescriptor {
    pub name: &'static str,
    pub columns: &'static [ColumnDescriptor],
    pub primary_key: PrimaryKeyDescriptor,
    pub indexes: &'static [IndexDescriptor],
    pub active_workflow_guard: Option<ActiveWorkflowGuardDescriptor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalStateSchema {
    pub tables: &'static [TableDescriptor],
}

impl LocalStateSchema {
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

pub const WORKFLOW_INDEX_TABLE: TableDescriptor = TableDescriptor {
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

pub const ARTIFACT_INDEX_TABLE: TableDescriptor = TableDescriptor {
    name: "artifact_index",
    columns: ARTIFACT_INDEX_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_artifact_index",
        columns: &["artifact_id"],
    },
    indexes: ARTIFACT_INDEX_INDEXES,
    active_workflow_guard: None,
};

pub const TRACKER_CACHE_TABLE: TableDescriptor = TableDescriptor {
    name: "tracker_cache",
    columns: TRACKER_CACHE_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_tracker_cache",
        columns: &["repo_id", "issue_ref"],
    },
    indexes: TRACKER_CACHE_INDEXES,
    active_workflow_guard: None,
};

pub const ACTIVITY_PROGRESS_TABLE: TableDescriptor = TableDescriptor {
    name: "activity_progress",
    columns: ACTIVITY_PROGRESS_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_activity_progress",
        columns: &["workflow_id", "activity_id"],
    },
    indexes: ACTIVITY_PROGRESS_INDEXES,
    active_workflow_guard: None,
};

pub const META_TABLE: TableDescriptor = TableDescriptor {
    name: "meta",
    columns: META_COLUMNS,
    primary_key: PrimaryKeyDescriptor {
        name: "pk_meta",
        columns: &["key"],
    },
    indexes: META_INDEXES,
    active_workflow_guard: None,
};

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
