use std::path::Path;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::workflow::WorkflowDefinition;

pub(crate) fn require_write_intent(write: bool) -> Result<(), Box<dyn std::error::Error>> {
    if write {
        Ok(())
    } else {
        Err("live write command requires explicit --write".into())
    }
}

pub(crate) fn warn_if_temporary_workflow_path(workflow_path: &Path) {
    if let Some(warning) = temporary_workflow_warning(workflow_path) {
        eprintln!("{warning}");
    }
}

pub(crate) fn temporary_workflow_warning(workflow_path: &Path) -> Option<String> {
    if !is_temporary_workflow_path(workflow_path) {
        return None;
    }
    Some(format!(
        "workflow_warning=temporary_path path={} action=promote durable_config=examples/ docs=docs/operator-dogfood.md",
        workflow_path.display()
    ))
}

fn is_temporary_workflow_path(workflow_path: &Path) -> bool {
    [Path::new("/private/tmp"), Path::new("/tmp")]
        .iter()
        .any(|prefix| workflow_path.starts_with(prefix))
        || workflow_path.starts_with(std::env::temp_dir())
}

pub(crate) fn load_config(
    workflow_path: &Path,
) -> Result<RuntimeConfig, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;
    Ok(config)
}
