use std::path::PathBuf;

use shea_symphony::config::RuntimeConfig;
use shea_symphony::issue_workspace::discover_issue_workspaces;
use shea_symphony::lane_claim::{LaneClaim, LaneClaimActor, LaneClaimSource, LaneClaimState};
use shea_symphony::model::{native_subissue_gate_blocker, TrackerIssue};
use shea_symphony::runtime_profile::{
    persist_runtime_readiness_failure, resolve_runtime_readiness,
};
use shea_symphony::session_registry::{
    save_session_record, session_registry_path, unix_timestamp_ms, AgentSessionRecord,
    SessionStatus,
};
use shea_symphony::tracker::{adapter_from_config, ProjectFieldAssignment};
use shea_symphony::workspace::safe_identifier;

use crate::cli::CliLaneClaimSource;
use crate::lanes::claim::{project_text_field, render_parseable_lane_claim};
use crate::orchestration::{
    append_tracker_mutation_audit, current_git_branch, current_time_ms, load_config,
    preflight_canonical_checkout_for_write_mode, set_project_field_with_recovery,
    TrackerMutationAudit,
};

use super::AgentSessionLaneArg;

pub(crate) fn lane_claim_command(
    workflow_path: PathBuf,
    issue_ref: String,
    lane: AgentSessionLaneArg,
    worker: String,
    source: CliLaneClaimSource,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if worker.trim().is_empty() {
        return Err("lane claim requires a non-empty --worker".into());
    }

    let config = load_config(&workflow_path)?;
    preflight_canonical_checkout_for_write_mode(
        &config,
        &format!("{} claim", lane.label()),
        write,
    )?;

    let adapter = adapter_from_config(&config);
    let mut issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    validate_lane_claim_state(&issue, lane, &config)?;

    if lane == AgentSessionLaneArg::Main {
        let repository_root = std::env::current_dir()?;
        let workspace = if config.tracker.kind == "github_project_v2"
            && config.tracker.fixture_path.is_none()
        {
            let report = discover_issue_workspaces(&config, &issue, &repository_root)?;
            let candidate = report
                .canonical_index
                .and_then(|index| report.candidates.get(index))
                .ok_or_else(|| {
                    format!(
                        "main claim requires one adopted canonical workspace for {}; run `workspace show` then `workspace adopt` before claiming",
                        issue.identifier
                    )
                })?;
            candidate.path.clone()
        } else {
            repository_root
        };
        let readiness =
            match resolve_runtime_readiness(&config.runtime_profile, &config.tracker, &workspace) {
                Ok(readiness) => readiness,
                Err(error) => {
                    let evidence = persist_runtime_readiness_failure(
                        &config.observability.logs_root,
                        &issue.identifier,
                        &config.runtime_profile,
                        &workspace,
                        &error,
                    )?;
                    return Err(format!(
                        "main claim blocked before tracker mutation: {error}; local evidence={}",
                        evidence.display()
                    )
                    .into());
                }
            };
        println!(
            "main_claim_readiness=ok issue_ref={} profile={} status={} workspace={} tracker_mutation=false",
            issue.identifier,
            readiness
                .report
                .profile_id
                .as_deref()
                .unwrap_or("not_configured"),
            readiness.report.status,
            workspace.display()
        );

        issue = adapter
            .get_issue(&issue_ref)?
            .ok_or_else(|| format!("issue disappeared after Main readiness: {issue_ref}"))?;
        validate_lane_claim_state(&issue, lane, &config)?;
    }

    let existing_value = project_text_field(&issue, lane.claim_field());
    let claim = lane_claim_for_manual_worker(
        &issue,
        lane,
        actor_from_worker(&worker),
        source.into(),
        worker.trim(),
        existing_value.as_deref(),
    )?;
    let claim_value = render_parseable_lane_claim(&claim)?;

    if !write {
        println!(
            "{}_claim_dry_run action=claim_field issue_ref={} field={:?} run={} value={claim_value}",
            lane.label(),
            issue.identifier,
            lane.claim_field(),
            claim.run
        );
        return Ok(());
    }

    let outcome = set_project_field_with_recovery(
        adapter.as_ref(),
        &issue,
        &ProjectFieldAssignment {
            name: lane.claim_field().into(),
            value: claim_value.clone(),
        },
        "claim_field",
    )?;
    let registry_path =
        record_manual_lane_claim_evidence(&config, &issue, lane, &claim, &claim_value, &worker)?;
    let command_name = format!("{} claim", lane.label());
    if outcome.should_record_audit() {
        append_tracker_mutation_audit(
            &config,
            TrackerMutationAudit {
                command: &command_name,
                mutation_type: "claim_field",
                issue_ref: Some(&issue.identifier),
                target: Some(format!("{}={claim_value}", lane.claim_field())),
                from_state: Some(issue.state.clone()),
                to_state: None,
                reason: "manual lane worker claim",
            },
        );
    }
    println!(
        "{}_claim={} issue_ref={} field={:?} worker={} run={} registry={} value={claim_value}",
        lane.label(),
        outcome.as_str(),
        issue.identifier,
        lane.claim_field(),
        worker.trim(),
        claim.run,
        registry_path.display()
    );
    Ok(())
}

