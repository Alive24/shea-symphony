//! Local Shea Symphony Temporal worker executable.
//!
//! This binary loads the tracked workflow configuration, constructs the 2607
//! Symphony runtime, and then blocks while the configured Temporal workers run.
//! It is an execution host, not a second workflow authority: deterministic
//! decisions remain in Temporal Workflows and external side effects remain in
//! Activities.

use std::path::PathBuf;

use shea_symphony::{
    runtime_identity::{print_if_requested, RuntimeRole},
    symphony::run_symphony_workers,
    RuntimeConfig, WorkflowStore,
};

const DEFAULT_WORKFLOW_PATH: &str = ".shea/workflows/shea-symphony.md";
const WORKFLOW_PATH_ENV: &str = "SHEA_WORKFLOW_PATH";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if print_if_requested(RuntimeRole::TemporalWorker, &args)? {
        return Ok(());
    }

    // In 2607 the default binary is the local Temporal worker runtime.
    // Transitional Legacy CLI dogfooding uses the separately identified
    // shea-symphony-legacy sidecar; this composition root stays Temporal-only.
    let workflow_path = workflow_path();
    let workflow_store = WorkflowStore::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(workflow_store.active(), &workflow_path)?;

    println!(
        "Starting Shea Symphony Temporal workers from {}",
        workflow_path.display()
    );
    run_symphony_workers(config).await?;

    Ok(())
}

fn workflow_path() -> PathBuf {
    workflow_path_from(std::env::var_os(WORKFLOW_PATH_ENV))
}

fn workflow_path_from(override_path: Option<std::ffi::OsString>) -> PathBuf {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        // The checked-in 2607 profile, not the removed legacy workflows/ path,
        // is the supported worker and smoke configuration boundary.
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKFLOW_PATH))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static WORKFLOW_PATH_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn workflow_path_defaults_to_checked_in_local_profile() {
        assert_eq!(
            workflow_path_from(None),
            PathBuf::from(DEFAULT_WORKFLOW_PATH)
        );
    }

    #[test]
    fn workflow_path_can_be_overridden_for_local_profiles() {
        assert_eq!(
            workflow_path_from(Some("/tmp/local-workflow.md".into())),
            PathBuf::from("/tmp/local-workflow.md")
        );
    }

    #[test]
    fn empty_profile_override_falls_back_to_checked_in_profile() {
        assert_eq!(
            workflow_path_from(Some("".into())),
            PathBuf::from(DEFAULT_WORKFLOW_PATH)
        );
    }

    #[test]
    fn workflow_path_reads_the_explicit_environment_override() {
        let _guard = WORKFLOW_PATH_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var(WORKFLOW_PATH_ENV, "/tmp/local-workflow.md");
        }

        assert_eq!(workflow_path(), PathBuf::from("/tmp/local-workflow.md"));

        unsafe {
            std::env::remove_var(WORKFLOW_PATH_ENV);
        }
    }
}
