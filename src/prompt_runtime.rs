use crate::{lane_claim::LaneClaim, prompt::STRICT_LIQUID_RENDERER_MODE};

pub const PROMPT_RENDERER_MODE: &str = STRICT_LIQUID_RENDERER_MODE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeEnvelopeSpec {
    pub id: &'static str,
    pub lane: &'static str,
    pub backend: &'static str,
    pub source: &'static str,
    pub purpose: &'static str,
}

pub const RUNTIME_ENVELOPES: &[RuntimeEnvelopeSpec] = &[
    RuntimeEnvelopeSpec {
        id: "assigned_lane_claim",
        lane: "main,merge",
        backend: "all",
        source: "code:claim/render_prompt_with_claim",
        purpose: "typed claim identity and run evidence",
    },
    RuntimeEnvelopeSpec {
        id: "codex_app_server_handoff_boundary",
        lane: "main",
        backend: "codex app-server",
        source: "backend_prompts.codex_app_server",
        purpose: "Markdown-owned app-server child-turn boundary",
    },
    RuntimeEnvelopeSpec {
        id: "automatic_headless_review_boundary",
        lane: "review",
        backend: "configured review backend",
        source: "backend_prompts.automatic_review[_structured]",
        purpose: "Markdown-owned review behavior with code-owned protocol validation",
    },
    RuntimeEnvelopeSpec {
        id: "claude_code_review_boundary",
        lane: "review",
        backend: "claude-code",
        source: "backend_prompts.claude_code_review",
        purpose: "Markdown-owned Claude behavior with code-owned JSON Schema",
    },
    RuntimeEnvelopeSpec {
        id: "merge_conflict_repair_boundary",
        lane: "merge",
        backend: "merge agent",
        source: "backend_prompts.merge_repair",
        purpose: "Markdown behavior rendered with typed conflict facts",
    },
];

pub const CODEX_APP_SERVER_CONTINUE_PROMPT: &str = "Continue";

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

pub fn merge_conflict_repair_values(
    input: MergeConflictRepairEnvelope<'_>,
) -> Vec<(&'static str, String)> {
    vec![
        ("pr_ref", input.pr_ref.into()),
        ("head_ref_name", input.head_ref_name.into()),
        ("expected_base", input.expected_base.into()),
        ("conflict_summary", input.conflict_summary.into()),
        ("mechanical_stderr", single_line(input.mechanical_stderr)),
    ]
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_names_code_and_markdown_runtime_sources() {
        assert_eq!(RUNTIME_ENVELOPES.len(), 5);
        assert!(RUNTIME_ENVELOPES
            .iter()
            .all(|envelope| !envelope.source.trim().is_empty()));
        assert!(RUNTIME_ENVELOPES
            .iter()
            .filter(|envelope| envelope.id != "assigned_lane_claim")
            .all(|envelope| envelope.source.starts_with("backend_prompts.")));
    }

    #[test]
    fn merge_repair_values_keep_dynamic_facts_typed_and_single_line() {
        let values = merge_conflict_repair_values(MergeConflictRepairEnvelope {
            pr_ref: "#326",
            head_ref_name: "feature/issue-326",
            expected_base: "main",
            conflict_summary: "src/main.rs conflict",
            mechanical_stderr: "line one\nline two",
        });
        assert!(values.contains(&("pr_ref", "#326".into())));
        assert!(values.contains(&("mechanical_stderr", "line one line two".into())));
    }

    #[test]
    fn long_backend_behavior_is_not_embedded_in_rust() {
        let source = include_str!("prompt_runtime.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for heading in [
            "Codex App-Server Runtime Boundary",
            "Automatic Headless Review Boundary",
            "Claude Code Structured Review Boundary",
            "Merge-Agent Conflict Repair Boundary",
        ] {
            assert!(
                !source.contains(heading),
                "embedded backend prose: {heading}"
            );
        }
    }
}
