use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::doctor::{
    audit_project_issues, draft_pr_repair_candidates, human_review_repair_candidates,
    render_doctor_repair_workpad, render_human_review_repair_workpad, ProjectAuditReport,
};
use shea_symphony::git_handoff::{ensure_pull_request_ready, ProcessHandoffCommandRunner};
use shea_symphony::model::TrackerIssue;
use shea_symphony::tracker::{adapter_from_config, TrackerAdapter};
use shea_symphony::workflow::WorkflowDefinition;

use crate::orchestration::{
    all_mapped_tracker_states, append_tracker_mutation_audit, TrackerMutationAudit,
};

use super::{hydrate_issues_for_doctor, DoctorRepairIssueOptions};

pub(super) fn apply_doctor_auto_fix(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    report: &ProjectAuditReport,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidates = human_review_repair_candidates(report);
    let draft_pr_candidates = draft_pr_repair_candidates(report);
    println!(
        "doctor_auto_fix safe_candidates={} write={write}",
        candidates.len()
    );
    for violation in draft_pr_candidates {
        println!(
            "doctor_auto_fix action=skip issue={} code={} reason=pr_ready_requires_operator_confirmation",
            violation.issue_ref, violation.code
        );
    }
    for violation in candidates {
        println!(
            "doctor_auto_fix action=move issue={} from={:?} to=agent_review",
            violation.issue_ref, violation.state
        );
        if write {
            let workpad = render_human_review_repair_workpad(violation);
            adapter.add_issue_comment(&violation.issue_ref, &workpad)?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor --auto-fix",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "doctor auto-fix evidence",
                },
            );
            adapter.set_state(&violation.issue_ref, "agent_review")?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor --auto-fix",
                    mutation_type: "state_change",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "safe doctor auto-fix for invalid Human Review boundary",
                },
            );
        } else {
            println!(
                "doctor_auto_fix_dry_run action=timeline_comment issue={} evidence=human_review_missing_review_evidence",
                violation.issue_ref
            );
            println!(
                "doctor_auto_fix_dry_run action=set_state issue={} target_state=agent_review",
                violation.issue_ref
            );
        }
    }
    Ok(())
}

pub(super) fn doctor_repair_issue(
    config: &RuntimeConfig,
    adapter: &dyn TrackerAdapter,
    issues: &[TrackerIssue],
    report: &ProjectAuditReport,
    repair: &DoctorRepairIssueOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let issue = issues
        .iter()
        .find(|issue| issue_ref_matches(&issue.identifier, &repair.issue_ref))
        .ok_or_else(|| format!("doctor repair could not find issue {}", repair.issue_ref))?;
    println!(
        "doctor_repair issue={} state={:?} write={} move_need_human_input={} mark_pr_ready={} confirm_handoff_ready={}",
        issue.identifier,
        issue.state,
        repair.write,
        repair.move_need_human_input,
        repair.mark_pr_ready,
        repair.confirm_handoff_ready
    );
    println!(
        "safe=no_op command=\"doctor repair {}\"",
        issue.identifier.trim_start_matches('#')
    );
    println!("uncertain=resume command=\"main loop <workflow> --write\" reason=requires operator confirmation and live workspace inspection");
    println!("uncertain=reset reason=requires confirming no useful work would be discarded");
    println!("uncertain=move_need_human_input command=\"doctor repair {} --move-need-human-input --write\" reason=records evidence before tracker mutation", issue.identifier.trim_start_matches('#'));
    println!("uncertain=mark_pr_ready command=\"doctor repair {} --mark-pr-ready --confirm-handoff-ready --write\" reason=requires operator-confirmed handoff evidence", issue.identifier.trim_start_matches('#'));
    println!("dangerous=delete_worktree reason=out_of_scope_for_doctor_repair");

    if repair.move_need_human_input {
        let workpad = render_doctor_repair_workpad(issue, report, "move_need_human_input");
        if repair.write {
            adapter.add_issue_comment(&issue.identifier, &workpad)?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&issue.identifier),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some("need_human_input".into()),
                    reason: "doctor repair evidence before human-input escalation",
                },
            );
            adapter.set_state(&issue.identifier, "need_human_input")?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "state_change",
                    issue_ref: Some(&issue.identifier),
                    target: None,
                    from_state: Some(issue.state.clone()),
                    to_state: Some("need_human_input".into()),
                    reason: "doctor repair escalated uncertain runtime state",
                },
            );
        } else {
            println!(
                "doctor_repair_dry_run action=timeline_comment issue={} evidence=doctor_repair",
                issue.identifier
            );
            println!(
                "doctor_repair_dry_run action=set_state issue={} target_state=need_human_input",
                issue.identifier
            );
        }
    }

    if repair.mark_pr_ready {
        if !repair.confirm_handoff_ready {
            println!(
                "doctor_repair_dry_run action=blocked issue={} reason=missing_confirm_handoff_ready",
                issue.identifier
            );
            if repair.write {
                return Err(
                    "doctor repair --mark-pr-ready requires --confirm-handoff-ready".into(),
                );
            }
            return Ok(());
        }
        let pr_ref = draft_pull_request_repair_target(issue)?;
        let workpad = render_doctor_repair_workpad(issue, report, "mark_pr_ready");
        if repair.write {
            adapter.add_issue_comment(&issue.identifier, &workpad)?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&issue.identifier),
                    target: Some(pr_ref.clone()),
                    from_state: Some(issue.state.clone()),
                    to_state: Some(issue.state.clone()),
                    reason: "doctor repair evidence before PR ready mutation",
                },
            );
            let ready = ensure_pull_request_ready(
                &pr_ref,
                &ProcessHandoffCommandRunner,
                &std::env::current_dir()?,
            )?;
            append_tracker_mutation_audit(
                config,
                TrackerMutationAudit {
                    command: "doctor repair",
                    mutation_type: "pr_ready",
                    issue_ref: Some(&issue.identifier),
                    target: Some(ready.pr_url.clone()),
                    from_state: Some(issue.state.clone()),
                    to_state: Some(issue.state.clone()),
                    reason: "operator-confirmed draft PR handoff repair",
                },
            );
            println!(
                "doctor_repair_action=mark_pr_ready issue={} url={} was_draft={} marked_ready={}",
                issue.identifier, ready.pr_url, ready.was_draft, ready.marked_ready
            );
        } else {
            println!(
                "doctor_repair_dry_run action=timeline_comment issue={} evidence=doctor_repair_mark_pr_ready",
                issue.identifier
            );
            println!(
                "doctor_repair_dry_run action=pr_ready issue={} pr_ref={} requires=confirm_handoff_ready",
                issue.identifier, pr_ref
            );
        }
    }

    Ok(())
}

