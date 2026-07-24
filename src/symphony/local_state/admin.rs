//! Read-only local-state diagnosis and explicit migration administration.
//!
//! Health inspection intentionally uses SQLite's read-only open mode and only
//! reads version/schema evidence. Mutation remains an explicit caller action
//! delegated to [`LocalStateDatabase::initialize`].

// T2607-07 is the first runtime consumer of this crate-internal library seam.
// Keep its visibility narrow without treating the pending integration as an
// error in the standalone library build.
#![allow(dead_code)]

use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, ErrorCode, OpenFlags};

use super::{
    database::{
        has_application_schema, is_busy, is_corrupt, read_user_version, CURRENT_SCHEMA_VERSION,
    },
    LocalStateDatabase, LocalStateError, LocalStateInitialization,
};

const REQUIRED_V1_TABLES: &[&str] = &[
    "workflow_index",
    "artifact_index",
    "tracker_cache",
    "activity_progress",
    "meta",
];
const REQUIRED_V1_METADATA_KEYS: &[&str] = &["created_at", "updated_at"];

/// Internal boundary for local-state health inspection and explicit migration.
///
/// This is intentionally crate-visible: a later Tauri command can use the
/// typed boundary without exposing SQLite connections or SQL outside the
/// Symphony runtime.
#[derive(Debug, Clone)]
pub(crate) struct LocalStateAdmin {
    database: LocalStateDatabase,
}

impl LocalStateAdmin {
    /// Builds an administrative boundary over one resolved local-state path.
    pub(crate) fn new(database: LocalStateDatabase) -> Self {
        Self { database }
    }

    /// Inspects local-state readiness without creating or changing anything.
    ///
    /// The returned diagnostic tells a caller whether an explicit migration,
    /// operator inspection, or later recovery path is appropriate. It never
    /// uses the initialization connection policy as a probe.
    pub(crate) fn check_health(&self) -> LocalStateHealth {
        let path = self.database.path();
        if let Some(diagnostic) = inspect_path(path) {
            return LocalStateHealth::NotReady(diagnostic);
        }

        // SQLite's ordinary open can create a missing file. Health instead
        // uses read-only mode and performs only snapshot reads below; it does
        // not set connection PRAGMAs, negotiate journal mode, or migrate.
        let mut connection =
            match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                Ok(connection) => connection,
                Err(error) => {
                    return LocalStateHealth::NotReady(classify_inspection_error(
                        path,
                        LocalStateInspectionPhase::Open,
                        error,
                    ));
                }
            };
        let transaction = match connection.transaction() {
            Ok(transaction) => transaction,
            Err(error) => {
                return LocalStateHealth::NotReady(classify_inspection_error(
                    path,
                    LocalStateInspectionPhase::ReadOnlyInspection,
                    error,
                ));
            }
        };
        let health = inspect_schema(&transaction, path);
        if let Err(error) = transaction.commit() {
            return LocalStateHealth::NotReady(classify_inspection_error(
                path,
                LocalStateInspectionPhase::ReadOnlyInspection,
                error,
            ));
        }
        health
    }

    /// Explicitly creates or forward-migrates the configured local-state DB.
    ///
    /// This is the only admin mutation entry point. The completed database
    /// lifecycle remains the version and transaction authority, so callers
    /// retain its future-version, conflict, corruption, contention, and
    /// idempotency semantics unchanged.
    pub(crate) fn migrate(&self) -> Result<LocalStateInitialization, LocalStateError> {
        self.database.initialize()
    }
}

/// Read-only assessment of the configured local-state database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalStateHealth {
    /// The current schema and its minimum v1 readiness evidence are readable.
    Current(LocalStateReadiness),
    /// A typed condition prevents the database from being used as current.
    NotReady(LocalStateHealthDiagnostic),
}

/// Readiness details for a verified current local-state database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalStateReadiness {
    /// Resolved local-state database path that was inspected.
    pub path: PathBuf,
    /// Schema version read from SQLite's `user_version` authority.
    pub schema_version: u32,
}

