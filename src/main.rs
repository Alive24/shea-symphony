//! Local Shea Symphony Temporal worker executable.
//!
//! This binary loads the tracked workflow configuration, constructs the 2607
//! Symphony runtime, and then blocks while the configured Temporal workers run.
//! It is an execution host, not a second workflow authority: deterministic
//! decisions remain in Temporal Workflows and external side effects remain in
//! Activities.

use std::path::PathBuf;

use shea_symphony::{symphony::run_symphony_workers, RuntimeConfig, WorkflowStore};

const DEFAULT_WORKFLOW_PATH: &str = "workflows/shea-symphony.md";
const WORKFLOW_PATH_ENV: &str = "SHEA_WORKFLOW_PATH";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // In 2607 the default binary is the local Temporal worker runtime.
    // Legacy CLI dogfooding remains pinned to the protected 2606-MVP branch.
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
    std::env::var_os(WORKFLOW_PATH_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKFLOW_PATH))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static WORKFLOW_PATH_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn workflow_path_defaults_to_repo_workflow() {
        let _guard = WORKFLOW_PATH_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var(WORKFLOW_PATH_ENV);
        }

        assert_eq!(workflow_path(), PathBuf::from(DEFAULT_WORKFLOW_PATH));
    }

    #[test]
    fn workflow_path_can_be_overridden_for_local_profiles() {
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
