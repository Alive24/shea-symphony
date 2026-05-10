use std::path::PathBuf;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::status_surface::render_snapshot;
use jade_symphony::tracker::adapter_from_config;
use jade_symphony::workflow::WorkflowDefinition;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let workflow_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("WORKFLOW.md"));

    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let integration_gaps = adapter.integration_gaps();
    let issues = adapter.list_dispatchable_issues()?;
    let orchestrator = Orchestrator::new(config);
    let mut plan = orchestrator.plan_dispatch(issues);
    plan.integration_gaps.extend(integration_gaps);

    println!("{}", render_snapshot(&plan.snapshot));

    if !plan.integration_gaps.is_empty() {
        println!("\nIntegration gaps:");
        for gap in &plan.integration_gaps {
            println!("- {gap}");
        }
    }

    Ok(())
}
