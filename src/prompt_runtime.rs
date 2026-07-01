use crate::{lane_claim::LaneClaim, prompt::STRICT_LIQUID_RENDERER_MODE};

pub const PROMPT_RENDERER_MODE: &str = STRICT_LIQUID_RENDERER_MODE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeEnvelopeSpec {
    pub id: &'static str,
    pub lane: &'static str,
    pub backend: &'static str,
    pub path: &'static str,
    pub purpose: &'static str,
}

pub const ASSIGNED_LANE_CLAIM_ENVELOPE: RuntimeEnvelopeSpec = RuntimeEnvelopeSpec {
    id: "assigned_lane_claim",
    lane: "main,merge",
    backend: "all",
    path: "claim/render_prompt_with_claim",
    purpose: "claim identity and run evidence",
};

pub const CODEX_APP_SERVER_HANDOFF_ENVELOPE: RuntimeEnvelopeSpec = RuntimeEnvelopeSpec {
    id: "codex_app_server_handoff_boundary",
    lane: "main",
    backend: "codex app-server",
    path: "main_loop/execution",
    purpose: "app-server child-turn mutation boundary",
};

pub const AUTOMATIC_HEADLESS_REVIEW_ENVELOPE: RuntimeEnvelopeSpec = RuntimeEnvelopeSpec {
    id: "automatic_headless_review_boundary",
    lane: "review",
    backend: "agy-cli",
    path: "review/automatic",
    purpose: "headless review safety and stdout protocol",
};

pub const MERGE_CONFLICT_REPAIR_TASK_ENVELOPE: RuntimeEnvelopeSpec = RuntimeEnvelopeSpec {
    id: "merge_conflict_repair_task",
    lane: "merge",
    backend: "merge agent",
    path: "merge/repair/agent_contract",
    purpose: "merge repair scope and conflict context",
};

pub const MERGE_REQUIRED_OUTPUT_MARKER_ENVELOPE: RuntimeEnvelopeSpec = RuntimeEnvelopeSpec {
    id: "merge_required_output_markers",
    lane: "merge",
    backend: "merge agent",
    path: "merge/repair/agent_contract",
    purpose: "required merge-agent routing markers",
};

pub const RUNTIME_ENVELOPES: &[RuntimeEnvelopeSpec] = &[
    ASSIGNED_LANE_CLAIM_ENVELOPE,
    CODEX_APP_SERVER_HANDOFF_ENVELOPE,
    AUTOMATIC_HEADLESS_REVIEW_ENVELOPE,
    MERGE_CONFLICT_REPAIR_TASK_ENVELOPE,
    MERGE_REQUIRED_OUTPUT_MARKER_ENVELOPE,
];

pub const CODEX_APP_SERVER_CONTINUE_PROMPT: &str = "Continue";

pub const CODEX_APP_SERVER_HANDOFF_BOUNDARY: &str = "\n\n## Codex App-Server Runtime Boundary\n\n\
This run is executing inside the Codex app-server backend. Treat the app-server \
turn as the implementation and local-verification worker only. Do not run \
GitHub Project reads or mutations, do not create or update pull requests, and \
do not attempt final Project state transitions from inside this child turn. \
Leave a concise terminal summary of changed files, verification commands, and \
any blocker. The outer Shea Symphony CLI will commit eligible worktree changes, \
publish or update the PR, write durable workpad evidence, verify linked PR \
readback, and perform the final `Agent Review` handoff.\n";

pub const AUTOMATIC_HEADLESS_REVIEW_BOUNDARY: &str =
    "\n\n## Automatic Headless Review Boundary\n\n\
This Gemini process is running under Shea Symphony automatic `review loop` or `review once`.\n\
Shea Symphony CLI has already claimed or will own any Review Agent claim, timeline comment write,\n\
issue body update, and Project state transition outside this process.\n\n\
Do not run mutating Shea Symphony or GitHub commands, including `review claim`, `review pass`,\n\
`review reject`, `project set-state`, `project workpad`, `forge`, `gh issue edit`, `gh issue comment`, raw\n\
Project GraphQL mutations, or Project UI changes. Do not activate or follow any manual review\n\
skill that tells you to mutate Project state.\n\n\
Return review evidence in stdout only. Start with exactly one line: `Review Result: PASS`,\n\
`Review Result: REWORK`, or `Review Result: NEEDS_CONTEXT`. Use `PASS` only when there are no\n\
blocking findings. Use `REWORK` only when confirmed implementation defects require Main Agent\n\
changes. Use `NEEDS_CONTEXT` when missing evidence or ambiguity prevents an independent decision.\n\n\
UAT is Human Review-owned unless this issue explicitly asks the Main Agent to implement a UAT\n\
harness, fixture, rehearsal path, or workflow capability. Missing Human-owned UAT execution is\n\
not a confirmed implementation defect and must not by itself produce `Review Result: REWORK`.\n\
Report UAT readiness or Human Review follow-up separately under `Evidence`.\n\n\
Only use `[Confirmed]`, `[Plausible]`, `[Rejected]`, or `[Needs Context]` for actual review\n\
findings. Do not use those bracketed finding tags for positive verification evidence, checklist\n\
items, or things that were implemented correctly; put positive observations under an `Evidence`\n\
heading with plain bullets instead. Leave routing and evidence persistence to the Shea Symphony\n\
wrapper after this process exits.\n";