pub(crate) fn validate_lane_claim_state(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let normalized = issue.normalized_state();
    let valid = match lane {
        AgentSessionLaneArg::Main => {
            matches!(normalized.as_str(), "todo" | "in progress" | "rework")
        }
        AgentSessionLaneArg::Review => normalized == "agent review",
        AgentSessionLaneArg::Merge => normalized == "merging",
    };
    if valid {
        if matches!(lane, AgentSessionLaneArg::Main) {
            let terminal_states = config.terminal_state_set().into_iter().collect();
            if let Some(reason) = native_subissue_gate_blocker(issue, &terminal_states) {
                return Err(format!(
                    "{} claim cannot claim {}; {reason}",
                    lane.label(),
                    issue.identifier
                )
                .into());
            }
        }
        return Ok(());
    }

    Err(format!(
        "{} claim cannot claim {}; {} is currently {}",
        lane.label(),
        issue.identifier,
        issue.identifier,
        issue.state
    )
    .into())
}

fn actor_from_worker(worker: &str) -> LaneClaimActor {
    let normalized = worker.to_ascii_lowercase();
    if normalized.contains("gemini") {
        LaneClaimActor::Gemini
    } else if normalized.contains("agy") || normalized.contains("antigravity") {
        LaneClaimActor::Antigravity
    } else if normalized.contains("claude") {
        LaneClaimActor::Claude
    } else if normalized.contains("human") {
        LaneClaimActor::Human
    } else {
        LaneClaimActor::Codex
    }
}

pub(crate) fn lane_claim_for_manual_worker(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    actor: LaneClaimActor,
    source: LaneClaimSource,
    worker: &str,
    existing: Option<&str>,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    if let Some(existing) = existing {
        if let Ok(claim) = LaneClaim::parse(existing) {
            if claim.lane == lane.claim_lane()
                && claim.issue == issue.identifier
                && claim.state == LaneClaimState::Active
            {
                if claim.worker.as_deref() == Some(worker) {
                    return Ok(claim);
                }
                return Err(format!(
                    "{} already has an active {} claim owned by {} run={}",
                    issue.identifier,
                    lane.label(),
                    claim.worker.as_deref().unwrap_or(claim.actor.as_str()),
                    claim.run
                )
                .into());
            }
        } else if !existing.trim().is_empty() {
            return Err(format!(
                "{} already has an unparseable {} claim: {existing}",
                issue.identifier,
                lane.claim_field()
            )
            .into());
        }
    }

    Ok(LaneClaim::active(
        &issue.identifier,
        lane.claim_lane(),
        actor,
        source,
        current_time_ms(),
    )
    .with_worker(worker))
}

pub(crate) fn record_manual_lane_claim_evidence(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    claim: &LaneClaim,
    claim_value: &str,
    worker: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let registry_path = session_registry_path(config);
    let now_ms = unix_timestamp_ms();
    let path = std::env::current_dir()?;
    let branch = current_git_branch(&path).ok().flatten();
    let session_name = format!("manual-{}-{}", lane.label(), safe_identifier(&claim.run));
    let record = AgentSessionRecord {
        issue_id: Some(issue.id.clone()),
        issue_identifier: Some(issue.identifier.clone()),
        issue_title: Some(issue.title.clone()),
        lane: lane.label().into(),
        run_id: Some(claim.run.clone()),
        thread: Some(claim.thread.clone()),
        session_source: Some("manual-claim".into()),
        claim_value: Some(claim_value.into()),
        actor_role: Some(claim.actor.as_str().into()),
        actor_label: Some(worker.trim().into()),
        git_author: None,
        profile_id: None,
        instance_name: None,
        worktree: path,
        branch,
        backend: "codex-app-manual".into(),
        session_name,
        process_id: None,
        pane_target: String::new(),
        prompt_artifact_path: PathBuf::new(),
        log_path: PathBuf::new(),
        attach_command: "not a tmux session; manual Codex App evidence only".into(),
        attempt: 1,
        status: SessionStatus::Recorded,
        started_at_ms: now_ms,
        updated_at_ms: now_ms,
    };
    save_session_record(&registry_path, record)?;
    Ok(registry_path)
}

pub(crate) fn timeline_claim_run(value: &str) -> Option<String> {
    LaneClaim::parse(value).ok().map(|claim| claim.run)
}

pub(crate) fn timeline_claim_actor(value: &str) -> Option<String> {
    LaneClaim::parse(value)
        .ok()
        .map(|claim| claim.actor.as_str().to_string())
}

pub(crate) fn matching_lane_claim_for_session(
    issue: &TrackerIssue,
    lane: AgentSessionLaneArg,
    run_id: &str,
) -> Result<LaneClaim, Box<dyn std::error::Error>> {
    let claim_value = project_text_field(issue, lane.claim_field()).ok_or_else(|| {
        format!(
            "session start requires an existing {} claim for {}",
            lane.claim_field(),
            issue.identifier
        )
    })?;
    let claim = LaneClaim::parse(&claim_value)?;
    if claim.lane != lane.claim_lane() {
        return Err(format!(
            "session start lane mismatch for {}; claim lane={} requested lane={}",
            issue.identifier,
            claim.lane.as_str(),
            lane.label()
        )
        .into());
    }
    if claim.issue != issue.identifier {
        return Err(format!(
            "session start issue mismatch for {}; claim points at {}",
            issue.identifier, claim.issue
        )
        .into());
    }
    if claim.run != run_id {
        return Err(format!(
            "session start run mismatch for {}; claim run={} requested run={run_id}",
            issue.identifier, claim.run
        )
        .into());
    }
    if claim.state != LaneClaimState::Active {
        return Err(format!(
            "session start requires an active claim; {} claim state={}",
            issue.identifier,
            claim.state.as_str()
        )
        .into());
    }
    if claim.worker.as_deref().unwrap_or("").trim().is_empty() {
        return Err(format!(
            "session start requires a structured worker= claim for {} run={}",
            issue.identifier, claim.run
        )
        .into());
    }
    Ok(claim)
}