/// Typed diagnostic returned when local state is not currently usable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LocalStateHealthDiagnostic {
    /// No database exists at the configured path; explicit migration may create it.
    MissingPath {
        /// Resolved path that was absent.
        path: PathBuf,
    },
    /// Filesystem inspection found a path that cannot be safely opened as a DB.
    UnavailablePath {
        /// Resolved path that could not be used for inspection.
        path: PathBuf,
        /// Typed reason no read-only SQLite connection was attempted or possible.
        reason: LocalStatePathIssue,
    },
    /// A supported older schema needs an explicit forward migration.
    MigrationRequired {
        /// Resolved database path.
        path: PathBuf,
        /// Version read from SQLite's `user_version` authority.
        observed_schema_version: u32,
        /// Latest version supported by this binary.
        supported_schema_version: u32,
    },
    /// A newer schema must not be changed by this binary.
    UnsupportedSchemaVersion {
        /// Resolved database path.
        path: PathBuf,
        /// Version read from SQLite's `user_version` authority.
        observed_schema_version: u32,
        /// Latest version supported by this binary.
        supported_schema_version: u32,
    },
    /// Version zero contains application objects and cannot be adopted safely.
    UnversionedSchemaConflict {
        /// Resolved database path.
        path: PathBuf,
    },
    /// `user_version` is current but minimum v1 readiness evidence is missing.
    IncompleteCurrentSchema {
        /// Resolved database path.
        path: PathBuf,
        /// Current version whose required evidence was incomplete.
        schema_version: u32,
        /// Required v1 table names absent from `sqlite_schema`.
        missing_tables: Vec<String>,
        /// Required v1 metadata keys absent from the `meta` table.
        missing_metadata_keys: Vec<String>,
    },
    /// SQLite rejected the configured file as corrupt or malformed.
    CorruptDatabase {
        /// Resolved database path.
        path: PathBuf,
    },
    /// A read-only open or inspection could not finish safely.
    InspectionFailure {
        /// Resolved database path.
        path: PathBuf,
        /// Operation that failed without changing database state.
        phase: LocalStateInspectionPhase,
        /// Classified SQLite failure; raw SQLite errors remain private.
        failure: LocalStateInspectionFailure,
    },
}

/// Filesystem condition that made a configured local-state path unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalStatePathIssue {
    /// The configured path exists but is not a regular file.
    NotARegularFile,
    /// Filesystem metadata could not be read because access was denied.
    PermissionDenied,
    /// Filesystem metadata could not be read for another safe-to-report reason.
    Unavailable,
}

/// Read-only inspection operation that encountered a SQLite failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalStateInspectionPhase {
    /// Opening the existing database with SQLite read-only flags.
    Open,
    /// Reading schema version or v1 readiness evidence in a deferred transaction.
    ReadOnlyInspection,
}

/// Classified SQLite failure from a read-only health inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalStateInspectionFailure {
    /// Another connection held a lock that prevented the read-only inspection.
    Busy,
    /// SQLite could not open the configured existing file.
    CannotOpen,
    /// SQLite denied the requested read-only access.
    ReadOnly,
    /// SQLite returned a safe but otherwise unclassified inspection failure.
    Other,
}

fn inspect_path(path: &Path) -> Option<LocalStateHealthDiagnostic> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => None,
        Ok(_) => Some(LocalStateHealthDiagnostic::UnavailablePath {
            path: path.to_path_buf(),
            reason: LocalStatePathIssue::NotARegularFile,
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Some(LocalStateHealthDiagnostic::MissingPath {
                path: path.to_path_buf(),
            })
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {
            Some(LocalStateHealthDiagnostic::UnavailablePath {
                path: path.to_path_buf(),
                reason: LocalStatePathIssue::PermissionDenied,
            })
        }
        Err(_) => Some(LocalStateHealthDiagnostic::UnavailablePath {
            path: path.to_path_buf(),
            reason: LocalStatePathIssue::Unavailable,
        }),
    }
}

