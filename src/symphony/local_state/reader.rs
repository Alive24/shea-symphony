//! Active-only reads from the ready local `workflow_index`.
//!
//! This crate-internal boundary performs no initialization, migration,
//! projection, repair, network access, or lifecycle interpretation.

// The first Coordinator/App consumers are tracked separately. Keep this
// focused internal seam warning-clean while its behavior is exercised here.
#![allow(dead_code)]

use std::path::PathBuf;

use rusqlite::{params_from_iter, Connection, ErrorCode, OpenFlags, OptionalExtension};
use sea_query::{Expr, ExprTrait, Order, Query, SqliteQueryBuilder};
use sea_query_rusqlite::RusqliteBinder;
use thiserror::Error;

use super::{
    admin::{LocalStateAdmin, LocalStateHealth, LocalStateHealthDiagnostic},
    database::{is_busy, is_corrupt},
    workflow_index::{workflow_index_columns, workflow_index_row},
    IssueRef, LocalStateDatabase, RepoId, WorkflowIndexRow, WorkflowIndexStatus,
    WorkspaceRuntimeId, ACTIVE_WORKFLOW_STATUSES,
};

/// Maximum number of rows returned by one scoped active-workflow list.
pub(crate) const ACTIVE_WORKFLOW_LIST_LIMIT: u64 = 100;

/// Typed failure from an active local-state read.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum LocalStateReaderError {
    /// The database did not pass the established read-only readiness check.
    #[error("local state is not ready: {0:?}")]
    NotReady(LocalStateHealthDiagnostic),
    /// The issue reference did not belong to the explicitly supplied repository.
    #[error("issue reference repository does not match the requested repository")]
    IssueRepositoryMismatch,
    /// A ready database became unavailable or exposed malformed read data.
    #[error("local-state read failed for {path}: {failure:?}")]
    ReadUnavailable {
        /// Resolved database path used for the read.
        path: PathBuf,
        /// Stable failure classification; raw SQLite errors stay private.
        failure: LocalStateReadFailure,
    },
}

/// Stable classification for a failed read after readiness was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalStateReadFailure {
    /// Another connection prevented the bounded read from completing.
    Busy,
    /// The database could no longer be opened at its configured path.
    CannotOpen,
    /// SQLite identified malformed or corrupt database storage.
    CorruptDatabase,
    /// A selected `workflow_index` value did not match the v1 typed row contract.
    MalformedWorkflowIndex,
    /// The read failed for another non-mutating SQLite reason.
    Other,
}

/// Read-only access to active rows in a compatible local-state database.
///
/// The reader exposes neither a generic Workflow lookup nor latest/history
/// helpers. Returned rows are the persisted v1 projection facts, unchanged.
#[derive(Debug, Clone)]
pub(crate) struct LocalStateReader {
    database: LocalStateDatabase,
}

impl LocalStateReader {
    /// Binds active reads to one resolved local-state database handle.
    pub(crate) fn new(database: LocalStateDatabase) -> Self {
        Self { database }
    }

    /// Finds the active row for one fully qualified repository issue.
    ///
    /// `Ok(None)` means a compatible, ready database contained no `starting`
    /// or `running` match. It does not mean that Temporal or tracker state has
    /// established the absence of an active Workflow.
    pub(crate) fn find_active_workflow_for_issue(
        &self,
        repo_id: &RepoId,
        issue_ref: &IssueRef,
    ) -> Result<Option<WorkflowIndexRow>, LocalStateReaderError> {
        if issue_ref.repo_id != *repo_id {
            return Err(LocalStateReaderError::IssueRepositoryMismatch);
        }
        let connection = self.open_ready_read_only()?;
        let (sql, values) = Query::select()
            .columns(workflow_index_columns())
            .from("workflow_index")
            .and_where(Expr::col("repo_id").eq(repo_id.database_key()))
            .and_where(Expr::col("issue_ref").eq(issue_ref.database_key()))
            // Active membership comes directly from #481's shared status
            // contract; this query never derives lifecycle from timestamps,
            // freshness, Run IDs, tracker state, or row ordering.
            .and_where(Expr::col("status").is_in(active_status_spellings()))
            .build_rusqlite(SqliteQueryBuilder);
        connection
            .query_row(
                &sql,
                params_from_iter(values.as_params()),
                workflow_index_row,
            )
            .optional()
            .map_err(|error| self.classify_read_error(error))
    }

