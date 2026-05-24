use std::path::Path;

use jade_symphony::config::RuntimeConfig;
use jade_symphony::event_log::{
    EventLog, EventRecord, TrackerMutationAuditInput, TrackerMutationAuditRecord,
};
use jade_symphony::git_handoff::{CommandOutput, HandoffCommandRunner};
use jade_symphony::lane_claim::LaneClaim;
use jade_symphony::merge_lane::{
    fetch_pull_request_status_with_recheck, merge_pull_request, MergeLaneDecision,
};
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::tracker::{
    classify_project_state_error, classify_project_state_failure_message, ProjectFieldAssignment,
    ProjectStateFailureKind, TrackerAdapter, TrackerError,
};

use super::time::current_time_ms;
use crate::lanes::claim::project_text_field;

pub(crate) struct TrackerMutationAudit<'a> {
    pub(crate) command: &'a str,
    pub(crate) mutation_type: &'a str,
    pub(crate) issue_ref: Option<&'a str>,
    pub(crate) target: Option<String>,
    pub(crate) from_state: Option<String>,
    pub(crate) to_state: Option<String>,
    pub(crate) reason: &'a str,
}

pub(crate) fn append_tracker_mutation_audit(
    config: &RuntimeConfig,
    audit: TrackerMutationAudit<'_>,
) {
    let issue_ref = audit.issue_ref.map(ToOwned::to_owned);
    let record = TrackerMutationAuditRecord::from_input(TrackerMutationAuditInput {
        command: audit.command.into(),
        mutation_type: audit.mutation_type.into(),
        issue_ref: issue_ref.clone(),
        target: audit.target,
        from_state: audit.from_state,
        to_state: audit.to_state,
        reason: audit.reason.into(),
        timestamp_ms: current_time_ms(),
    });
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    if let Err(error) = log.append(&EventRecord {
        event: "tracker_mutation".into(),
        issue_id: None,
        issue_identifier: issue_ref,
        session_id: None,
        profile_id: None,
        instance_name: None,
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: Some(record),
        message: audit.reason.into(),
    }) {
        eprintln!("audit_warning=tracker_mutation_unavailable reason={error}");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackerMutationOutcome {
    Applied,
    AlreadyApplied,
    Recovered,
}

impl TrackerMutationOutcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::AlreadyApplied => "already_applied",
            Self::Recovered => "recovered",
        }
    }

    pub(crate) fn should_record_audit(self) -> bool {
        !matches!(self, Self::AlreadyApplied)
    }
}

pub(crate) fn tracker_recovery_marker(key: &str) -> String {
    format!(
        "<!-- jade-symphony-tracker-recovery key={} -->",
        recovery_key_component(key)
    )
}

fn ensure_tracker_recovery_marker(markdown: &str, key: &str) -> String {
    let marker = tracker_recovery_marker(key);
    if markdown.contains(&marker) {
        return markdown.to_string();
    }

    let mut body = markdown.trim_end().to_string();
    if !body.is_empty() {
        body.push_str("\n\n");
    }
    body.push_str(&marker);
    body
}

pub(crate) fn recovery_key(label: &str, issue_ref: &str, seed: &str) -> String {
    format!(
        "{}-{}-{}",
        recovery_key_component(label),
        recovery_key_component(issue_ref),
        stable_recovery_hash(seed)
    )
}

fn recovery_key_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

pub(crate) fn stable_recovery_hash(value: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn issue_has_recovery_marker(issue: &TrackerIssue, key: &str) -> bool {
    issue
        .description
        .as_deref()
        .is_some_and(|description| description.contains(&tracker_recovery_marker(key)))
}

fn issue_project_field_matches(issue: &TrackerIssue, field_name: &str, value: &str) -> bool {
    project_text_field(issue, field_name).as_deref() == Some(value)
}

fn issue_state_matches(issue: &TrackerIssue, normalized_state: &str) -> bool {
    tracker_state_match_key(&issue.state) == tracker_state_match_key(normalized_state)
}

pub(crate) fn issue_is_closed(issue: &TrackerIssue) -> bool {
    issue
        .project_fields
        .get("GitHub Issue State")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|state| tracker_state_match_key(state) == "closed")
}