fn inspect_schema(connection: &Connection, path: &Path) -> LocalStateHealth {
    let schema_version = match read_user_version(connection) {
        Ok(schema_version) => schema_version,
        Err(error) => {
            return LocalStateHealth::NotReady(classify_inspection_error(
                path,
                LocalStateInspectionPhase::ReadOnlyInspection,
                error,
            ));
        }
    };
    if schema_version > CURRENT_SCHEMA_VERSION {
        return LocalStateHealth::NotReady(LocalStateHealthDiagnostic::UnsupportedSchemaVersion {
            path: path.to_path_buf(),
            observed_schema_version: schema_version,
            supported_schema_version: CURRENT_SCHEMA_VERSION,
        });
    }
    if schema_version == 0 {
        return match has_application_schema(connection) {
            Ok(true) => {
                LocalStateHealth::NotReady(LocalStateHealthDiagnostic::UnversionedSchemaConflict {
                    path: path.to_path_buf(),
                })
            }
            Ok(false) => {
                LocalStateHealth::NotReady(LocalStateHealthDiagnostic::MigrationRequired {
                    path: path.to_path_buf(),
                    observed_schema_version: schema_version,
                    supported_schema_version: CURRENT_SCHEMA_VERSION,
                })
            }
            Err(error) => LocalStateHealth::NotReady(classify_inspection_error(
                path,
                LocalStateInspectionPhase::ReadOnlyInspection,
                error,
            )),
        };
    }
    if schema_version < CURRENT_SCHEMA_VERSION {
        return LocalStateHealth::NotReady(LocalStateHealthDiagnostic::MigrationRequired {
            path: path.to_path_buf(),
            observed_schema_version: schema_version,
            supported_schema_version: CURRENT_SCHEMA_VERSION,
        });
    }

    match missing_v1_evidence(connection) {
        Ok((missing_tables, missing_metadata_keys))
            if missing_tables.is_empty() && missing_metadata_keys.is_empty() =>
        {
            LocalStateHealth::Current(LocalStateReadiness {
                path: path.to_path_buf(),
                schema_version,
            })
        }
        Ok((missing_tables, missing_metadata_keys)) => {
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::IncompleteCurrentSchema {
                path: path.to_path_buf(),
                schema_version,
                missing_tables,
                missing_metadata_keys,
            })
        }
        Err(error) => LocalStateHealth::NotReady(classify_inspection_error(
            path,
            LocalStateInspectionPhase::ReadOnlyInspection,
            error,
        )),
    }
}

fn missing_v1_evidence(connection: &Connection) -> rusqlite::Result<(Vec<String>, Vec<String>)> {
    // This fixed schema inspection is intentionally smaller than a full audit:
    // current v1 needs only its table set and migration metadata to be useful.
    let mut table_statement = connection.prepare(
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name IN \
         ('workflow_index', 'artifact_index', 'tracker_cache', 'activity_progress', 'meta')",
    )?;
    let present_tables = table_statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let missing_tables = missing_entries(REQUIRED_V1_TABLES, &present_tables);
    if missing_tables.iter().any(|name| name == "meta") {
        return Ok((
            missing_tables,
            REQUIRED_V1_METADATA_KEYS
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        ));
    }

    let mut metadata_statement =
        connection.prepare("SELECT key FROM meta WHERE key IN ('created_at', 'updated_at')")?;
    let present_metadata = metadata_statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((
        missing_tables,
        missing_entries(REQUIRED_V1_METADATA_KEYS, &present_metadata),
    ))
}

fn missing_entries(required: &[&str], present: &[String]) -> Vec<String> {
    required
        .iter()
        .filter(|name| !present.iter().any(|present_name| present_name == **name))
        .map(|name| (*name).to_owned())
        .collect()
}