    /// Lists active rows for one runtime and repository scope.
    ///
    /// Results are ordered by the stored fully qualified `issue_ref`, then by
    /// `workflow_id`, both ascending, and are capped at
    /// [`ACTIVE_WORKFLOW_LIST_LIMIT`]. Ordering is only deterministic
    /// presentation order; it does not select a latest lifecycle episode.
    pub(crate) fn list_active_workflows_for_scope(
        &self,
        workspace_runtime_id: &WorkspaceRuntimeId,
        repo_id: &RepoId,
    ) -> Result<Vec<WorkflowIndexRow>, LocalStateReaderError> {
        let connection = self.open_ready_read_only()?;
        let (sql, values) = Query::select()
            .columns(workflow_index_columns())
            .from("workflow_index")
            .and_where(
                Expr::col("workspace_runtime_id").eq(workspace_runtime_id.as_str().to_owned()),
            )
            .and_where(Expr::col("repo_id").eq(repo_id.database_key()))
            .and_where(Expr::col("status").is_in(active_status_spellings()))
            .order_by("issue_ref", Order::Asc)
            .order_by("workflow_id", Order::Asc)
            .limit(ACTIVE_WORKFLOW_LIST_LIMIT)
            .build_rusqlite(SqliteQueryBuilder);
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| self.classify_read_error(error))?;
        let result = statement
            .query_map(params_from_iter(values.as_params()), workflow_index_row)
            .map_err(|error| self.classify_read_error(error))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| self.classify_read_error(error));
        result
    }

    fn open_ready_read_only(&self) -> Result<Connection, LocalStateReaderError> {
        // Readiness is the unavailable-versus-empty boundary. A missing,
        // uninitialized, incompatible, or corrupt database never collapses
        // into the successful absence values returned by the methods above.
        match LocalStateAdmin::new(self.database.clone()).check_health() {
            LocalStateHealth::Current(_) => {}
            LocalStateHealth::NotReady(diagnostic) => {
                return Err(LocalStateReaderError::NotReady(diagnostic));
            }
        }

        // SQLite's ordinary open may create a file. The reader uses only
        // read-only flags and never applies connection or journal PRAGMAs.
        Connection::open_with_flags(self.database.path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| self.classify_read_error(error))
    }

    fn classify_read_error(&self, error: rusqlite::Error) -> LocalStateReaderError {
        let failure = if is_busy(&error) {
            LocalStateReadFailure::Busy
        } else if is_corrupt(&error) {
            LocalStateReadFailure::CorruptDatabase
        } else {
            match error {
                rusqlite::Error::SqliteFailure(inner, _) if inner.code == ErrorCode::CannotOpen => {
                    LocalStateReadFailure::CannotOpen
                }
                rusqlite::Error::FromSqlConversionFailure(..)
                | rusqlite::Error::InvalidColumnType(..)
                | rusqlite::Error::InvalidColumnName(..)
                | rusqlite::Error::Utf8Error(..)
                | rusqlite::Error::IntegralValueOutOfRange(..) => {
                    LocalStateReadFailure::MalformedWorkflowIndex
                }
                _ => LocalStateReadFailure::Other,
            }
        };
        LocalStateReaderError::ReadUnavailable {
            path: self.database.path().to_path_buf(),
            failure,
        }
    }
}

fn active_status_spellings() -> impl Iterator<Item = &'static str> {
    ACTIVE_WORKFLOW_STATUSES
        .iter()
        .copied()
        .map(WorkflowIndexStatus::as_str)
}

