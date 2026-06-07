use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::TrackerIssue;
use shea_symphony::progress::run_with_progress_heartbeat;
use shea_symphony::prompt::smoke_render_prompt;
use shea_symphony::prompt_runtime::{PROMPT_RENDERER_MODE, RUNTIME_ENVELOPES};
use shea_symphony::tracker::adapter_from_config;
use shea_symphony::workflow::{AgentLane, WorkflowDefinition};
use shea_symphony::workpad_templates::{smoke_render_workpad_template, workpad_template_readback};

use crate::commands::gate::evaluate_issue_for_current_source;
use crate::commands::project::{filter_issues_by_state, render_state_summary};
use crate::orchestration::{
    progress_spec_for_config, tracker_backend_label, warn_if_temporary_workflow_path,
};

pub(crate) fn validate(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(&workflow_path);
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    println!("workflow={}", workflow_path.display());
    println!("tracker={}", config.tracker.kind);
    println!("backend={}", config.backend.kind);
    println!("workspace_root={}", config.workspace.root.display());
    println!("prompt_template_bytes={}", workflow.prompt_template.len());
    println!("prompt_renderer={PROMPT_RENDERER_MODE}");
    let smoke_issue = validate_smoke_issue();
    for lane in [
        AgentLane::MainAgent,
        AgentLane::ReviewAgent,
        AgentLane::MergeAgent,
    ] {
        let source = workflow.prompt_source_for_lane(lane);
        let path = source
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".into());
        println!(
            "prompt_source.{}={} path={} bytes={}",
            lane.config_key(),
            source.kind.as_str(),
            path,
            workflow.prompt_for_lane(lane).len()
        );
        smoke_render_prompt(workflow.prompt_for_lane(lane), &smoke_issue)?;
        println!("prompt_template_smoke.{}=pass", lane.config_key());
    }
    for envelope in RUNTIME_ENVELOPES {
        println!(
            "runtime_envelope={} lane={} backend={} path={} purpose={}",
            envelope.id, envelope.lane, envelope.backend, envelope.path, envelope.purpose
        );
    }
    let workpad_smoke_values = validate_workpad_smoke_values();
    for template in workpad_template_readback(&workflow) {
        let diagnostic = template
            .source
            .diagnostic()
            .map(|diagnostic| format!(" diagnostic={diagnostic}"))
            .unwrap_or_default();
        smoke_render_workpad_template(&template, &workpad_smoke_values)?;
        println!(
            "workpad_template.{}={} path={} bytes={} smoke=pass{}",
            template.id.key(),
            template.source.kind(),
            template.source.path_display(),
            template.body.len(),
            diagnostic
        );
    }
    println!("status=valid");
    Ok(())
}

pub(crate) fn inspect(
    workflow_path: PathBuf,
    state_filters: Vec<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .backend(tracker_backend_label(&config))
            .next("load_project_summary"),
        || adapter.list_project_summary_issues(),
    )?;
    let issues = filter_issues_by_state(issues, &state_filters);

    if !state_filters.is_empty() {
        println!("state_filter={}", state_filters.join(","));
    }
    println!("issues={}", issues.len());
    println!("{}", render_state_summary(&issues));
    for issue in issues {
        let gate = evaluate_issue_for_current_source(&config, &issue)?;
        println!(
            "- {} {} state={} gate={:?}",
            issue.identifier, issue.title, issue.state, gate.kind
        );
        if !gate.missing.is_empty() {
            println!("  missing={}", gate.missing.join(", "));
        }
        if !gate.assumptions.is_empty() {
            println!("  assumptions={}", gate.assumptions.join("; "));
        }
    }

    for gap in adapter.integration_gaps() {
        println!("integration_gap={gap}");
    }

    Ok(())
}

fn validate_smoke_issue() -> TrackerIssue {
    TrackerIssue {
        tracker_kind: "github_project_v2".into(),
        id: "I_smoke".into(),
        item_id: Some("PVTI_smoke".into()),
        identifier: "#0".into(),
        title: "Validate smoke issue".into(),
        description: Some("## Issue Goal\nValidate strict Liquid-compatible rendering.".into()),
        url: Some("https://github.com/Alive24/shea-symphony/issues/0".into()),
        state: "Todo".into(),
        labels: vec!["smoke".into(), "template".into()],
        assignees: vec!["Alive24".into()],
        priority: Some(2),
        branch_name: Some("feature/issue-0-smoke".into()),
        linked_pull_requests: vec![],
        blocked_by: vec![],
        project_fields: Default::default(),
        created_at: Some("2026-06-06T00:00:00Z".into()),
        updated_at: Some("2026-06-06T00:00:00Z".into()),
    }
}

fn validate_workpad_smoke_values() -> Vec<(&'static str, String)> {
    [
        "action",
        "actor",
        "agent_review_note",
        "artifact_line",
        "assignees",
        "assumptions",
        "attempt_details",
        "backend",
        "branch",
        "claim",
        "classifier",
        "current_claim",
        "current_state",
        "decision",
        "doctor_findings",
        "error",
        "event",
        "evidence",
        "evidence_summary",
        "extra_lines",
        "findings_section",
        "first_job_id",
        "gemini_health_lines",
        "gemini_health_section",
        "generated_at",
        "head",
        "inconclusive_section",
        "input_state",
        "issue_ref",
        "issue_title",
        "job_id",
        "job_state",
        "last_transition",
        "ledger_line",
        "merge_action",
        "merge_repair_evidence",
        "message",
        "missing",
        "notes",
        "operator_action_section",
        "operator_confirmation",
        "parent_final_base_branch",
        "parent_integration_branch",
        "parent_issue_ref",
        "parent_issue_title",
        "pass_evidence_section",
        "planned_handoff",
        "post_merge_readback",
        "preflight",
        "previous_job_id",
        "pr",
        "pr_line",
        "project_pr_link_verified",
        "pull_request",
        "pull_request_is_draft",
        "readbacks",
        "reason",
        "repair",
        "repair_evidence",
        "repeat_count",
        "required_human_input",
        "result",
        "result_note",
        "retry_delay_ms",
        "reviewer_backend",
        "rework_title",
        "run",
        "run_evidence",
        "run_id",
        "runtime_ownership_marker",
        "signature",
        "status",
        "stderr_section",
        "stdout_section",
        "target_state",
        "terminal_claim",
        "usage_limit_section",
        "validation_summary",
        "violation_code",
        "worker_key",
        "workspace_path",
    ]
    .into_iter()
    .map(|key| (key, format!("smoke_{key}")))
    .collect()
}
