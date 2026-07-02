use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::{GateDecision, GateDecisionKind, TrackerIssue};
use shea_symphony::progress::run_with_progress_heartbeat;
use shea_symphony::quality_gate::{
    evaluate_issue_with_dependency_preflight, evaluate_issue_with_llm_gate,
    evaluate_issue_with_source_alignment, LlmGateMode, LlmGateOptions,
};
use shea_symphony::tracker::adapter_from_config;
use shea_symphony::workpad_templates::{render_workpad_template, WorkpadTemplateId};

use crate::orchestration::{
    append_tracker_mutation_audit, live_github_tracker, load_config, progress_spec_for_config,
    require_write_intent, tracker_backend_label, TrackerMutationAudit,
};

pub(crate) fn quality_gate(
    workflow_path: PathBuf,
    issue_ref: String,
    apply: bool,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = run_with_progress_heartbeat(
        progress_spec_for_config(&config, "github_project_read")
            .issue(issue_ref.clone())
            .backend(tracker_backend_label(&config))
            .next("inspect_issue"),
        || adapter.get_issue(&issue_ref),
    )?
    .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let decision = evaluate_issue_for_current_source(&config, &issue)?;

    println!(
        "gate={:?} dispatchable={}",
        decision.kind,
        decision.is_dispatchable()
    );
    if !decision.missing.is_empty() {
        println!("missing={}", decision.missing.join(", "));
    }
    if !decision.assumptions.is_empty() {
        println!("assumptions={}", decision.assumptions.join("; "));
    }

    if apply {
        require_write_intent(write)?;
        let workpad = gate_workpad(&issue, &decision);
        adapter.upsert_workpad(&issue_ref, &workpad)?;
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: "forge validate",
                mutation_type: "workpad_write",
                issue_ref: Some(&issue_ref),
                target: None,
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "quality gate workpad",
            },
        );
        if !decision.is_dispatchable() {
            let target_state = gate_target_state(&decision);
            adapter.set_state(&issue_ref, target_state)?;
            append_tracker_mutation_audit(
                &config,
                TrackerMutationAudit {
                    command: "project inspect",
                    mutation_type: "state_change",
                    issue_ref: Some(&issue_ref),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some(target_state.into()),
                    reason: "quality gate routing",
                },
            );
            println!("applied=true target_state={target_state}");
        } else {
            println!("applied=true target_state=unchanged");
        }
    }

    Ok(())
}

pub(crate) fn evaluate_issue_for_current_source(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<GateDecision, Box<dyn std::error::Error>> {
    let repo_root = std::env::current_dir()?;
    let expected_target = expected_target_repository(config);
    let deterministic =
        evaluate_issue_with_source_alignment(issue, &repo_root, expected_target.as_deref());
    if let Some(blocker) = live_missing_assignee_gate_blocker(config, issue) {
        return Ok(GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec![blocker],
            assumptions: deterministic.assumptions,
            notes: vec!["Live GitHub dispatch requires explicit issue ownership.".into()],
        });
    }
    let decision = evaluate_issue_with_llm_gate(
        issue,
        deterministic,
        &LlmGateOptions {
            mode: LlmGateMode::parse(&config.quality_gate.llm.mode),
            command: config.quality_gate.llm.command.clone(),
            timeout_ms: config.quality_gate.llm.timeout_ms,
        },
    );
    if !decision.is_dispatchable() {
        return Ok(decision);
    }

    let terminal_states = config.terminal_state_set().into_iter().collect();
    let dependency_preflight = evaluate_issue_with_dependency_preflight(issue, &terminal_states);
    if dependency_preflight.is_dispatchable() {
        Ok(decision)
    } else {
        Ok(dependency_preflight)
    }
}

pub(crate) fn live_missing_assignee_gate_blocker(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Option<String> {
    (live_github_tracker(config) && issue.assignees.is_empty())
        .then(|| "live GitHub issue assignee".into())
}

fn expected_target_repository(config: &RuntimeConfig) -> Option<String> {
    Some(format!(
        "{}/{}",
        config.tracker.owner.as_ref()?,
        config.tracker.repo.as_ref()?
    ))
}

pub(crate) fn gate_workpad(issue: &TrackerIssue, decision: &GateDecision) -> String {
    let assumptions = if decision.assumptions.is_empty() {
        "- None recorded.".into()
    } else {
        decision
            .assumptions
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let missing = if decision.missing.is_empty() {
        String::new()
    } else {
        format!("- Missing: {}", decision.missing.join(", "))
    };
    let notes = decision
        .notes
        .iter()
        .map(|item| format!("- Note: {item}"))
        .collect::<Vec<_>>()
        .join("\n");

    render_workpad_template(
        None,
        WorkpadTemplateId::MainQualityGate,
        &[
            ("issue_ref", issue.identifier.clone()),
            ("issue_title", issue.title.clone()),
            ("current_state", issue.state.clone()),
            ("assumptions", assumptions),
            ("decision", format!("{:?}", decision.kind)),
            ("missing", missing),
            ("notes", notes),
        ],
    )
    .expect("centralized main quality gate workpad template must render")
}

pub(crate) fn gate_target_state(decision: &GateDecision) -> &'static str {
    match decision.kind {
        GateDecisionKind::NeedToClarify | GateDecisionKind::TooBroad => "need_to_clarify",
        GateDecisionKind::Blocked => "need_human_input",
        GateDecisionKind::DuplicateAlreadyCovered => "done",
        GateDecisionKind::Ready | GateDecisionKind::ReadyWithAssumptions => "todo",
    }
}