fn classify_inspection_error(
    path: &Path,
    phase: LocalStateInspectionPhase,
    error: rusqlite::Error,
) -> LocalStateHealthDiagnostic {
    if is_corrupt(&error) {
        return LocalStateHealthDiagnostic::CorruptDatabase {
            path: path.to_path_buf(),
        };
    }
    let failure = if is_busy(&error) {
        LocalStateInspectionFailure::Busy
    } else {
        match error {
            rusqlite::Error::SqliteFailure(inner, _) if inner.code == ErrorCode::CannotOpen => {
                LocalStateInspectionFailure::CannotOpen
            }
            rusqlite::Error::SqliteFailure(inner, _) if inner.code == ErrorCode::ReadOnly => {
                LocalStateInspectionFailure::ReadOnly
            }
            _ => LocalStateInspectionFailure::Other,
        }
    };
    LocalStateHealthDiagnostic::InspectionFailure {
        path: path.to_path_buf(),
        phase,
        failure,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::*;

    fn temporary_database() -> (TempDir, LocalStateDatabase) {
        let temporary = tempfile::tempdir().unwrap();
        let database =
            LocalStateDatabase::at_resolved_path(temporary.path().join("nested/state/symphony.db"))
                .unwrap();
        (temporary, database)
    }

    fn schema_version(database: &LocalStateDatabase) -> u32 {
        Connection::open(database.path())
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    fn migration_metadata(database: &LocalStateDatabase) -> BTreeMap<String, String> {
        let connection = Connection::open(database.path()).unwrap();
        let mut statement = connection
            .prepare("SELECT key, value FROM meta ORDER BY key")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn directory_entries(database: &LocalStateDatabase) -> Vec<std::path::PathBuf> {
        let mut entries = std::fs::read_dir(database.path().parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        entries
    }

    #[test]
    fn missing_path_health_is_typed_and_creates_nothing() {
        let (_temporary, database) = temporary_database();
        let admin = LocalStateAdmin::new(database.clone());
        let parent = database.path().parent().unwrap();

        assert!(!parent.exists());
        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::MissingPath {
                path: database.path().to_path_buf(),
            })
        );
        assert!(!database.path().exists());
        assert!(!parent.exists());
    }

    #[test]
    fn current_v1_health_verifies_readiness_without_changing_metadata() {
        let (_temporary, database) = temporary_database();
        let admin = LocalStateAdmin::new(database.clone());
        admin.migrate().unwrap();
        let before_version = schema_version(&database);
        let before_metadata = migration_metadata(&database);
        let before_entries = directory_entries(&database);

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::Current(LocalStateReadiness {
                path: database.path().to_path_buf(),
                schema_version: 1,
            })
        );
        assert!(database.path().exists());
        assert_eq!(schema_version(&database), before_version);
        assert_eq!(migration_metadata(&database), before_metadata);
        assert_eq!(directory_entries(&database), before_entries);
    }

    #[test]
    fn empty_existing_database_requires_explicit_migration() {
        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let connection = Connection::open(database.path()).unwrap();
        let initial_version: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        drop(connection);
        let admin = LocalStateAdmin::new(database.clone());

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::MigrationRequired {
                path: database.path().to_path_buf(),
                observed_schema_version: 0,
                supported_schema_version: 1,
            })
        );
        assert_eq!(initial_version, 0);
        assert_eq!(schema_version(&database), 0);
    }

    #[test]
    fn current_version_without_required_v1_evidence_is_not_healthy() {
        let (_temporary, database) = temporary_database();
        let admin = LocalStateAdmin::new(database.clone());
        admin.migrate().unwrap();
        Connection::open(database.path())
            .unwrap()
            .execute("DROP TABLE artifact_index", [])
            .unwrap();

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::IncompleteCurrentSchema {
                path: database.path().to_path_buf(),
                schema_version: 1,
                missing_tables: vec!["artifact_index".into()],
                missing_metadata_keys: vec![],
            })
        );
    }

    #[test]
    fn malformed_database_health_is_typed_and_never_replaces_the_file() {
        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let bytes = b"not a sqlite database";
        std::fs::write(database.path(), bytes).unwrap();
        let admin = LocalStateAdmin::new(database.clone());

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::CorruptDatabase {
                path: database.path().to_path_buf(),
            })
        );
        assert_eq!(std::fs::read(database.path()).unwrap(), bytes);
    }

    #[test]
    fn future_schema_health_is_typed_and_non_mutating() {
        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let connection = Connection::open(database.path()).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        drop(connection);
        let admin = LocalStateAdmin::new(database.clone());

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::UnsupportedSchemaVersion {
                path: database.path().to_path_buf(),
                observed_schema_version: 2,
                supported_schema_version: 1,
            })
        );
        assert_eq!(schema_version(&database), 2);
    }

    #[test]
    fn unversioned_application_schema_health_is_typed_and_non_mutating() {
        let (_temporary, database) = temporary_database();
        std::fs::create_dir_all(database.path().parent().unwrap()).unwrap();
        let connection = Connection::open(database.path()).unwrap();
        connection
            .execute("CREATE TABLE foreign_schema (id INTEGER)", [])
            .unwrap();
        drop(connection);
        let admin = LocalStateAdmin::new(database.clone());

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::UnversionedSchemaConflict {
                path: database.path().to_path_buf(),
            })
        );
        let connection = Connection::open(database.path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'workflow_index'",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn explicit_migration_creates_once_and_reentry_is_a_no_op() {
        let (_temporary, database) = temporary_database();
        let admin = LocalStateAdmin::new(database.clone());

        let first = admin.migrate().unwrap();
        let second = admin.migrate().unwrap();

        assert_eq!(first.path, database.path());
        assert_eq!(first.observed_schema_version, 0);
        assert_eq!(first.schema_version, 1);
        assert!(first.migration_ran);
        assert_eq!(second.observed_schema_version, 1);
        assert_eq!(second.schema_version, 1);
        assert!(!second.migration_ran);
        assert!(matches!(admin.check_health(), LocalStateHealth::Current(_)));
    }

    #[test]
    fn explicit_migration_preserves_nonrecoverable_inputs() {
        let (_temporary, malformed) = temporary_database();
        std::fs::create_dir_all(malformed.path().parent().unwrap()).unwrap();
        let malformed_bytes = b"not a sqlite database";
        std::fs::write(malformed.path(), malformed_bytes).unwrap();
        let malformed_admin = LocalStateAdmin::new(malformed.clone());
        assert!(matches!(
            malformed_admin.migrate(),
            Err(LocalStateError::CorruptDatabase { .. })
        ));
        assert_eq!(std::fs::read(malformed.path()).unwrap(), malformed_bytes);

        let (_temporary, future) = temporary_database();
        std::fs::create_dir_all(future.path().parent().unwrap()).unwrap();
        let connection = Connection::open(future.path()).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        drop(connection);
        let future_admin = LocalStateAdmin::new(future.clone());
        assert!(matches!(
            future_admin.migrate(),
            Err(LocalStateError::UnsupportedSchemaVersion { observed: 2, .. })
        ));
        assert_eq!(schema_version(&future), 2);

        let (_temporary, conflict) = temporary_database();
        std::fs::create_dir_all(conflict.path().parent().unwrap()).unwrap();
        let connection = Connection::open(conflict.path()).unwrap();
        connection
            .execute("CREATE TABLE foreign_schema (id INTEGER)", [])
            .unwrap();
        drop(connection);
        let conflict_admin = LocalStateAdmin::new(conflict.clone());
        assert!(matches!(
            conflict_admin.migrate(),
            Err(LocalStateError::UnversionedSchemaConflict { .. })
        ));
        let connection = Connection::open(conflict.path()).unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'workflow_index'",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn non_file_path_is_a_typed_unavailable_diagnostic() {
        let temporary = tempfile::tempdir().unwrap();
        let database = LocalStateDatabase::at_resolved_path(temporary.path()).unwrap();
        let admin = LocalStateAdmin::new(database.clone());

        assert_eq!(
            admin.check_health(),
            LocalStateHealth::NotReady(LocalStateHealthDiagnostic::UnavailablePath {
                path: database.path().to_path_buf(),
                reason: LocalStatePathIssue::NotARegularFile,
            })
        );
    }
}
