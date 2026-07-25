//! Read-only Temporal readiness for the currently selected workspace.
//!
//! This module owns only the Tauri adaptation boundary: workflow parsing,
//! runtime validation, and Temporal connectivity remain in the shared
//! `shea-symphony` library. The response contains the captured workspace
//! identity so callers can discard a stale result after switching targets.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

use std::{future::Future, time::Duration};

use serde::Serialize;
use shea_symphony::{
    symphony::{SymphonyTemporalClient, TemporalRuntimeError},
    RuntimeConfig, WorkflowDefinition,
};
use tauri::State;

use crate::workspace::WorkspaceManager;

const TEMPORAL_READINESS_TIMEOUT: Duration = Duration::from_secs(5);

/// Bounded Temporal readiness states returned to the App.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TemporalRuntimeHealthStatus {
    /// The configured Temporal service accepted a connection.
    Ready,
    /// The configured Temporal service could not be reached.
    Unavailable,
    /// The readiness probe exceeded its bounded wait.
    TimedOut,
    /// The selected workflow or its runtime configuration is invalid.
    InvalidConfig,
}

/// Operator-safe context for an expected readiness outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TemporalRuntimeDiagnostic {
    code: &'static str,
    summary: &'static str,
}

/// Temporal readiness observed for one captured workspace selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TemporalRuntimeHealth {
    status: TemporalRuntimeHealthStatus,
    workspace_path: String,
    workflow_path: String,
    diagnostic: Option<TemporalRuntimeDiagnostic>,
}

/// Checks Temporal readiness using the active workspace and shared client.
///
/// The command accepts no caller-supplied target, workflow, or Temporal
/// parameters. Expected configuration, connection, and timeout outcomes are
/// returned as [`TemporalRuntimeHealth`]; `Err` is reserved for an unexpected
/// shared-client failure.
#[tauri::command]
pub(crate) async fn get_temporal_runtime_health(
    manager: State<'_, WorkspaceManager>,
) -> Result<TemporalRuntimeHealth, String> {
    temporal_runtime_health_with_probe(
        &manager,
        TEMPORAL_READINESS_TIMEOUT,
        |temporal_config| async move {
            SymphonyTemporalClient::new(temporal_config)
                .check_service()
                .await
        },
    )
    .await
}

