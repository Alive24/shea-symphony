use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::normalize_state;
use shea_symphony::review::{
    classify_review_freshness, render_review_freshness_workpad, ReviewFreshnessInput,
};
use shea_symphony::review_status::{
    load_review_status, render_review_status_human, ReviewStatusOptions,
};
use shea_symphony::session_registry::unix_timestamp_ms;
use shea_symphony::tracker::adapter_from_config;
use shea_symphony::workflow::WorkflowDefinition;

use crate::orchestration::hydrate_issues_for_review_lane;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewStatusCliOptions {
    pub(crate) workflow_path: PathBuf,
    pub(crate) issue_filter: Option<String>,
    pub(crate) recent_limit: usize,
    pub(crate) verbose: bool,
    pub(crate) json: bool,
}

pub(crate) fn review_freshness(
    input: ReviewFreshnessInput,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = classify_review_freshness(input);
    println!("review_freshness={:?}", report.decision.kind);
    println!(
        "prior_human_review_valid={}",
        report.decision.prior_human_review_valid
    );
    println!(
        "human_rereview_required={}",
        report.decision.human_rereview_required
    );
    println!(
        "main_agent_target_state={}",
        report.decision.main_agent_target_state
    );
    println!(
        "authorized_next_state={}",
        report
            .decision
            .authorized_next_state
            .as_deref()
            .unwrap_or("none")
    );
    println!("rationale={}", report.decision.rationale);
    println!("\n--- review freshness evidence ---\n");
    println!("{}", render_review_freshness_workpad(None, &report)?);
    Ok(())
}

pub(crate) fn review_status(
    options: ReviewStatusCliOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&options.workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let issues = if let Some(issue_ref) = &options.issue_filter {
        adapter
            .get_issue(issue_ref)?
            .map(|issue| vec![issue])
            .ok_or_else(|| format!("issue not found: {issue_ref}"))?
    } else {
        let mut states = config.tracker.active_states.clone();
        if !states.iter().any(|state| {
            normalize_state(state) == normalize_state(&config.tracker.state_map.agent_review)
        }) {
            states.push(config.tracker.state_map.agent_review.clone());
        }
        hydrate_issues_for_review_lane(adapter.as_ref(), adapter.fetch_issues_by_states(&states)?)?
    };
    let payload = load_review_status(
        &config,
        &issues,
        &ReviewStatusOptions {
            issue_filter: options.issue_filter.clone(),
            recent_limit: options.recent_limit,
            verbose: options.verbose,
        },
        unix_timestamp_ms(),
    )?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", render_review_status_human(&payload, options.verbose));
    }
    Ok(())
}