// TODO(T2607-02): Only `workflow_index` is readable in this slice; absent
// `tracker_cache`, `activity_progress`, or `artifact_index` data is not
// evidence that those views are fresh, successful, or complete.

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::{params, Connection};
    use tempfile::TempDir;

    use super::*;
    use crate::symphony::local_state::{admin::LocalStateReadiness, Freshness, TrackerBackend};

    struct Fixture {
        _temporary: TempDir,
        database: LocalStateDatabase,
        reader: LocalStateReader,
    }

    fn fixture() -> Fixture {
        let temporary = tempfile::tempdir().unwrap();
        let database =
            LocalStateDatabase::at_resolved_path(temporary.path().join("state.db")).unwrap();
        database.initialize().unwrap();
        let reader = LocalStateReader::new(database.clone());
        Fixture {
            _temporary: temporary,
            database,
            reader,
        }
    }

    fn database_at(temporary: &TempDir, name: &str) -> LocalStateDatabase {
        LocalStateDatabase::at_resolved_path(temporary.path().join(name)).unwrap()
    }

    fn repo() -> RepoId {
        RepoId::new("github.com", "Alive24", "shea-symphony")
    }

    fn issue(repo_id: &RepoId, number: u64) -> IssueRef {
        IssueRef::new(TrackerBackend::GithubProjectV2, repo_id.clone(), number)
    }

    fn insert_row(
        database: &LocalStateDatabase,
        workflow_id: &str,
        runtime_id: &str,
        repo_id: &RepoId,
        issue_ref: &IssueRef,
        status: WorkflowIndexStatus,
        freshness: &str,
    ) {
        let connection = Connection::open(database.path()).unwrap();
        connection
            .execute(
                "INSERT INTO workflow_index (
                    workflow_id, run_id, workspace_runtime_id, repo_id, issue_ref,
                    from_state, target_kind, current_state, active_step, waiting_kind,
                    source_ref, source_tracker_revision, started_at, last_progress_at,
                    status, terminal_outcome, operator_action_ref, freshness, updated_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12,
                    NULL, ?13, NULL, NULL, ?14, ?15
                 )",
                params![
                    workflow_id,
                    format!("run-{workflow_id}"),
                    runtime_id,
                    repo_id.database_key(),
                    issue_ref.database_key(),
                    "Todo",
                    "implementation",
                    "In Progress",
                    "workflow_execution",
                    format!("project-item:{}", issue_ref.number),
                    "revision-1",
                    "2026-07-25T10:00:00Z",
                    status.as_str(),
                    freshness,
                    "2026-07-25T10:01:00Z",
                ],
            )
            .unwrap();
    }

    fn schema_version(database: &LocalStateDatabase) -> u32 {
        Connection::open(database.path())
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn ready_empty_database_returns_absence_without_inference() {
        let fixture = fixture();
        let repo_id = repo();
        let issue_ref = issue(&repo_id, 493);

        assert_eq!(
            fixture
                .reader
                .find_active_workflow_for_issue(&repo_id, &issue_ref)
                .unwrap(),
            None
        );
        assert_eq!(
            fixture
                .reader
                .list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo_id,)
                .unwrap(),
            Vec::<WorkflowIndexRow>::new()
        );
        assert_eq!(
            LocalStateAdmin::new(fixture.database).check_health(),
            LocalStateHealth::Current(LocalStateReadiness {
                path: fixture.reader.database.path().to_path_buf(),
                schema_version: 1,
            })
        );
    }

    #[test]
    fn fully_qualified_lookup_returns_active_not_historical_episode() {
        let fixture = fixture();
        let repo_id = repo();
        let issue_ref = issue(&repo_id, 493);
        insert_row(
            &fixture.database,
            "workflow-historical",
            "runtime-a",
            &repo_id,
            &issue_ref,
            WorkflowIndexStatus::Completed,
            Freshness::Fresh.as_str(),
        );
        insert_row(
            &fixture.database,
            "workflow-active",
            "runtime-b",
            &repo_id,
            &issue_ref,
            WorkflowIndexStatus::Running,
            Freshness::Stale.as_str(),
        );

        let row = fixture
            .reader
            .find_active_workflow_for_issue(&repo_id, &issue_ref)
            .unwrap()
            .unwrap();
        assert_eq!(row.workflow_id, "workflow-active");
        assert_eq!(row.status, WorkflowIndexStatus::Running);
        // Freshness does not participate in active membership.
        assert_eq!(row.freshness, Freshness::Stale);
    }

    #[test]
    fn scoped_list_is_filtered_deterministic_and_bounded() {
        let fixture = fixture();
        let repo_id = repo();
        for number in 1..=(ACTIVE_WORKFLOW_LIST_LIMIT + 5) {
            let issue_ref = issue(&repo_id, number);
            insert_row(
                &fixture.database,
                &format!("workflow-{number:03}"),
                "runtime-a",
                &repo_id,
                &issue_ref,
                WorkflowIndexStatus::Running,
                Freshness::Fresh.as_str(),
            );
        }
        let other_runtime_issue = issue(&repo_id, 500);
        insert_row(
            &fixture.database,
            "workflow-other-runtime",
            "runtime-b",
            &repo_id,
            &other_runtime_issue,
            WorkflowIndexStatus::Starting,
            Freshness::Unknown.as_str(),
        );
        let starting = fixture
            .reader
            .find_active_workflow_for_issue(&repo_id, &other_runtime_issue)
            .unwrap()
            .unwrap();
        assert_eq!(starting.status, WorkflowIndexStatus::Starting);
        let other_repo = RepoId::new("github.com", "Alive24", "other");
        let other_repo_issue = issue(&other_repo, 1);
        insert_row(
            &fixture.database,
            "workflow-other-repo",
            "runtime-a",
            &other_repo,
            &other_repo_issue,
            WorkflowIndexStatus::Running,
            Freshness::Fresh.as_str(),
        );
        let historical_issue = issue(&repo_id, 999);
        insert_row(
            &fixture.database,
            "workflow-historical",
            "runtime-a",
            &repo_id,
            &historical_issue,
            WorkflowIndexStatus::Failed,
            Freshness::Fresh.as_str(),
        );

        let runtime_id = WorkspaceRuntimeId::new("runtime-a");
        let first = fixture
            .reader
            .list_active_workflows_for_scope(&runtime_id, &repo_id)
            .unwrap();
        let second = fixture
            .reader
            .list_active_workflows_for_scope(&runtime_id, &repo_id)
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(first.len(), ACTIVE_WORKFLOW_LIST_LIMIT as usize);
        assert!(first.iter().all(|row| {
            row.workspace_runtime_id == "runtime-a"
                && row.repo_id == repo_id.database_key()
                && row.status.is_active()
        }));
        let ordering = first
            .iter()
            .map(|row| (row.issue_ref.clone(), row.workflow_id.clone()))
            .collect::<Vec<_>>();
        let mut sorted = ordering.clone();
        sorted.sort();
        assert_eq!(ordering, sorted);
        assert!(!first
            .iter()
            .any(|row| row.workflow_id == "workflow-other-runtime"
                || row.workflow_id == "workflow-other-repo"
                || row.workflow_id == "workflow-historical"));
    }

    #[test]
    fn missing_and_uninitialized_paths_are_not_empty_results_or_mutated() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = database_at(&temporary, "missing/parent/state.db");
        let missing_reader = LocalStateReader::new(missing.clone());
        let parent = missing.path().parent().unwrap();

        assert_eq!(
            missing_reader
                .list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo(),),
            Err(LocalStateReaderError::NotReady(
                LocalStateHealthDiagnostic::MissingPath {
                    path: missing.path().to_path_buf(),
                },
            ))
        );
        assert!(!parent.exists());

        let uninitialized = database_at(&temporary, "uninitialized.db");
        Connection::open(uninitialized.path()).unwrap();
        let before = fs::read(uninitialized.path()).unwrap();
        let reader = LocalStateReader::new(uninitialized.clone());
        assert_eq!(
            reader.list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo(),),
            Err(LocalStateReaderError::NotReady(
                LocalStateHealthDiagnostic::MigrationRequired {
                    path: uninitialized.path().to_path_buf(),
                    observed_schema_version: 0,
                    supported_schema_version: 1,
                },
            ))
        );
        assert_eq!(fs::read(uninitialized.path()).unwrap(), before);
        assert_eq!(schema_version(&uninitialized), 0);
    }

    #[test]
    fn corrupt_future_and_unversioned_databases_are_typed_and_unchanged() {
        let temporary = tempfile::tempdir().unwrap();
        let corrupt = database_at(&temporary, "corrupt.db");
        fs::write(corrupt.path(), b"not a sqlite database").unwrap();
        let corrupt_before = fs::read(corrupt.path()).unwrap();
        assert_eq!(
            LocalStateReader::new(corrupt.clone())
                .list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo(),),
            Err(LocalStateReaderError::NotReady(
                LocalStateHealthDiagnostic::CorruptDatabase {
                    path: corrupt.path().to_path_buf(),
                },
            ))
        );
        assert_eq!(fs::read(corrupt.path()).unwrap(), corrupt_before);

        let future = database_at(&temporary, "future.db");
        future.initialize().unwrap();
        Connection::open(future.path())
            .unwrap()
            .pragma_update(None, "user_version", 2)
            .unwrap();
        assert_eq!(
            LocalStateReader::new(future.clone())
                .list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo(),),
            Err(LocalStateReaderError::NotReady(
                LocalStateHealthDiagnostic::UnsupportedSchemaVersion {
                    path: future.path().to_path_buf(),
                    observed_schema_version: 2,
                    supported_schema_version: 1,
                },
            ))
        );
        assert_eq!(schema_version(&future), 2);

        let unversioned = database_at(&temporary, "unversioned.db");
        Connection::open(unversioned.path())
            .unwrap()
            .execute("CREATE TABLE unexpected(value TEXT)", [])
            .unwrap();
        assert_eq!(
            LocalStateReader::new(unversioned.clone())
                .list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo(),),
            Err(LocalStateReaderError::NotReady(
                LocalStateHealthDiagnostic::UnversionedSchemaConflict {
                    path: unversioned.path().to_path_buf(),
                },
            ))
        );
        assert_eq!(schema_version(&unversioned), 0);
        let unexpected_count: u32 = Connection::open(unversioned.path())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM unexpected", [], |row| row.get(0))
            .unwrap();
        assert_eq!(unexpected_count, 0);
    }

    #[test]
    fn incomplete_schema_and_malformed_rows_are_typed_not_empty() {
        let temporary = tempfile::tempdir().unwrap();
        let incomplete = database_at(&temporary, "incomplete.db");
        incomplete.initialize().unwrap();
        Connection::open(incomplete.path())
            .unwrap()
            .execute("DROP TABLE workflow_index", [])
            .unwrap();
        assert_eq!(
            LocalStateReader::new(incomplete.clone())
                .list_active_workflows_for_scope(&WorkspaceRuntimeId::new("runtime-a"), &repo(),),
            Err(LocalStateReaderError::NotReady(
                LocalStateHealthDiagnostic::IncompleteCurrentSchema {
                    path: incomplete.path().to_path_buf(),
                    schema_version: 1,
                    missing_tables: vec!["workflow_index".to_string()],
                    missing_metadata_keys: vec![],
                },
            ))
        );

        let malformed = database_at(&temporary, "malformed-row.db");
        malformed.initialize().unwrap();
        let repo_id = repo();
        let issue_ref = issue(&repo_id, 493);
        insert_row(
            &malformed,
            "workflow-malformed",
            "runtime-a",
            &repo_id,
            &issue_ref,
            WorkflowIndexStatus::Running,
            "not-a-freshness",
        );
        assert_eq!(
            LocalStateReader::new(malformed.clone())
                .find_active_workflow_for_issue(&repo_id, &issue_ref),
            Err(LocalStateReaderError::ReadUnavailable {
                path: malformed.path().to_path_buf(),
                failure: LocalStateReadFailure::MalformedWorkflowIndex,
            })
        );
        let stored_freshness: String = Connection::open(malformed.path())
            .unwrap()
            .query_row(
                "SELECT freshness FROM workflow_index WHERE workflow_id = ?1",
                ["workflow-malformed"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_freshness, "not-a-freshness");
    }

    #[test]
    fn lookup_rejects_a_repo_issue_scope_mismatch_before_opening_state() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = database_at(&temporary, "missing.db");
        let requested_repo = repo();
        let other_repo = RepoId::new("github.com", "Alive24", "other");

        assert_eq!(
            LocalStateReader::new(missing.clone())
                .find_active_workflow_for_issue(&requested_repo, &issue(&other_repo, 493),),
            Err(LocalStateReaderError::IssueRepositoryMismatch)
        );
        assert!(!missing.path().exists());
    }
}
