use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use rusqlite::{Connection, ErrorCode, Transaction, TransactionBehavior};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::migration::{Migration, MIGRATIONS};

/// Latest schema version understood by this binary.
pub(super) const CURRENT_SCHEMA_VERSION: u32 = super::migration::CURRENT_SCHEMA_VERSION;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const DATABASE_RELATIVE_PATH: &str = "state/symphony.db";

/// Confirmed SQLite journaling mode for an initialized local-state database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalMode {
    /// SQLite write-ahead logging, allowing concurrent local readers.
    Wal,
}

/// Observable result of opening and, when needed, migrating local state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalStateInitialization {
    /// Absolute database path used for this initialization.
    pub path: PathBuf,
    /// Version observed before this call acquired any migration lock.
    pub observed_schema_version: u32,
    /// Version confirmed after initialization completed.
    pub schema_version: u32,
    /// Whether this call applied at least one migration.
    pub migration_ran: bool,
    /// Journal mode read back from SQLite after configuration.
    pub journal_mode: JournalMode,
}

/// Typed local-state initialization failures.
#[derive(Debug, Error)]
pub enum LocalStateError {
    /// The operator home directory could not be resolved.
    #[error("home_resolution: HOME is missing or empty")]
    HomeResolution,
    /// A caller claimed to provide a resolved path but passed a relative path.
    #[error("home_resolution: local-state path must be absolute: {path}")]
    UnresolvedPath {
        /// Rejected path.
        path: PathBuf,
    },
    /// The database parent directory could not be created.
    #[error("parent_directory_creation: could not create {path}: {source}")]
    ParentDirectoryCreation {
        /// Parent directory that could not be created.
        path: PathBuf,
        /// Underlying filesystem error.
        #[source]
        source: std::io::Error,
    },
    /// SQLite could not open the configured file.
    #[error("database_open: could not open {path}: {source}")]
    Open {
        /// Database path.
        path: PathBuf,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// Required per-connection SQLite policy could not be applied or verified.
    #[error("database_configuration: could not configure {path}: {source}")]
    Configuration {
        /// Database path.
        path: PathBuf,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// The database stayed locked beyond the bounded five-second timeout.
    #[error("database_busy: timed out waiting for {path}")]
    DatabaseBusy {
        /// Contended database path.
        path: PathBuf,
    },
    /// SQLite identified the configured file as corrupt or malformed.
    #[error("database_corrupt: malformed database at {path}: {source}")]
    CorruptDatabase {
        /// Corrupt database path.
        path: PathBuf,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// The file was created by a newer, incompatible binary.
    #[error(
        "unsupported_schema_version: {path} has version {observed}, supported maximum is {supported}"
    )]
    UnsupportedSchemaVersion {
        /// Database path.
        path: PathBuf,
        /// Version found in `PRAGMA user_version`.
        observed: u32,
        /// Latest version supported by this binary.
        supported: u32,
    },
    /// Version zero contained application objects and cannot be adopted safely.
    #[error("unversioned_schema_conflict: {path} contains an unversioned application schema")]
    UnversionedSchemaConflict {
        /// Conflicting database path.
        path: PathBuf,
    },
    /// An ordered migration failed and was rolled back.
    #[error("migration_failure: migration {version} failed for {path}: {source}")]
    Migration {
        /// Database path.
        path: PathBuf,
        /// Migration version that failed.
        version: u32,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// Schema version could not be confirmed after migration.
    #[error("post_migration_readback: could not confirm version for {path}: {source}")]
    PostMigrationReadback {
        /// Database path.
        path: PathBuf,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// A projector received a required value that was absent or invalid.
    #[error("projection_input: {field} must be present and valid")]
    ProjectionInput {
        /// Name of the rejected projection field.
        field: &'static str,
    },
    /// A synchronous local-state projection could not complete.
    #[error("projection_failure: could not project local state at {path}: {source}")]
    Projection {
        /// Database path.
        path: PathBuf,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// A persisted local-state timestamp could not be represented as RFC 3339 UTC text.
    #[error("local_state_timestamp: could not format timestamp: {0}")]
    Timestamp(#[from] time::error::Format),
}

/// Cloneable database path and deterministic connection-policy handle.
///
/// Physical [`Connection`] values never escape this boundary. Each operation
/// opens and configures its own synchronous connection; callers must keep this
/// work off Tokio and Temporal async worker threads.
#[derive(Debug, Clone)]
pub struct LocalStateDatabase {
    path: Arc<PathBuf>,
}

impl LocalStateDatabase {
    /// Resolves the default `~/.shea/state/symphony.db` path from `HOME`.
    pub fn from_environment() -> Result<Self, LocalStateError> {
        let home = env::var_os("HOME").filter(|value| !value.is_empty());
        Self::for_home(home.ok_or(LocalStateError::HomeResolution)?)
    }

    /// Resolves the default database path beneath an injected home directory.
    pub fn for_home(home: impl AsRef<Path>) -> Result<Self, LocalStateError> {
        let home = home.as_ref();
        if !home.is_absolute() {
            return Err(LocalStateError::UnresolvedPath {
                path: home.to_path_buf(),
            });
        }
        Ok(Self {
            path: Arc::new(home.join(".shea").join(DATABASE_RELATIVE_PATH)),
        })
    }

    /// Resolves an override; relative values are anchored beneath `~/.shea/`.
    pub fn with_override(
        home: impl AsRef<Path>,
        override_path: impl AsRef<Path>,
    ) -> Result<Self, LocalStateError> {
        let override_path = override_path.as_ref();
        let path = if override_path.is_absolute() {
            override_path.to_path_buf()
        } else {
            if !home.as_ref().is_absolute() {
                return Err(LocalStateError::UnresolvedPath {
                    path: home.as_ref().to_path_buf(),
                });
            }
            home.as_ref().join(".shea").join(override_path)
        };
        Ok(Self {
            path: Arc::new(path),
        })
    }

    /// Uses a caller-resolved absolute database path.
    pub fn at_resolved_path(path: impl Into<PathBuf>) -> Result<Self, LocalStateError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(LocalStateError::UnresolvedPath { path });
        }
        Ok(Self {
            path: Arc::new(path),
        })
    }

    /// Returns the configured database path without opening the file.
    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    /// Creates, configures, and forward-migrates the local-state database.
    pub fn initialize(&self) -> Result<LocalStateInitialization, LocalStateError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| LocalStateError::UnresolvedPath {
                path: self.path.as_ref().clone(),
            })?;
        std::fs::create_dir_all(parent).map_err(|source| {
            LocalStateError::ParentDirectoryCreation {
                path: parent.to_path_buf(),
                source,
            }
        })?;

        let mut connection = self.open_connection()?;
        configure_connection_policy(&connection)
            .map_err(|error| self.classify(error, SqlitePhase::Configuration))?;
        let (observed_schema_version, unversioned_conflict) =
            inspect_initial_schema(&mut connection)
                .map_err(|error| self.classify(error, SqlitePhase::Configuration))?;
        if observed_schema_version > CURRENT_SCHEMA_VERSION {
            return Err(LocalStateError::UnsupportedSchemaVersion {
                path: self.path.as_ref().clone(),
                observed: observed_schema_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if unversioned_conflict {
            return Err(LocalStateError::UnversionedSchemaConflict {
                path: self.path.as_ref().clone(),
            });
        }
        let journal_mode = configure_journal_mode_bounded(&connection)
            .map_err(|error| self.classify(error, SqlitePhase::Configuration))?;
        if observed_schema_version == CURRENT_SCHEMA_VERSION {
            return Ok(LocalStateInitialization {
                path: self.path.as_ref().clone(),
                observed_schema_version,
                schema_version: observed_schema_version,
                migration_ran: false,
                journal_mode,
            });
        }

        let created_at = OffsetDateTime::now_utc().format(&Rfc3339)?;
        let migration_ran = self.apply_migrations(&mut connection, &created_at, MIGRATIONS)?;
        let schema_version = read_user_version(&connection)
            .map_err(|error| self.classify(error, SqlitePhase::Readback))?;
        if schema_version != CURRENT_SCHEMA_VERSION {
            return Err(LocalStateError::PostMigrationReadback {
                path: self.path.as_ref().clone(),
                source: rusqlite::Error::InvalidQuery,
            });
        }

        Ok(LocalStateInitialization {
            path: self.path.as_ref().clone(),
            observed_schema_version,
            schema_version,
            migration_ran,
            journal_mode,
        })
    }

    /// Runs one short synchronous projection operation under `BEGIN IMMEDIATE`.
    ///
    /// This crate-private helper keeps physical connections and transactions
    /// inside local state while applying the established connection policy.
    pub(super) fn with_immediate_transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<T, LocalStateError> {
        let mut connection = self.open_connection()?;
        configure_connection_policy(&connection)
            .map_err(|error| self.classify(error, SqlitePhase::Projection))?;
        // Initialization negotiates WAL once. Projection connections only
        // verify that machine-wide policy instead of changing journal mode or
        // introducing a second contention retry loop.
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(|error| self.classify(error, SqlitePhase::Projection))?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(self.classify(rusqlite::Error::InvalidQuery, SqlitePhase::Projection));
        }

        // Projection never performs Temporal or tracker I/O while this short
        // writer lock is held. BEGIN IMMEDIATE serializes the read, transition,
        // write, and readback without adding a second retry scheduler.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| self.classify(error, SqlitePhase::Projection))?;
        let output = operation(&transaction)
            .map_err(|error| self.classify(error, SqlitePhase::Projection))?;
        transaction
            .commit()
            .map_err(|error| self.classify(error, SqlitePhase::Projection))?;
        Ok(output)
    }

    fn open_connection(&self) -> Result<Connection, LocalStateError> {
        Connection::open(self.path.as_path())
            .map_err(|error| self.classify(error, SqlitePhase::Open))
    }

    fn apply_migrations(
        &self,
        connection: &mut Connection,
        created_at: &str,
        migrations: &[Migration],
    ) -> Result<bool, LocalStateError> {
        let mut migration_ran = false;
        for migration in migrations {
            // BEGIN IMMEDIATE serializes machine-shared migration writers. The
            // version is re-read only after this lock to make retries idempotent.
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|error| self.classify(error, SqlitePhase::Migration(migration.version)))?;
            let version = read_user_version(&transaction)
                .map_err(|error| self.classify(error, SqlitePhase::Migration(migration.version)))?;
            if version >= migration.version {
                transaction.commit().map_err(|error| {
                    self.classify(error, SqlitePhase::Migration(migration.version))
                })?;
                continue;
            }
            if version == 0
                && has_application_schema(&transaction).map_err(|error| {
                    self.classify(error, SqlitePhase::Migration(migration.version))
                })?
            {
                return Err(LocalStateError::UnversionedSchemaConflict {
                    path: self.path.as_ref().clone(),
                });
            }
            if version + 1 != migration.version {
                return Err(LocalStateError::Migration {
                    path: self.path.as_ref().clone(),
                    version: migration.version,
                    source: rusqlite::Error::InvalidQuery,
                });
            }

            (migration.apply)(&transaction, created_at)
                .map_err(|error| self.classify(error, SqlitePhase::Migration(migration.version)))?;
            // user_version is intentionally updated in the same transaction as
            // its schema so rollback can never expose a partially applied v1.
            transaction
                .pragma_update(None, "user_version", migration.version)
                .map_err(|error| self.classify(error, SqlitePhase::Migration(migration.version)))?;
            let readback = read_user_version(&transaction)
                .map_err(|error| self.classify(error, SqlitePhase::Readback))?;
            if readback != migration.version {
                return Err(LocalStateError::PostMigrationReadback {
                    path: self.path.as_ref().clone(),
                    source: rusqlite::Error::InvalidQuery,
                });
            }
            transaction
                .commit()
                .map_err(|error| self.classify(error, SqlitePhase::Migration(migration.version)))?;
            migration_ran = true;
        }
        Ok(migration_ran)
    }

    fn classify(&self, error: rusqlite::Error, phase: SqlitePhase) -> LocalStateError {
        if is_busy(&error) {
            return LocalStateError::DatabaseBusy {
                path: self.path.as_ref().clone(),
            };
        }
        if is_corrupt(&error) {
            return LocalStateError::CorruptDatabase {
                path: self.path.as_ref().clone(),
                source: error,
            };
        }
        match phase {
            SqlitePhase::Open => LocalStateError::Open {
                path: self.path.as_ref().clone(),
                source: error,
            },
            SqlitePhase::Configuration => LocalStateError::Configuration {
                path: self.path.as_ref().clone(),
                source: error,
            },
            SqlitePhase::Migration(version) => LocalStateError::Migration {
                path: self.path.as_ref().clone(),
                version,
                source: error,
            },
            SqlitePhase::Readback => LocalStateError::PostMigrationReadback {
                path: self.path.as_ref().clone(),
                source: error,
            },
            SqlitePhase::Projection => LocalStateError::Projection {
                path: self.path.as_ref().clone(),
                source: error,
            },
        }
    }
}

#[derive(Clone, Copy)]
enum SqlitePhase {
    Open,
    Configuration,
    Migration(u32),
    Readback,
    Projection,
}

fn configure_connection_policy(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    let busy_timeout: u32 =
        connection.pragma_query_value(None, "busy_timeout", |row| row.get(0))?;
    let foreign_keys: u32 =
        connection.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    let synchronous: u32 = connection.pragma_query_value(None, "synchronous", |row| row.get(0))?;
    if busy_timeout != 5_000 || foreign_keys != 1 || synchronous != 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}

fn configure_journal_mode(connection: &Connection) -> rusqlite::Result<JournalMode> {
    // SQLite forbids changing journal mode inside a transaction, so WAL is
    // negotiated and read back before any migration lock is acquired.
    let mode: String =
        connection.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(JournalMode::Wal)
}

fn configure_journal_mode_bounded(connection: &Connection) -> rusqlite::Result<JournalMode> {
    let deadline = std::time::Instant::now() + BUSY_TIMEOUT;
    loop {
        match configure_journal_mode(connection) {
            Ok(mode) => return Ok(mode),
            // SQLite may report an immediate busy while two new processes both
            // switch a database to WAL. Retry only inside the same five-second
            // contention budget used by the connection busy handler.
            Err(error) if is_busy(&error) && std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
fn configure_connection(connection: &Connection) -> rusqlite::Result<JournalMode> {
    configure_connection_policy(connection)?;
    configure_journal_mode(connection)
}

/// Reads SQLite's sole schema-version authority without changing it.
pub(super) fn read_user_version(connection: &Connection) -> rusqlite::Result<u32> {
    // PRAGMA user_version is SQLite's only migration-version authority and has
    // no SeaQuery representation.
    connection.pragma_query_value(None, "user_version", |row| row.get(0))
}

fn inspect_initial_schema(connection: &mut Connection) -> rusqlite::Result<(u32, bool)> {
    // A short read transaction keeps user_version and the conditional v0
    // schema check on one snapshot while another process may commit v1.
    let transaction = connection.transaction()?;
    let version = read_user_version(&transaction)?;
    let conflict = version == 0 && has_application_schema(&transaction)?;
    transaction.commit()?;
    Ok((version, conflict))
}

/// Detects application-owned schema objects while ignoring SQLite internals.
pub(super) fn has_application_schema(connection: &Connection) -> rusqlite::Result<bool> {
    // sqlite_schema introspection has no SeaQuery representation. Internal
    // sqlite_* objects are ignored; any application object makes v0 ambiguous.
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%')",
        [],
        |row| row.get(0),
    )
}

/// Returns whether SQLite reported bounded lock contention.
pub(super) fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(inner.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// Returns whether SQLite rejected the file as corrupt or malformed.
pub(super) fn is_corrupt(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(inner.code, ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc, thread};

    use rusqlite::{params_from_iter, OptionalExtension};
    use sea_query::{Query, SqliteQueryBuilder, Table};
    use sea_query_rusqlite::RusqliteBinder;
    use tempfile::TempDir;

    use super::*;

    fn temporary_database() -> (TempDir, LocalStateDatabase) {
        let temporary = tempfile::tempdir().unwrap();
        let database =
            LocalStateDatabase::at_resolved_path(temporary.path().join("nested/state/symphony.db"))
                .unwrap();
        (temporary, database)
    }

    fn raw_connection(database: &LocalStateDatabase) -> Connection {
        Connection::open(database.path()).unwrap()
    }

    #[test]
    fn default_and_override_paths_are_anchored_outside_the_worktree() {
        let temporary = tempfile::tempdir().unwrap();
        let default = LocalStateDatabase::for_home(temporary.path()).unwrap();
        let relative =
            LocalStateDatabase::with_override(temporary.path(), "custom/local.db").unwrap();
        let absolute_path = temporary.path().join("absolute.db");
        let absolute = LocalStateDatabase::with_override(temporary.path(), &absolute_path).unwrap();

        assert_eq!(
            default.path(),
            temporary.path().join(".shea/state/symphony.db")
        );
        assert_eq!(
            relative.path(),
            temporary.path().join(".shea/custom/local.db")
        );
        assert_eq!(absolute.path(), absolute_path);
        assert!(matches!(
            LocalStateDatabase::at_resolved_path("relative.db"),
            Err(LocalStateError::UnresolvedPath { .. })
        ));
        assert!(matches!(
            LocalStateDatabase::for_home("relative-home"),
            Err(LocalStateError::UnresolvedPath { .. })
        ));
    }

    #[test]
    fn fresh_initialization_migrates_once_and_preserves_metadata() {
        let (_temporary, database) = temporary_database();
        let first = database.initialize().unwrap();
        let connection = raw_connection(&database);
        let created_at: String = connection
            .query_row(
                "SELECT value FROM meta WHERE key = 'created_at'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let updated_at: String = connection
            .query_row(
                "SELECT value FROM meta WHERE key = 'updated_at'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        let second = database.initialize().unwrap();

        assert_eq!(first.observed_schema_version, 0);
        assert_eq!(first.schema_version, 1);
        assert!(first.migration_ran);
        assert_eq!(first.journal_mode, JournalMode::Wal);
        assert_eq!(created_at, updated_at);
        assert!(created_at.contains('T') && created_at.ends_with('Z'));
        assert_eq!(second.observed_schema_version, 1);
        assert!(!second.migration_ran);
        let connection = raw_connection(&database);
        assert_eq!(
            connection
                .query_row(
                    "SELECT value FROM meta WHERE key = 'created_at'",
                    [],
                    |row| { row.get::<_, String>(0) }
                )
                .unwrap(),
            created_at
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM meta WHERE key = 'schema_version'",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn every_connection_policy_is_applied_and_read_back() {
        let (_temporary, database) = temporary_database();
        database.initialize().unwrap();
        let connection = database.open_connection().unwrap();
        assert_eq!(configure_connection(&connection).unwrap(), JournalMode::Wal);

        let busy: u32 = connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        let foreign_keys: u32 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        let synchronous: u32 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 5_000);
        assert_eq!(foreign_keys, 1);
        assert_eq!(synchronous, 1);
        assert_eq!(journal, "wal");
    }

    #[test]
    fn schema_columns_keys_and_indexes_match_v1_contract() {
        let (_temporary, database) = temporary_database();
        database.initialize().unwrap();
        let connection = raw_connection(&database);
        let expected: BTreeMap<&str, Vec<(&str, bool)>> = BTreeMap::from([
            (
                "workflow_index",
                vec![
                    ("workflow_id", true),
                    ("run_id", false),
                    ("workspace_runtime_id", true),
                    ("repo_id", true),
                    ("issue_ref", true),
                    ("from_state", true),
                    ("target_kind", true),
                    ("current_state", true),
                    ("active_step", true),
                    ("waiting_kind", false),
                    ("source_ref", true),
                    ("source_tracker_revision", true),
                    ("started_at", true),
                    ("last_progress_at", false),
                    ("status", true),
                    ("terminal_outcome", false),
                    ("operator_action_ref", false),
                    ("freshness", true),
                    ("updated_at", true),
                ],
            ),
            (
                "artifact_index",
                vec![
                    ("artifact_id", true),
                    ("workflow_id", false),
                    ("workspace_runtime_id", true),
                    ("repo_id", true),
                    ("issue_ref", true),
                    ("kind", true),
                    ("path", true),
                    ("summary", false),
                    ("created_by_step", false),
                    ("created_at", true),
                ],
            ),
            (
                "tracker_cache",
                vec![
                    ("workspace_runtime_id", true),
                    ("repo_id", true),
                    ("issue_ref", true),
                    ("tracker_backend", true),
                    ("tracker_state", true),
                    ("title", true),
                    ("pr_number", false),
                    ("pr_state", false),
                    ("pr_relation_confirmed_at", false),
                    ("updated_at", true),
                    ("freshness", true),
                ],
            ),
            (
                "activity_progress",
                vec![
                    ("workspace_runtime_id", true),
                    ("workflow_id", true),
                    ("activity_id", true),
                    ("activity_kind", true),
                    ("target_ref", true),
                    ("mutation_id", false),
                    ("outcome", false),
                    ("status", true),
                    ("attempt_count", true),
                    ("last_heartbeat_at", false),
                    ("next_retry_at", false),
                    ("summary", false),
                ],
            ),
            ("meta", vec![("key", true), ("value", true)]),
        ]);

        for (table, expected_columns) in expected {
            let mut statement = connection
                .prepare("SELECT name, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid")
                .unwrap();
            let columns: Vec<(String, bool, u32)> = statement
                .query_map([table], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                columns
                    .iter()
                    .map(|(name, not_null, _)| (name.as_str(), *not_null))
                    .collect::<Vec<_>>(),
                expected_columns,
                "column contract for {table}"
            );
        }

        let expected_indexes = [
            (
                "idx_workflow_index_scope_issue_status",
                false,
                vec!["workspace_runtime_id", "repo_id", "issue_ref", "status"],
            ),
            (
                "idx_workflow_index_scope_lane",
                false,
                vec!["workspace_runtime_id", "current_state", "waiting_kind"],
            ),
            (
                "uq_workflow_index_active_issue",
                true,
                vec!["repo_id", "issue_ref"],
            ),
            (
                "idx_artifact_index_scope_workflow",
                false,
                vec!["workspace_runtime_id", "workflow_id"],
            ),
            (
                "idx_artifact_index_scope_issue",
                false,
                vec!["workspace_runtime_id", "repo_id", "issue_ref"],
            ),
            (
                "idx_tracker_cache_scope_freshness",
                false,
                vec!["workspace_runtime_id", "freshness"],
            ),
            (
                "idx_activity_progress_scope_workflow_mutation",
                false,
                vec!["workspace_runtime_id", "workflow_id", "mutation_id"],
            ),
        ];
        for (index, unique, expected_columns) in expected_indexes {
            let actual_unique: bool = connection
                .query_row(
                    "SELECT \"unique\" FROM pragma_index_list((SELECT tbl_name FROM sqlite_schema WHERE name = ?1)) WHERE name = ?1",
                    [index],
                    |row| row.get(0),
                )
                .unwrap();
            let mut statement = connection
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                .unwrap();
            let columns: Vec<String> = statement
                .query_map([index], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(actual_unique, unique, "uniqueness for {index}");
            assert_eq!(columns, expected_columns, "columns for {index}");
        }

        let active_sql: String = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name = 'uq_workflow_index_active_issue'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let normalized = active_sql.to_ascii_lowercase().replace(' ', "");
        assert!(normalized.ends_with("where\"status\"in('starting','running')"));
    }

    fn insert_workflow(
        connection: &Connection,
        workflow_id: &str,
        runtime: &str,
        status: &str,
    ) -> rusqlite::Result<()> {
        let (sql, values) = Query::insert()
            .into_table("workflow_index")
            .columns([
                "workflow_id",
                "workspace_runtime_id",
                "repo_id",
                "issue_ref",
                "from_state",
                "target_kind",
                "current_state",
                "active_step",
                "source_ref",
                "source_tracker_revision",
                "started_at",
                "status",
                "freshness",
                "updated_at",
            ])
            .values_panic([
                workflow_id.into(),
                runtime.into(),
                "github.com/Alive24/repo".into(),
                "github_project_v2|github.com/Alive24/repo|479".into(),
                "Todo".into(),
                "main".into(),
                "In Progress".into(),
                "implement".into(),
                "issue:479".into(),
                "revision-1".into(),
                "2026-07-10T19:04:00Z".into(),
                status.into(),
                "fresh".into(),
                "2026-07-10T19:04:00Z".into(),
            ])
            .build_rusqlite(SqliteQueryBuilder);
        connection.execute(&sql, params_from_iter(values.as_params()))?;
        Ok(())
    }

    #[test]
    fn active_guard_is_machine_wide_while_completed_rows_do_not_block() {
        let (_temporary, database) = temporary_database();
        database.initialize().unwrap();
        let connection = raw_connection(&database);
        insert_workflow(&connection, "workflow-1", "runtime-a", "running").unwrap();
        let conflict =
            insert_workflow(&connection, "workflow-2", "runtime-b", "starting").unwrap_err();
        assert!(matches!(
            conflict,
            rusqlite::Error::SqliteFailure(inner, _) if inner.code == ErrorCode::ConstraintViolation
        ));
        connection
            .execute(
                "UPDATE workflow_index SET status = 'completed' WHERE workflow_id = 'workflow-1'",
                [],
            )
            .unwrap();
        insert_workflow(&connection, "workflow-2", "runtime-b", "starting").unwrap();
    }

    #[test]
    fn tracker_cache_primary_key_is_workspace_scoped() {
        let (_temporary, database) = temporary_database();
        database.initialize().unwrap();
        let connection = raw_connection(&database);
        for runtime in ["runtime-a", "runtime-b"] {
            let (sql, values) = Query::insert()
                .into_table("tracker_cache")
                .columns([
                    "workspace_runtime_id",
                    "repo_id",
                    "issue_ref",
                    "tracker_backend",
                    "tracker_state",
                    "title",
                    "updated_at",
                    "freshness",
                ])
                .values_panic([
                    runtime.into(),
                    "github.com/Alive24/repo".into(),
                    "github_project_v2|github.com/Alive24/repo|479".into(),
                    "github_project_v2".into(),
                    "Todo".into(),
                    "Issue".into(),
                    "2026-07-10T19:04:00Z".into(),
                    "fresh".into(),
                ])
                .build_rusqlite(SqliteQueryBuilder);
            connection
                .execute(&sql, params_from_iter(values.as_params()))
                .unwrap();
        }
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM tracker_cache", [], |row| row
                    .get::<_, u32>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn concurrent_initializers_apply_v1_once() {
        let (_temporary, database) = temporary_database();
        let database = Arc::new(database);
        let first = {
            let database = Arc::clone(&database);
            thread::spawn(move || database.initialize().unwrap())
        };
        let second = {
            let database = Arc::clone(&database);
            thread::spawn(move || database.initialize().unwrap())
        };
        let results = [first.join().unwrap(), second.join().unwrap()];

        assert_eq!(
            results.iter().filter(|result| result.migration_ran).count(),
            1
        );
        assert!(results.iter().all(|result| result.schema_version == 1));
        let connection = raw_connection(&database);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM meta WHERE key = 'created_at'",
                    [],
                    |row| { row.get::<_, u32>(0) }
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn future_and_unversioned_schemas_are_not_mutated() {
        let (_temporary, future_database) = temporary_database();
        std::fs::create_dir_all(future_database.path().parent().unwrap()).unwrap();
        let future = raw_connection(&future_database);
        future.pragma_update(None, "user_version", 2).unwrap();
        let original_journal: String = future
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        drop(future);
        assert!(matches!(
            future_database.initialize(),
            Err(LocalStateError::UnsupportedSchemaVersion { observed: 2, .. })
        ));
        assert_eq!(
            read_user_version(&raw_connection(&future_database)).unwrap(),
            2
        );
        assert_eq!(
            raw_connection(&future_database)
                .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
                .unwrap(),
            original_journal
        );

        let (_temporary, conflict_database) = temporary_database();
        std::fs::create_dir_all(conflict_database.path().parent().unwrap()).unwrap();
        let conflict = raw_connection(&conflict_database);
        conflict
            .execute(
                &Table::create()
                    .table("foreign_schema")
                    .col(sea_query::ColumnDef::new("id").integer())
                    .to_string(SqliteQueryBuilder),
                [],
            )
            .unwrap();
        drop(conflict);
        assert!(matches!(
            conflict_database.initialize(),
            Err(LocalStateError::UnversionedSchemaConflict { .. })
        ));
        let conflict = raw_connection(&conflict_database);
        assert_eq!(read_user_version(&conflict).unwrap(), 0);
        assert!(conflict
            .query_row(
                "SELECT 1 FROM sqlite_schema WHERE name = 'workflow_index'",
                [],
                |row| row.get::<_, u32>(0)
            )
            .optional()
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_database_is_reported_without_replacement() {
        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let bytes = b"not a sqlite database";
        std::fs::write(database.path(), bytes).unwrap();

        assert!(matches!(
            database.initialize(),
            Err(LocalStateError::CorruptDatabase { .. })
        ));
        assert_eq!(std::fs::read(database.path()).unwrap(), bytes);
    }

    #[test]
    fn write_lock_timeout_is_typed_and_bounded() {
        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let mut locking_connection = database.open_connection().unwrap();
        configure_connection(&locking_connection).unwrap();
        let transaction = locking_connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();

        let started = std::time::Instant::now();
        assert!(matches!(
            database.initialize(),
            Err(LocalStateError::DatabaseBusy { .. })
        ));
        assert!(started.elapsed() >= Duration::from_secs(4));
        assert!(started.elapsed() < Duration::from_secs(7));
        drop(transaction);
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_version_together() {
        fn fail_after_schema_change(
            transaction: &rusqlite::Transaction<'_>,
            _created_at: &str,
        ) -> rusqlite::Result<()> {
            transaction.execute_batch(
                "CREATE TABLE should_roll_back (id INTEGER); SELECT * FROM missing_table;",
            )
        }

        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let mut connection = database.open_connection().unwrap();
        configure_connection(&connection).unwrap();
        let migrations = [Migration {
            version: 1,
            apply: fail_after_schema_change,
        }];
        assert!(matches!(
            database.apply_migrations(&mut connection, "2026-07-10T19:04:00Z", &migrations),
            Err(LocalStateError::Migration { version: 1, .. })
        ));
        assert_eq!(read_user_version(&connection).unwrap(), 0);
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'should_roll_back')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!exists);
    }
}
