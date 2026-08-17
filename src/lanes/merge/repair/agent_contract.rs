use shea_symphony::git_handoff::CommandOutput;
use shea_symphony::lane_claim::LaneClaim;
use shea_symphony::model::{AgentEvent, TrackerIssue};
use shea_symphony::prompt::render_template_with_values;
use shea_symphony::prompt_runtime::{merge_conflict_repair_values, MergeConflictRepairEnvelope};
use shea_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::lanes::claim::render_prompt_with_claim;

#[allow(clippy::too_many_arguments)]
pub(super) fn merge_agent_conflict_repair_prompt(
    workflow: &WorkflowDefinition,
    issue: &TrackerIssue,
    claim: &LaneClaim,
    pr_ref: &str,
    head_ref_name: &str,
    expected_base: &str,
    conflict_summary: &str,
    mechanical_output: &CommandOutput,
) -> Result<String, shea_symphony::prompt::PromptError> {
    let mut prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(AgentLane::MergeAgent),
        issue,
        None,
        Some(claim),
    )?;
    let values = merge_conflict_repair_values(MergeConflictRepairEnvelope {
        pr_ref,
        head_ref_name,
        expected_base,
        conflict_summary,
        mechanical_stderr: &mechanical_output.stderr,
    });
    let template = workflow
        .backend_prompt("merge_repair")
        .map_err(|error| shea_symphony::prompt::PromptError::Context(error.to_string()))?;
    let boundary = render_template_with_values(template, &values)?;
    prompt.push_str("\n\n");
    prompt.push_str(&boundary);
    Ok(prompt)
}

pub(super) fn agent_events_text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::Message { text, .. } => Some(text.as_str()),
            AgentEvent::Completed { summary, .. } => Some(summary.as_str()),
            AgentEvent::Failed { error, .. } => Some(error.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn merge_agent_reports_repaired(text: &str) -> bool {
    text.contains("MERGE_AGENT_DECISION: repaired")
}

pub(crate) fn merge_agent_requests_human_input(text: &str) -> bool {
    text.contains("MERGE_AGENT_DECISION: needs_human_input")
        || text.to_ascii_lowercase().contains("semantic uncertainty")
}

pub(super) fn merge_agent_resolution_summary(text: &str) -> String {
    marker_line(text, "RESOLUTION_SUMMARY:")
        .unwrap_or_else(|| "Merge-agent reported repaired conflict resolution.".into())
}

pub(super) fn merge_agent_semantic_safety(text: &str) -> String {
    marker_line(text, "SEMANTIC_SAFETY:").unwrap_or_else(|| {
        "Merge-agent reported that reviewed implementation intent was preserved.".into()
    })
}

fn marker_line(text: &str, marker: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(marker).map(str::trim))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