fn tracker_state_match_key(value: &str) -> String {
    normalize_state(value).replace('_', " ")
}

fn recoverable_project_state_failure(kind: ProjectStateFailureKind) -> bool {
    matches!(
        kind,
        ProjectStateFailureKind::Network
            | ProjectStateFailureKind::TransientBackend
            | ProjectStateFailureKind::RateLimit
    )
}

fn tracker_recovery_next_action(kind: ProjectStateFailureKind) -> &'static str {
    match kind {
        ProjectStateFailureKind::RateLimit => "wait_then_readback_or_rerun_same_lane",
        ProjectStateFailureKind::TransientBackend => "wait_then_readback_or_rerun_same_lane",
        ProjectStateFailureKind::Network => "readback_or_rerun_same_lane",
        _ => "human_input_required",
    }
}

fn recover_after_tracker_error<F>(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    mutation_type: &str,
    error: TrackerError,
    expected: F,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>>
where
    F: Fn(&TrackerIssue) -> bool,
{
    let kind = classify_project_state_error(&error);
    if !recoverable_project_state_failure(kind) {
        return Err(error.into());
    }

    match adapter.get_issue(issue_ref) {
        Ok(Some(issue)) if expected(&issue) => {
            println!(
                "tracker_recovery action=recovered mutation_type={} issue={} failure_kind={} next=continue",
                mutation_type,
                issue_ref,
                kind.as_str()
            );
            Ok(TrackerMutationOutcome::Recovered)
        }
        Ok(Some(issue)) => Err(format!(
            "recoverable_tracker_mutation_uncertain mutation_type={} issue={} failure_kind={} current_state={:?} next={} error={}",
            mutation_type,
            issue_ref,
            kind.as_str(),
            issue.state,
            tracker_recovery_next_action(kind),
            error
        )
        .into()),
        Ok(None) => Err(format!(
            "recoverable_tracker_mutation_uncertain mutation_type={} issue={} failure_kind={} current_state=missing next={} error={}",
            mutation_type,
            issue_ref,
            kind.as_str(),
            tracker_recovery_next_action(kind),
            error
        )
        .into()),
        Err(readback_error) => {
            let readback_kind = classify_project_state_error(&readback_error);
            Err(format!(
                "recoverable_tracker_mutation_readback_failed mutation_type={} issue={} failure_kind={} readback_kind={} next={} error={} readback_error={}",
                mutation_type,
                issue_ref,
                kind.as_str(),
                readback_kind.as_str(),
                tracker_recovery_next_action(kind),
                error,
                readback_error
            )
            .into())
        }
    }
}

pub(crate) fn set_project_field_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    assignment: &ProjectFieldAssignment,
    mutation_type: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if issue_project_field_matches(issue, &assignment.name, &assignment.value) {
        println!(
            "tracker_recovery action=already_applied mutation_type={} issue={} field={:?}",
            mutation_type, issue.identifier, assignment.name
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    match adapter.set_project_field(&issue.identifier, assignment) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => recover_after_tracker_error(
            adapter,
            &issue.identifier,
            mutation_type,
            error,
            |readback| issue_project_field_matches(readback, &assignment.name, &assignment.value),
        ),
    }
}

pub(crate) fn set_state_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    normalized_state: &str,
    mutation_type: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(|issue| issue_state_matches(issue, normalized_state)) {
        println!(
            "tracker_recovery action=already_applied mutation_type={} issue={} state={}",
            mutation_type, issue_ref, normalized_state
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    match adapter.set_state(issue_ref, normalized_state) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, mutation_type, error, |readback| {
                issue_state_matches(readback, normalized_state)
            })
        }
    }
}

pub(crate) fn upsert_workpad_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    markdown: &str,
    key: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(|issue| issue_has_recovery_marker(issue, key)) {
        println!(
            "tracker_recovery action=already_applied mutation_type=workpad_write issue={} key={}",
            issue_ref,
            recovery_key_component(key)
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    let body = ensure_tracker_recovery_marker(markdown, key);
    match adapter.upsert_workpad(issue_ref, &body) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, "workpad_write", error, |readback| {
                issue_has_recovery_marker(readback, key)
            })
        }
    }
}