pub fn render_assigned_lane_claim_envelope(claim: &LaneClaim) -> String {
    format!(
        "\n\n## Assigned Lane Claim\n\n\
- Preserve this `run=` value in handoff evidence and summaries.\n\
- Run: `{}`\n\
- Claim: `{}`\n\
- Registry pointer: `{}`\n",
        claim.run,
        claim.render(),
        claim.registry
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeConflictRepairEnvelope<'a> {
    pub pr_ref: &'a str,
    pub head_ref_name: &'a str,
    pub expected_base: &'a str,
    pub conflict_summary: &'a str,
    pub mechanical_stderr: &'a str,
}

pub fn render_merge_conflict_repair_task_envelope(
    input: MergeConflictRepairEnvelope<'_>,
) -> String {
    format!(
        "\n\n## Merge-Agent Conflict Repair Task\n\n\
You are repairing the existing approved PR branch in place. Preserve the intent that already passed Agent Review and Human Review. Resolve only conflicts caused by merging the target base into this PR branch. Do not create a replacement PR, do not switch workspaces, and do not route through Rework.\n\n\
- Pull request: `{}`\n\
- Head branch: `{}`\n\
- Expected base: `{}`\n\
- Conflict summary: {}\n\
- Mechanical merge stderr: `{}`\n",
        input.pr_ref,
        input.head_ref_name,
        input.expected_base,
        input.conflict_summary,
        single_line(input.mechanical_stderr)
    )
}

pub const MERGE_REQUIRED_OUTPUT_MARKER_ENVELOPE_TEXT: &str =
    "\n### Required Output Marker\n\n\
End your final response with one of these exact markers:\n\
- `MERGE_AGENT_DECISION: repaired` only if the resolution preserves reviewed intent and verification can proceed.\n\
- `MERGE_AGENT_DECISION: needs_human_input` if there is semantic uncertainty, unrelated drift, unsafe branch/worktree state, or missing verification confidence.\n\n\
Also include `RESOLUTION_SUMMARY:` and `SEMANTIC_SAFETY:` lines. Leave the merge resolution staged or ready for `git add -A`; the merge lane will commit, verify cleanliness, push, and keep the issue in `Merging` for the next tick.\n";

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_current_cli_runtime_envelopes() {
        let ids = RUNTIME_ENVELOPES
            .iter()
            .map(|envelope| envelope.id)
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "assigned_lane_claim",
                "codex_app_server_handoff_boundary",
                "automatic_headless_review_boundary",
                "merge_conflict_repair_task",
                "merge_required_output_markers",
            ]
        );
        assert!(RUNTIME_ENVELOPES
            .iter()
            .all(|envelope| !envelope.path.trim().is_empty()));
    }

    #[test]
    fn merge_repair_envelope_preserves_required_protocol() {
        let envelope = render_merge_conflict_repair_task_envelope(MergeConflictRepairEnvelope {
            pr_ref: "#326",
            head_ref_name: "feature/issue-326",
            expected_base: "main",
            conflict_summary: "src/main.rs conflict",
            mechanical_stderr: "line one\nline two",
        }) + MERGE_REQUIRED_OUTPUT_MARKER_ENVELOPE_TEXT;

        assert!(envelope.contains("Merge-Agent Conflict Repair Task"));
        assert!(envelope.contains("- Pull request: `#326`"));
        assert!(envelope.contains("- Mechanical merge stderr: `line one line two`"));
        assert!(envelope.contains("MERGE_AGENT_DECISION: repaired"));
        assert!(envelope.contains("MERGE_AGENT_DECISION: needs_human_input"));
        assert!(envelope.contains("RESOLUTION_SUMMARY:"));
        assert!(envelope.contains("SEMANTIC_SAFETY:"));
    }

    #[test]
    fn runtime_boundary_headings_stay_in_registry_module() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for path in [
            src.join("lanes/claim.rs"),
            src.join("lanes/main_loop/execution.rs"),
            src.join("lanes/review/automatic.rs"),
            src.join("lanes/merge/repair/agent_contract.rs"),
        ] {
            let content = std::fs::read_to_string(&path).unwrap();
            for heading in [
                "## Assigned Lane Claim",
                "## Codex App-Server Runtime Boundary",
                "## Automatic Headless Review Boundary",
                "## Merge-Agent Conflict Repair Task",
                "### Required Output Marker",
            ] {
                if content.contains(heading) {
                    offenders.push(format!("{} contains {heading}", path.display()));
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "runtime envelope text must live in prompt_runtime.rs: {offenders:?}"
        );
    }
}