fn draft_pull_request_repair_target(
    issue: &TrackerIssue,
) -> Result<String, Box<dyn std::error::Error>> {
    issue
        .linked_pull_requests
        .iter()
        .find(|pr| pr.is_draft == Some(true))
        .and_then(|pr| {
            pr.url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
        })
        .ok_or_else(|| {
            format!(
                "doctor repair could not find a linked draft PR for {}",
                issue.identifier
            )
            .into()
        })
}

fn issue_ref_matches(left: &str, right: &str) -> bool {
    left.trim().trim_start_matches('#') == right.trim().trim_start_matches('#')
}

pub(crate) fn doctor_repair_human_review(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;
    let issues = hydrate_issues_for_doctor(adapter.as_ref(), issues)?;
    let report = audit_project_issues(&issues);
    let candidates = human_review_repair_candidates(&report);

    println!(
        "doctor_repair_human_review candidates={} write={write}",
        candidates.len()
    );
    for violation in candidates {
        println!(
            "doctor_repair_human_review action=move issue={} from={:?} to=agent_review",
            violation.issue_ref, violation.state
        );
        if write {
            let workpad = render_human_review_repair_workpad(violation);
            adapter.add_issue_comment(&violation.issue_ref, &workpad)?;
            append_tracker_mutation_audit(
                &config,
                TrackerMutationAudit {
                    command: "doctor-repair-human-review",
                    mutation_type: "timeline_comment",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "doctor repair evidence",
                },
            );
            adapter.set_state(&violation.issue_ref, "agent_review")?;
            append_tracker_mutation_audit(
                &config,
                TrackerMutationAudit {
                    command: "doctor-repair-human-review",
                    mutation_type: "state_change",
                    issue_ref: Some(&violation.issue_ref),
                    target: None,
                    from_state: Some(violation.state.clone()),
                    to_state: Some("agent_review".into()),
                    reason: "repair invalid Human Review boundary",
                },
            );
        } else {
            println!(
                "doctor_repair_human_review_dry_run action=timeline_comment issue={} evidence=human_review_missing_review_evidence",
                violation.issue_ref
            );
            println!(
                "doctor_repair_human_review_dry_run action=set_state issue={} target_state=agent_review",
                violation.issue_ref
            );
        }
    }

    Ok(())
}