pub(crate) fn add_timeline_comment_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
    markdown: &str,
    key: &str,
    mutation_type: &str,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(|issue| issue_has_recovery_marker(issue, key)) {
        println!(
            "tracker_recovery action=already_applied mutation_type={} issue={} key={}",
            mutation_type,
            issue_ref,
            recovery_key_component(key)
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    let body = ensure_tracker_recovery_marker(markdown, key);
    match adapter.add_issue_comment(issue_ref, &body) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, mutation_type, error, |readback| {
                issue_has_recovery_marker(readback, key)
            })
        }
    }
}

pub(crate) fn close_issue_with_recovery(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    initial_issue: Option<&TrackerIssue>,
) -> Result<TrackerMutationOutcome, Box<dyn std::error::Error>> {
    if initial_issue.is_some_and(issue_is_closed) {
        println!(
            "tracker_recovery action=already_applied mutation_type=issue_close issue={issue_ref}"
        );
        return Ok(TrackerMutationOutcome::AlreadyApplied);
    }

    match adapter.close_issue(issue_ref) {
        Ok(()) => Ok(TrackerMutationOutcome::Applied),
        Err(TrackerError::NotImplemented(message)) => {
            eprintln!("merge_once_warning=issue_close_unavailable reason={message}");
            Ok(TrackerMutationOutcome::AlreadyApplied)
        }
        Err(error) => {
            recover_after_tracker_error(adapter, issue_ref, "issue_close", error, issue_is_closed)
        }
    }
}

pub(crate) fn merge_completion_recovery_key(issue: &TrackerIssue, pr_ref: &str) -> String {
    let run = project_text_field(issue, "Merging Agent")
        .as_deref()
        .and_then(|value| LaneClaim::parse(value).ok())
        .map(|claim| claim.run)
        .unwrap_or_else(|| "run-not-recorded".into());
    recovery_key(
        "merge-completion",
        &issue.identifier,
        &format!("{}|{}|{}", issue.identifier, run, pr_ref),
    )
}

pub(crate) fn merge_decision_recovery_key(
    issue: &TrackerIssue,
    decision: &MergeLaneDecision,
) -> String {
    recovery_key(
        "merge-decision",
        &issue.identifier,
        &format!(
            "{}|{:?}|{}|{}",
            issue.identifier,
            decision.kind,
            decision.pr_url.as_deref().unwrap_or("missing"),
            decision.target_state.unwrap_or("none")
        ),
    )
}

pub(crate) fn merge_pull_request_with_recovery(
    pr_ref: &str,
    runner: &dyn HandoffCommandRunner,
    cwd: &Path,
) -> Result<(CommandOutput, TrackerMutationOutcome), Box<dyn std::error::Error>> {
    match merge_pull_request(pr_ref, runner, cwd) {
        Ok(output) => Ok((output, TrackerMutationOutcome::Applied)),
        Err(error) => {
            let kind = classify_project_state_failure_message(&error.to_string());
            if !recoverable_project_state_failure(kind) {
                return Err(error.into());
            }
            match fetch_pull_request_status_with_recheck(pr_ref, runner, cwd, 2) {
                Ok(status) if normalize_state(&status.state) == "merged" => {
                    println!(
                        "tracker_recovery action=recovered mutation_type=pr_merge pr={} failure_kind={} next=continue",
                        pr_ref,
                        kind.as_str()
                    );
                    Ok((
                        CommandOutput {
                            status: 0,
                            stdout: format!(
                                "merge command failed after possible server-side success; readback shows PR merged: {pr_ref}"
                            ),
                            stderr: error.to_string(),
                        },
                        TrackerMutationOutcome::Recovered,
                    ))
                }
                Ok(status) => Err(format!(
                    "recoverable_tracker_mutation_uncertain mutation_type=pr_merge pr={} failure_kind={} readback_state={} next={} error={}",
                    pr_ref,
                    kind.as_str(),
                    status.state,
                    tracker_recovery_next_action(kind),
                    error
                )
                .into()),
                Err(readback_error) => Err(format!(
                    "recoverable_tracker_mutation_readback_failed mutation_type=pr_merge pr={} failure_kind={} next={} error={} readback_error={}",
                    pr_ref,
                    kind.as_str(),
                    tracker_recovery_next_action(kind),
                    error,
                    readback_error
                )
                .into()),
            }
        }
    }
}
