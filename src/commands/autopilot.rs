use std::path::{Path, PathBuf};

use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::ProjectAuditReport;
use jade_symphony::model::{normalize_state, TrackerIssue};
use jade_symphony::runtime_state::load_runtime_states;
use jade_symphony::tracker::{adapter_from_config, TrackerAdapter};
use jade_symphony::workflow::WorkflowDefinition;
use serde::Serialize;

use crate::commands::project::render_state_summary;
use crate::orchestration::{
    all_mapped_tracker_states, session_status_snapshots, single_line,
    warn_if_temporary_workflow_path,
};

mod lanes;
mod readiness;

use lanes::autopilot_lane_plans;
pub(crate) use readiness::{
    autopilot_doctor_report, autopilot_readiness, AutopilotCanonicalCheckout,
    AutopilotDoctorSummary, AutopilotReadiness, AutopilotRuntimeSummary,
};

pub(crate) fn autopilot_plan(
    workflow_path: PathBuf,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = build_autopilot_plan(&workflow_path)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        println!("{}", render_autopilot_plan_human(&snapshot));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotPlanSnapshot {
    pub(crate) schema_version: u8,
    pub(crate) read_only: bool,
    pub(crate) workflow_path: String,
    pub(crate) state_summary: String,
    pub(crate) lanes: Vec<AutopilotLanePlan>,
    pub(crate) parked_queues: Vec<AutopilotParkedQueue>,
    pub(crate) readiness: AutopilotReadiness,
    pub(crate) doctor: AutopilotDoctorSummary,
    pub(crate) canonical_checkout: AutopilotCanonicalCheckout,
    pub(crate) runtime: AutopilotRuntimeSummary,
    pub(crate) integration_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotLanePlan {
    pub(crate) lane: String,
    pub(crate) status: String,
    pub(crate) selected_issue: Option<AutopilotIssueSummary>,
    pub(crate) proposed_action: String,
    pub(crate) target_state: Option<String>,
    pub(crate) reason: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotIssueSummary {
    pub(crate) identifier: String,
    pub(crate) title: String,
    pub(crate) state: String,
    pub(crate) url: Option<String>,
    pub(crate) priority: Option<i64>,
    pub(crate) pull_request: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AutopilotParkedQueue {
    pub(crate) name: String,
    pub(crate) state: String,
    pub(crate) count: usize,
    pub(crate) issues: Vec<AutopilotIssueSummary>,
}

fn build_autopilot_plan(
    workflow_path: &Path,
) -> Result<AutopilotPlanSnapshot, Box<dyn std::error::Error>> {
    warn_if_temporary_workflow_path(workflow_path);
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;
    let adapter = adapter_from_config(&config);
    let mut integration_gaps = adapter.integration_gaps();
    let issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;

    let (runtime_states, runtime_load_error) = match load_runtime_states(&config) {
        Ok(states) => (states, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let (sessions, session_load_error) = match session_status_snapshots(&config) {
        Ok(sessions) => (sessions, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    if let Some(error) = &runtime_load_error {
        integration_gaps.push(format!("runtime_state_load_error: {error}"));
    }
    if let Some(error) = &session_load_error {
        integration_gaps.push(format!("tmux_session_status_unavailable: {error}"));
    }

    let canonical_checkout = AutopilotCanonicalCheckout::read_current(&config);
    let doctor = autopilot_doctor_report(
        workflow_path,
        &config,
        &issues,
        &runtime_states,
        &sessions,
        integration_gaps.clone(),
    );
    build_autopilot_plan_from_parts(AutopilotPlanInputs {
        workflow_path,
        config: &config,
        adapter: adapter.as_ref(),
        issues,
        doctor_report: doctor,
        canonical_checkout,
        runtime: AutopilotRuntimeSummary::from_parts(
            &runtime_states,
            &sessions,
            runtime_load_error,
            session_load_error,
        ),
        integration_gaps,
    })
}

pub(crate) struct AutopilotPlanInputs<'a> {
    pub(crate) workflow_path: &'a Path,
    pub(crate) config: &'a RuntimeConfig,
    pub(crate) adapter: &'a dyn TrackerAdapter,
    pub(crate) issues: Vec<TrackerIssue>,
    pub(crate) doctor_report: ProjectAuditReport,
    pub(crate) canonical_checkout: AutopilotCanonicalCheckout,
    pub(crate) runtime: AutopilotRuntimeSummary,
    pub(crate) integration_gaps: Vec<String>,
}

pub(crate) fn build_autopilot_plan_from_parts(
    inputs: AutopilotPlanInputs<'_>,
) -> Result<AutopilotPlanSnapshot, Box<dyn std::error::Error>> {
    let AutopilotPlanInputs {
        workflow_path,
        config,
        adapter,
        issues,
        doctor_report,
        canonical_checkout,
        runtime,
        integration_gaps,
    } = inputs;
    let lanes = autopilot_lane_plans(config, adapter, &issues)?;
    let parked_queues = autopilot_parked_queues(config, &issues);
    let doctor = AutopilotDoctorSummary::from_report(&doctor_report);
    let readiness = autopilot_readiness(
        &lanes,
        &doctor,
        &canonical_checkout,
        &runtime,
        &integration_gaps,
    );
    Ok(AutopilotPlanSnapshot {
        schema_version: 1,
        read_only: true,
        workflow_path: workflow_path.display().to_string(),
        state_summary: state_summary_value(&issues),
        lanes,
        parked_queues,
        readiness,
        doctor,
        canonical_checkout,
        runtime,
        integration_gaps,
    })
}

fn autopilot_parked_queues(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> Vec<AutopilotParkedQueue> {
    vec![
        parked_queue_for_state(
            "Human Review",
            &config.tracker.state_map.human_review,
            issues,
        ),
        parked_queue_for_state(
            "Need Human Input",
            &config.tracker.state_map.need_human_input,
            issues,
        ),
        {
            let issues = coordination_issue_summaries(config, issues);
            AutopilotParkedQueue {
                name: "Dogfood / Coordination".into(),
                state: "non-dispatchable".into(),
                count: issues.len(),
                issues,
            }
        },
    ]
}

fn parked_queue_for_state(
    name: &str,
    state: &str,
    issues: &[TrackerIssue],
) -> AutopilotParkedQueue {
    let parked = issues
        .iter()
        .filter(|issue| issue.normalized_state() == normalize_state(state))
        .map(AutopilotIssueSummary::from_issue)
        .collect::<Vec<_>>();
    AutopilotParkedQueue {
        name: name.into(),
        state: state.into(),
        count: parked.len(),
        issues: parked,
    }
}

fn coordination_issue_summaries(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> Vec<AutopilotIssueSummary> {
    let done_state = normalize_state(&config.tracker.state_map.done);
    issues
        .iter()
        .filter(|issue| issue.normalized_state() != done_state && coordination_issue_hint(issue))
        .map(AutopilotIssueSummary::from_issue)
        .collect()
}

fn coordination_issue_hint(issue: &TrackerIssue) -> bool {
    let title = issue.title.trim().to_ascii_lowercase();
    let labels = issue
        .labels
        .iter()
        .map(|label| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    title.starts_with("dogfood:")
        || title.contains("dogfood session")
        || title.contains("session coordination")
        || labels.iter().any(|label| {
            matches!(
                label.as_str(),
                "dogfood-session" | "dogfood session" | "coordination"
            )
        })
}

impl AutopilotIssueSummary {
    fn from_issue(issue: &TrackerIssue) -> Self {
        Self {
            identifier: issue.identifier.clone(),
            title: issue.title.clone(),
            state: issue.state.clone(),
            url: issue.url.clone(),
            priority: issue.priority,
            pull_request: issue.linked_pull_requests.first().and_then(|pr| {
                pr.url
                    .clone()
                    .or_else(|| pr.number.map(|n| format!("#{n}")))
            }),
        }
    }
}

fn state_summary_value(issues: &[TrackerIssue]) -> String {
    render_state_summary(issues)
        .strip_prefix("state_summary=")
        .unwrap_or("(unknown)")
        .to_string()
}

fn render_autopilot_plan_human(snapshot: &AutopilotPlanSnapshot) -> String {
    let mut lines = vec![
        "Autopilot Plan".to_string(),
        "read_only=true".to_string(),
        format!("workflow={}", snapshot.workflow_path),
        format!("state_summary={}", snapshot.state_summary),
        format!(
            "readiness={} reason={}",
            snapshot.readiness.status, snapshot.readiness.reason
        ),
        String::new(),
        "Lanes".to_string(),
    ];
    for lane in &snapshot.lanes {
        let selected = lane
            .selected_issue
            .as_ref()
            .map(|issue| format!("{} {}", issue.identifier, single_line(&issue.title)))
            .unwrap_or_else(|| "none".into());
        lines.push(format!(
            "- {} status={} selected={} action={} target={} reason={}",
            lane.lane,
            lane.status,
            selected,
            lane.proposed_action,
            lane.target_state.as_deref().unwrap_or("none"),
            lane.reason
        ));
        for evidence in lane.evidence.iter().take(3) {
            lines.push(format!("  evidence={evidence}"));
        }
    }
    lines.push(String::new());
    lines.push("Parked Queues".into());
    for queue in &snapshot.parked_queues {
        let issues = if queue.issues.is_empty() {
            "none".into()
        } else {
            queue
                .issues
                .iter()
                .map(|issue| issue.identifier.clone())
                .collect::<Vec<_>>()
                .join(",")
        };
        lines.push(format!(
            "- {} count={} issues={}",
            queue.name, queue.count, issues
        ));
    }
    lines.push(String::new());
    lines.push("Readiness Evidence".into());
    lines.push(format!(
        "- doctor blockers={} warnings={}",
        snapshot.doctor.blockers, snapshot.doctor.warnings
    ));
    lines.push(format!(
        "- canonical safe_for_write={} branch={} reason={}",
        snapshot.canonical_checkout.safe_for_write,
        snapshot
            .canonical_checkout
            .branch
            .as_deref()
            .unwrap_or("unknown"),
        snapshot
            .canonical_checkout
            .reason
            .as_deref()
            .unwrap_or("none")
    ));
    lines.push(format!(
        "- runtime active_states={} sessions={} attention={}",
        snapshot.runtime.runtime_state_count,
        snapshot.runtime.session_count,
        snapshot.runtime.session_attention_count
    ));
    for blocker in &snapshot.readiness.blockers {
        lines.push(format!("  blocker={blocker}"));
    }
    for warning in snapshot.readiness.warnings.iter().take(5) {
        lines.push(format!("  warning={warning}"));
    }
    lines.join("\n")
}
