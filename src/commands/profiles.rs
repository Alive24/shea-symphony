use std::path::PathBuf;

use shea_symphony::profiles::{discover_execution_profiles, selected_execution_profile};
use shea_symphony::runtime_profile::resolve_runtime_readiness;

use crate::orchestration::load_config;

pub(crate) fn list_profiles(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let profiles = discover_execution_profiles(&config.profiles)?;
    let selected = selected_execution_profile(&config.profiles)?;

    println!("profiles={}", profiles.len());
    if let Some(profile) = selected {
        println!("selected_profile={}", profile.profile_id);
        println!("selected_instance={}", profile.instance_name);
    }
    for profile in profiles {
        println!(
            "- profile_id={} instance_name={} source={} workspace_namespace={} backend={}",
            profile.profile_id,
            profile.instance_name,
            profile.source,
            profile.workspace_namespace,
            profile.backend.as_deref().unwrap_or("configured")
        );
    }
    let workspace = std::env::current_dir()?;
    let runtime = resolve_runtime_readiness(&config.runtime_profile, &config.tracker, &workspace)?;
    println!(
        "runtime_profile_path={}",
        runtime.report.profile_path.display()
    );
    println!("runtime_profile_status={}", runtime.report.status);
    println!(
        "runtime_profile_id={}",
        runtime
            .report
            .profile_id
            .as_deref()
            .unwrap_or("not_configured")
    );
    println!("runtime_profile_workspace={}", workspace.display());
    for evidence in runtime.report.evidence {
        println!("runtime_profile_evidence={evidence}");
    }
    Ok(())
}
