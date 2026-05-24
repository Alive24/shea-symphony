use jade_symphony::git_handoff::CommandOutput;
use jade_symphony::lane_claim::LaneClaim;
use jade_symphony::model::{AgentEvent, TrackerIssue};
use jade_symphony::workflow::{AgentLane, WorkflowDefinition};

use crate::{render_prompt_with_claim, single_line};

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
) -> Result<String, jade_symphony::prompt::PromptError> {
    let mut prompt = render_prompt_with_claim(
        workflow.prompt_for_lane(AgentLane::MergeAgent),
        issue,
        None,
        Some(claim),
    )?;
    prompt.push_str(
        "\n\n## Merge-Agent Conflict Repair Task\n\n\
You are repairing the existing approved PR branch in place. Preserve the intent that already passed Agent Review and Human Review. Resolve only conflicts caused by merging the target base into this PR branch. Do not create a replacement PR, do not switch workspaces, and do not route through Rework.\n\n",
    );
    prompt.push_str(&format!("- Pull request: `{pr_ref}`\n"));
    prompt.push_str(&format!("- Head branch: `{head_ref_name}`\n"));
    prompt.push_str(&format!("- Expected base: `{expected_base}`\n"));
    prompt.push_str(&format!("- Conflict summary: {conflict_summary}\n"));
    prompt.push_str(&format!(
        "- Mechanical merge stderr: `{}`\n",
        single_line(&mechanical_output.stderr)
    ));
    prompt.push_str(
        "\n### Required Output Marker\n\n\
End your final response with one of these exact markers:\n\
- `MERGE_AGENT_DECISION: repaired` only if the resolution preserves reviewed intent and verification can proceed.\n\
- `MERGE_AGENT_DECISION: needs_human_input` if there is semantic uncertainty, unrelated drift, unsafe branch/worktree state, or missing verification confidence.\n\n\
Also include `RESOLUTION_SUMMARY:` and `SEMANTIC_SAFETY:` lines. Leave the merge resolution staged or ready for `git add -A`; the merge lane will commit, verify cleanliness, push, and keep the issue in `Merging` for the next tick.\n",
    );
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