async fn temporal_runtime_health_with_probe<F, Fut>(
    manager: &WorkspaceManager,
    timeout_duration: Duration,
    probe: F,
) -> Result<TemporalRuntimeHealth, String>
where
    F: FnOnce(shea_symphony::config::TemporalConfig) -> Fut,
    Fut: Future<Output = Result<(), TemporalRuntimeError>>,
{
    // Capture active-workspace authority once before file or network I/O so an
    // in-flight workspace switch cannot silently retarget this invocation.
    let workspace = manager.current();
    let workspace_path = workspace.target_path();
    let workflow_path = workspace.workflow_file_path();

    let invalid_config = |code, summary| TemporalRuntimeHealth {
        status: TemporalRuntimeHealthStatus::InvalidConfig,
        workspace_path: workspace_path.display().to_string(),
        workflow_path: workflow_path.display().to_string(),
        diagnostic: Some(TemporalRuntimeDiagnostic { code, summary }),
    };

    let workflow = match WorkflowDefinition::load(&workflow_path) {
        Ok(workflow) => workflow,
        Err(_) => {
            return Ok(invalid_config(
                "workflowDefinitionInvalid",
                "The selected workflow definition could not be loaded.",
            ));
        }
    };
    let runtime_config =
        match RuntimeConfig::from_workflow(&workflow, &workflow_path).and_then(|config| {
            config.validate()?;
            Ok(config)
        }) {
            Ok(config) => config,
            Err(_) => {
                return Ok(invalid_config(
                    "runtimeConfigInvalid",
                    "The selected workflow runtime configuration is invalid.",
                ));
            }
        };

    // Five seconds bounds production waits; the injected probe and duration
    // make timeout coverage deterministic without a live Temporal service.
    let probe_result = tokio::time::timeout(timeout_duration, probe(runtime_config.temporal)).await;

    // Connection/configuration failures and timeout are health states. Other
    // client variants cannot arise from check_service and remain internal errors.
    let (status, diagnostic) = match probe_result {
        Ok(Ok(())) => (TemporalRuntimeHealthStatus::Ready, None),
        Ok(Err(TemporalRuntimeError::Unavailable { .. })) => (
            TemporalRuntimeHealthStatus::Unavailable,
            Some(TemporalRuntimeDiagnostic {
                code: "temporalServiceUnavailable",
                summary: "The configured Temporal service is unavailable.",
            }),
        ),
        Ok(Err(TemporalRuntimeError::InvalidConfig(_))) => (
            TemporalRuntimeHealthStatus::InvalidConfig,
            Some(TemporalRuntimeDiagnostic {
                code: "temporalConfigInvalid",
                summary: "The configured Temporal endpoint is invalid.",
            }),
        ),
        Err(_) => (
            TemporalRuntimeHealthStatus::TimedOut,
            Some(TemporalRuntimeDiagnostic {
                code: "temporalReadinessTimedOut",
                summary: "The Temporal readiness check timed out.",
            }),
        ),
        Ok(Err(_)) => return Err("unexpected Temporal readiness probe failure".into()),
    };

    // Preserve the captured identity instead of re-reading current workspace;
    // a future UI can compare these fields and reject a stale response.
    Ok(TemporalRuntimeHealth {
        status,
        workspace_path: workspace_path.display().to_string(),
        workflow_path: workflow_path.display().to_string(),
        diagnostic,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, future, path::Path};

    use serde_json::json;
    use shea_symphony::config::TemporalConfig;
    use tempfile::TempDir;

    use super::*;
    use crate::workspace::WorkspaceProfile;

    const WORKFLOW_RELATIVE_PATH: &str = ".shea/workflows/shea-symphony.md";

    fn write_workflow(root: &Path, namespace: &str, temporal_address: &str) {
        let workflow_path = root.join(WORKFLOW_RELATIVE_PATH);
        fs::create_dir_all(workflow_path.parent().unwrap()).unwrap();
        fs::write(
            workflow_path,
            format!(
                r#"---
tracker:
  kind: memory
temporal:
  address: "{temporal_address}"
  namespace: "{namespace}"
main_lane:
  backend: dry-run
review_lane:
  backend: fake
---
Test workflow"#
            ),
        )
        .unwrap();
    }

    fn manager(engine: &TempDir, profile: WorkspaceProfile) -> WorkspaceManager {
        WorkspaceManager::new(
            engine.path().to_path_buf(),
            profile,
            engine.path().join("profile.json"),
        )
    }

    async fn ready_probe(_: TemporalConfig) -> Result<(), TemporalRuntimeError> {
        Ok(())
    }

    #[tokio::test]
    async fn self_workspace_selects_its_workflow_and_serializes_ready() {
        let engine = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "self-namespace", "127.0.0.1:7233");
        let manager = manager(
            &engine,
            WorkspaceProfile::self_targeted(engine.path().to_path_buf()),
        );

        let result = temporal_runtime_health_with_probe(
            &manager,
            Duration::from_secs(1),
            |config| async move {
                assert_eq!(config.namespace, "self-namespace");
                ready_probe(config).await
            },
        )
        .await
        .unwrap();

        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "status": "ready",
                "workspacePath": engine.path().canonicalize().unwrap(),
                "workflowPath": engine.path().canonicalize().unwrap().join(WORKFLOW_RELATIVE_PATH),
                "diagnostic": null,
            })
        );
    }

    #[tokio::test]
    async fn external_target_selects_its_workflow() {
        let engine = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "engine-namespace", "127.0.0.1:7233");
        write_workflow(target.path(), "target-namespace", "127.0.0.1:7233");
        let profile = WorkspaceProfile {
            engine_root: engine.path().canonicalize().unwrap().display().to_string(),
            target_root: target.path().canonicalize().unwrap().display().to_string(),
            workflow_path: WORKFLOW_RELATIVE_PATH.into(),
            cli_path: None,
            source: "test".into(),
            error: None,
        };
        let manager = manager(&engine, profile);

        let result = temporal_runtime_health_with_probe(
            &manager,
            Duration::from_secs(1),
            |config| async move {
                assert_eq!(config.namespace, "target-namespace");
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(result.status, TemporalRuntimeHealthStatus::Ready);
        assert_eq!(
            result.workspace_path,
            target.path().canonicalize().unwrap().display().to_string()
        );
        assert_eq!(
            result.workflow_path,
            target
                .path()
                .canonicalize()
                .unwrap()
                .join(WORKFLOW_RELATIVE_PATH)
                .display()
                .to_string()
        );
    }

    #[tokio::test]
    async fn invalid_workflow_config_is_a_typed_result() {
        let engine = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "default", " ");
        let manager = manager(
            &engine,
            WorkspaceProfile::self_targeted(engine.path().to_path_buf()),
        );

        let result =
            temporal_runtime_health_with_probe(&manager, Duration::from_secs(1), |_| async {
                panic!("invalid configuration must not reach the Temporal probe");
                #[allow(unreachable_code)]
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(result.status, TemporalRuntimeHealthStatus::InvalidConfig);
        assert_eq!(result.diagnostic.unwrap().code, "runtimeConfigInvalid");
    }

    #[tokio::test]
    async fn unavailable_service_is_bounded_and_operator_safe() {
        let engine = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "default", "127.0.0.1:7233");
        let manager = manager(
            &engine,
            WorkspaceProfile::self_targeted(engine.path().to_path_buf()),
        );

        let result =
            temporal_runtime_health_with_probe(&manager, Duration::from_secs(1), |_| async {
                Err(TemporalRuntimeError::Unavailable {
                    address: "secret-host:7233".into(),
                    namespace: "secret-namespace".into(),
                    source_error: "unbounded sdk details".into(),
                })
            })
            .await
            .unwrap();
        let serialized = serde_json::to_string(&result).unwrap();

        assert_eq!(result.status, TemporalRuntimeHealthStatus::Unavailable);
        assert!(serialized.contains("temporalServiceUnavailable"));
        assert!(!serialized.contains("secret-host"));
        assert!(!serialized.contains("unbounded sdk details"));
    }

    #[tokio::test]
    async fn unexpected_probe_failure_uses_a_bounded_tauri_error() {
        let engine = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "default", "127.0.0.1:7233");
        let manager = manager(
            &engine,
            WorkspaceProfile::self_targeted(engine.path().to_path_buf()),
        );

        let error =
            temporal_runtime_health_with_probe(&manager, Duration::from_secs(1), |_| async {
                Err(TemporalRuntimeError::RuntimeInitialization(
                    "unbounded internal detail".into(),
                ))
            })
            .await
            .unwrap_err();

        assert_eq!(error, "unexpected Temporal readiness probe failure");
        assert!(!error.contains("unbounded internal detail"));
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_is_deterministic_without_a_live_service() {
        let engine = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "default", "127.0.0.1:7233");
        let manager = manager(
            &engine,
            WorkspaceProfile::self_targeted(engine.path().to_path_buf()),
        );

        let result = temporal_runtime_health_with_probe(&manager, Duration::from_secs(5), |_| {
            future::pending()
        })
        .await
        .unwrap();

        assert_eq!(result.status, TemporalRuntimeHealthStatus::TimedOut);
        assert_eq!(result.diagnostic.unwrap().code, "temporalReadinessTimedOut");
    }

    #[tokio::test]
    async fn response_preserves_snapshot_identity_after_workspace_switch() {
        let engine = tempfile::tempdir().unwrap();
        let next_target = tempfile::tempdir().unwrap();
        write_workflow(engine.path(), "captured", "127.0.0.1:7233");
        write_workflow(next_target.path(), "next", "127.0.0.1:7233");
        let manager = manager(
            &engine,
            WorkspaceProfile::self_targeted(engine.path().to_path_buf()),
        );
        let switch_manager = manager.clone();
        let next_target_path = next_target.path().display().to_string();

        let result = temporal_runtime_health_with_probe(
            &manager,
            Duration::from_secs(1),
            move |_| async move {
                switch_manager.set_target(Some(next_target_path)).unwrap();
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(
            result.workspace_path,
            engine.path().canonicalize().unwrap().display().to_string()
        );
        assert_eq!(
            result.workflow_path,
            engine
                .path()
                .canonicalize()
                .unwrap()
                .join(WORKFLOW_RELATIVE_PATH)
                .display()
                .to_string()
        );
        assert_eq!(
            manager.current().target_path(),
            next_target.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn all_status_values_use_the_contract_spelling() {
        assert_eq!(
            serde_json::to_value(TemporalRuntimeHealthStatus::Ready).unwrap(),
            json!("ready")
        );
        assert_eq!(
            serde_json::to_value(TemporalRuntimeHealthStatus::Unavailable).unwrap(),
            json!("unavailable")
        );
        assert_eq!(
            serde_json::to_value(TemporalRuntimeHealthStatus::TimedOut).unwrap(),
            json!("timedOut")
        );
        assert_eq!(
            serde_json::to_value(TemporalRuntimeHealthStatus::InvalidConfig).unwrap(),
            json!("invalidConfig")
        );
    }
}
