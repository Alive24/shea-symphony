use serde::{Deserialize, Serialize};

use crate::model::{GateDecision, GateDecisionKind, TrackerIssue};
use crate::quality_gate::evaluate_issue;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IssueForgeMode {
    Discover,
    Discuss,
    Draft,
    Validate,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateClassification {
    Ready,
    ReadyWithAssumptions,
    NeedToClarify,
    TooBroad,
    Blocked,
    DuplicateAlreadyCovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueCandidate {
    pub title: String,
    pub classification: CandidateClassification,
    pub rationale: String,
    #[serde(default)]
    pub follow_up_candidates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClarificationQuestion {
    pub question: String,
    pub why_it_matters: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForgeValidationReport {
    pub title: String,
    pub decision: GateDecision,
    pub question: Option<ClarificationQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairReport {
    pub validation: ForgeValidationReport,
    pub repaired_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueForgeSkill {
    pub key: String,
    pub label: String,
    pub description: String,
    pub knowledge_sources: Vec<String>,
    pub code_paths: Vec<String>,
    pub guardrails: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractiveForgeInput {
    pub title: String,
    pub intent: String,
    pub skill: Option<String>,
    pub context: Option<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractiveForgeReport {
    pub selected_skill: IssueForgeSkill,
    pub issue_markdown: String,
    pub validation: ForgeValidationReport,
    pub question: Option<ClarificationQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectiveForgeCandidate {
    pub title: String,
    pub skill: IssueForgeSkill,
    pub rationale: String,
    pub issue_markdown: String,
    pub validation: ForgeValidationReport,
}

pub fn discover_candidates(intent: &str) -> Vec<IssueCandidate> {
    let title = title_from_intent(intent);
    let classification = if intent.split_whitespace().count() >= 5 {
        CandidateClassification::ReadyWithAssumptions
    } else {
        CandidateClassification::NeedToClarify
    };

    vec![IssueCandidate {
        title,
        classification,
        rationale: "Derived from local operator intent; validate or repair before dispatch.".into(),
        follow_up_candidates: Vec::new(),
    }]
}

pub fn issue_skill_registry() -> Vec<IssueForgeSkill> {
    vec![
        IssueForgeSkill {
            key: "runtime".into(),
            label: "Runtime loop".into(),
            description: "Run-loop, runtime state, polling, resume, and orchestration control."
                .into(),
            knowledge_sources: vec![
                "docs/bootstrap/JADE_SYMPHONY_SPEC.md".into(),
                "docs/bootstrap/JADE_WORKFLOW.md".into(),
            ],
            code_paths: vec![
                "src/main.rs".into(),
                "src/runtime_state.rs".into(),
                "src/orchestrator.rs".into(),
            ],
            guardrails: vec![
                "Main implementation work must stop at Agent Review.".into(),
                "Preserve dry-run behavior and bounded-loop controls.".into(),
            ],
        },
        IssueForgeSkill {
            key: "tracker".into(),
            label: "Tracker integration".into(),
            description:
                "GitHub Project v2, Linear, status transitions, workpads, and issue creation."
                    .into(),
            knowledge_sources: vec![
                "docs/bootstrap/TRACKER_GITHUB_PROJECT_V2.md".into(),
                "docs/bootstrap/JADE_WORKFLOW.md".into(),
            ],
            code_paths: vec!["src/tracker.rs".into(), "src/config.rs".into()],
            guardrails: vec![
                "Keep GitHub Project v2 and Linear behind the normalized tracker adapter.".into(),
                "Do not mutate tracker state without explicit --write.".into(),
            ],
        },
        IssueForgeSkill {
            key: "backend".into(),
            label: "Agent backend".into(),
            description: "Codex, Claude Code, dry-run, and subprocess execution backends.".into(),
            knowledge_sources: vec![
                "docs/bootstrap/JADE_SYMPHONY_SPEC.md".into(),
                "docs/bootstrap/references/openai-symphony/SPEC.md".into(),
            ],
            code_paths: vec!["src/agent.rs".into(), "src/prompt.rs".into()],
            guardrails: vec![
                "Keep Codex and Claude Code behind the normalized backend boundary.".into(),
                "Keep dry-run backend safe for tests.".into(),
            ],
        },
        IssueForgeSkill {
            key: "review".into(),
            label: "Agent Review".into(),
            description:
                "Review agent lifecycle, finding classification, and review-state ownership.".into(),
            knowledge_sources: vec![
                "docs/bootstrap/JADE_WORKFLOW.md".into(),
                "docs/bootstrap/JADE_SYMPHONY_SPEC.md".into(),
            ],
            code_paths: vec!["src/review.rs".into(), "src/main.rs".into()],
            guardrails: vec![
                "Main implementation agent must never set Human Review.".into(),
                "Failed, unavailable, or inconclusive review must not advance to Human Review."
                    .into(),
            ],
        },
        IssueForgeSkill {
            key: "docs".into(),
            label: "Docs and readiness".into(),
            description: "README, dogfood readiness, workflow docs, and operator-facing examples."
                .into(),
            knowledge_sources: vec![
                "README.md".into(),
                "docs/dogfood-readiness.md".into(),
                "docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md".into(),
            ],
            code_paths: vec!["README.md".into(), "docs/dogfood-readiness.md".into()],
            guardrails: vec![
                "Keep docs honest about dry-run, stubbed, and live behavior.".into(),
                "Do not claim full autonomous orchestration before controlled live proof.".into(),
            ],
        },
        IssueForgeSkill {
            key: "integration-test".into(),
            label: "Integration test".into(),
            description: "Credential-gated smoke tests, fixtures, and dry-run/live verification."
                .into(),
            knowledge_sources: vec![
                "docs/bootstrap/JADE_SYMPHONY_SPEC.md".into(),
                "docs/dogfood-readiness.md".into(),
            ],
            code_paths: vec!["examples/".into(), "tests/".into(), "src/tracker.rs".into()],
            guardrails: vec![
                "Keep live tests credential-gated and safe to skip locally.".into(),
                "Preserve fixture-backed dry-run coverage.".into(),
            ],
        },
    ]
}

pub fn find_issue_skill(key: &str) -> Option<IssueForgeSkill> {
    let normalized = key.trim().to_lowercase();
    issue_skill_registry()
        .into_iter()
        .find(|skill| skill.key == normalized)
}

pub fn select_issue_skill(intent: &str, explicit: Option<&str>) -> IssueForgeSkill {
    if let Some(skill) = explicit.and_then(find_issue_skill) {
        return skill;
    }

    let text = intent.to_lowercase();
    let selected = if contains_any(
        &text,
        &[
            "github", "project", "tracker", "linear", "status", "workpad",
        ],
    ) {
        "tracker"
    } else if contains_any(
        &text,
        &[
            "backend",
            "codex",
            "claude",
            "subprocess",
            "agent execution",
        ],
    ) {
        "backend"
    } else if contains_any(
        &text,
        &[
            "review",
            "gemini",
            "finding",
            "human review",
            "agent review",
        ],
    ) {
        "review"
    } else if contains_any(&text, &["readme", "docs", "documentation", "dogfood"]) {
        "docs"
    } else if contains_any(&text, &["test", "fixture", "smoke", "uat", "verification"]) {
        "integration-test"
    } else {
        "runtime"
    };

    find_issue_skill(selected).expect("built-in issue forge skill exists")
}

pub fn conversational_title_from_intent(intent: &str) -> String {
    title_from_intent(intent)
}

pub fn interactive_forge(input: InteractiveForgeInput) -> InteractiveForgeReport {
    let selected_skill = select_issue_skill(&input.intent, input.skill.as_deref());
    let issue_markdown = issue_markdown_from_interactive_input(&input, &selected_skill);
    let validation = validate_markdown(&input.title, &issue_markdown);
    let question = focused_interactive_question(&input, &selected_skill, &validation);

    InteractiveForgeReport {
        selected_skill,
        issue_markdown,
        validation,
        question,
    }
}

pub fn reflective_candidates_from_context(
    context: &str,
    requested_skill: Option<&str>,
    limit: usize,
) -> Vec<ReflectiveForgeCandidate> {
    context
        .lines()
        .map(str::trim)
        .filter(|line| reflective_signal(line))
        .take(limit)
        .enumerate()
        .map(|(index, line)| {
            let title = reflective_title(line, index + 1);
            let intent = reflective_intent(line);
            let skill = select_issue_skill(&format!("{title} {intent}"), requested_skill);
            let input = InteractiveForgeInput {
                title: title.clone(),
                intent,
                skill: Some(skill.key.clone()),
                context: Some(line.to_string()),
                assignees: Vec::new(),
            };
            let report = interactive_forge(input);
            ReflectiveForgeCandidate {
                title,
                skill,
                rationale: "Derived from a conservative follow-up signal in the supplied context."
                    .into(),
                issue_markdown: report.issue_markdown,
                validation: report.validation,
            }
        })
        .collect()
}

pub fn next_clarification_question(decision: &GateDecision) -> Option<ClarificationQuestion> {
    if !matches!(
        decision.kind,
        GateDecisionKind::NeedToClarify | GateDecisionKind::TooBroad | GateDecisionKind::Blocked
    ) {
        return None;
    }

    decision.missing.first().map(|missing| ClarificationQuestion {
        question: format!("What is the smallest concrete detail that resolves `{missing}` for this issue?"),
        why_it_matters: "Jade Symphony needs this before dispatch so the agent does not invent execution intent.".into(),
    })
}

pub fn validate_markdown(title: &str, markdown: &str) -> ForgeValidationReport {
    let issue = forge_issue(title, markdown);
    let decision = evaluate_issue(&issue);
    let question = next_clarification_question(&decision);
    ForgeValidationReport {
        title: title.to_string(),
        decision,
        question,
    }
}

pub fn repair_markdown(title: &str, markdown: &str) -> RepairReport {
    let validation = validate_markdown(title, markdown);
    let repaired_markdown = if validation.decision.is_dispatchable() {
        record_gate_notes(markdown, &validation.decision)
    } else {
        repaired_draft(title, markdown, &validation.decision)
    };

    RepairReport {
        validation,
        repaired_markdown,
    }
}

pub fn draft_from_template(title: &str, goal: &str) -> String {
    format!(
        r#"## Issue Setup

- UAT Required: No
- Related Parent Issue or Context: 

## Issue Goal

{goal}

## Why Now

TBD

## Issue Context

TBD

## Decisions / Assumptions

### Decisions

- TBD

### Assumptions

- TBD

## Dependencies

- No blocking dependencies identified yet.

## Non-Negotiable Guardrails

- Keep Jade Symphony orchestration infrastructure separate from downstream product business logic.

## Scope

### In Scope

- {title}

### Out of Scope

- Unrelated product business logic.

## Canonical References

### Target Repository / Package

- TBD

### Relevant Knowledge Sources

- TBD

### Relevant Code Paths

- TBD

## Current State

TBD

## Deliverable Shape

TBD

## Risks or Constraints

- TBD

## Expected Outcome

TBD

## Verification

### Completion Criteria

- TBD

### Functional Verification

- TBD

### UAT

- Not required unless the issue becomes operator-observable.

### Context Verification

- Confirm the issue still matches canonical sources.
"#
    )
}

fn title_from_intent(intent: &str) -> String {
    let trimmed = intent.trim();
    if trimmed.is_empty() {
        return "Clarify issue intent".into();
    }

    let words = trimmed
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ");
    format!("Issue candidate: {words}")
}

fn issue_markdown_from_interactive_input(
    input: &InteractiveForgeInput,
    skill: &IssueForgeSkill,
) -> String {
    let mut draft = draft_from_template(&input.title, input.intent.trim());
    draft = draft.replace(
        "- UAT Required: No",
        &format!(
            "- UAT Required: No\n- Assignee: {}",
            format_interactive_assignee(input)
        ),
    );
    draft = draft.replace(
        "## Why Now\n\nTBD",
        "## Why Now\n\nThis follow-up is needed to continue turning Jade Symphony from a dry-run skeleton into a usable orchestration binary.",
    );
    draft = draft.replace(
        "## Issue Context\n\nTBD",
        &format_interactive_context(input, skill),
    );
    draft = draft.replace(
        "### Decisions\n\n- TBD",
        &format!(
            "### Decisions\n\n- Use the `{}` Issue Forge skill as the starting contract shape.\n- Keep the implementation focused to this issue and create follow-ups for broader work.",
            skill.key
        ),
    );
    draft = draft.replace(
        "### Assumptions\n\n- TBD",
        "### Assumptions\n\n- The existing Issue Quality Gate remains the acceptance boundary for generated tracker issues.",
    );
    draft = draft.replace(
        "## Dependencies\n\n- No blocking dependencies identified yet.",
        &format_interactive_dependencies(input),
    );
    draft = draft.replace(
        "## Non-Negotiable Guardrails\n\n- Keep Jade Symphony orchestration infrastructure separate from downstream product business logic.",
        &format!(
            "## Non-Negotiable Guardrails\n\n- Keep Jade Symphony orchestration infrastructure separate from downstream product business logic.\n{}",
            skill
                .guardrails
                .iter()
                .map(|guardrail| format!("- {guardrail}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    draft = draft.replace(
        &format!("### In Scope\n\n- {}", input.title),
        &format!(
            "### In Scope\n\n- Implement the smallest executable slice for: {}.\n- Update tests and docs that directly describe this capability.",
            input.title
        ),
    );
    draft = draft.replace(
        "### Target Repository / Package\n\n- TBD",
        "### Target Repository / Package\n\n- Alive24/jade-symphony",
    );
    draft = draft.replace(
        "### Relevant Knowledge Sources\n\n- TBD",
        &format!(
            "### Relevant Knowledge Sources\n\n{}",
            skill
                .knowledge_sources
                .iter()
                .map(|source| format!("- {source}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    draft = draft.replace(
        "### Relevant Code Paths\n\n- TBD",
        &format!(
            "### Relevant Code Paths\n\n{}",
            skill
                .code_paths
                .iter()
                .map(|path| format!("- {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    draft = draft.replace(
        "## Current State\n\nTBD",
        "## Current State\n\nOperator intent has been captured by Issue Forge and shaped into a quality-gated issue contract.",
    );
    draft = draft.replace(
        "## Deliverable Shape\n\nTBD",
        "## Deliverable Shape\n\nA focused code, test, fixture, or documentation change matching the selected Issue Forge skill.",
    );
    draft = draft.replace(
        "## Risks or Constraints\n\n- TBD",
        "## Risks or Constraints\n\n- Do not expand this issue into unrelated roadmap or product work.\n- Keep live external mutations behind explicit `--write` flags.",
    );
    draft = draft.replace(
        "## Expected Outcome\n\nTBD",
        "## Expected Outcome\n\nA locally verifiable slice that can be handed from main implementation to Agent Review.",
    );
    draft = draft.replace(
        "### Completion Criteria\n\n- TBD",
        "### Completion Criteria\n\n- The issue contract passes `jade-symphony forge-validate`.\n- Implementation and documentation are limited to the issue scope.",
    );
    draft = draft.replace(
        "### Functional Verification\n\n- TBD",
        "### Functional Verification\n\n- Run `cargo test`.\n- Run `cargo fmt --check`.\n- Run `cargo clippy --all-targets --all-features -- -D warnings`.",
    );
    draft = draft.replace(
        "### Context Verification\n\n- Confirm the issue still matches canonical sources.",
        "### Context Verification\n\n- Confirm the issue still matches canonical sources and Project #9 state before dispatch.",
    );
    draft
}

fn format_interactive_assignee(input: &InteractiveForgeInput) -> String {
    input
        .assignees
        .first()
        .map(|assignee| assignee.trim().trim_start_matches('@').to_string())
        .filter(|assignee| !assignee.is_empty())
        .unwrap_or_else(|| "Alive24".into())
}

fn format_interactive_context(input: &InteractiveForgeInput, skill: &IssueForgeSkill) -> String {
    let mut lines = vec![
        "## Issue Context".to_string(),
        String::new(),
        format!(
            "- Selected Issue Forge skill: `{}` ({})",
            skill.key, skill.label
        ),
        format!("- Operator intent: {}", input.intent.trim()),
    ];
    if let Some(context) = input
        .context
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push("- Supplied context:".into());
        lines.push(String::new());
        lines.push("```md".into());
        lines.push(context.trim().into());
        lines.push("```".into());
    }
    lines.join("\n")
}

fn format_interactive_dependencies(input: &InteractiveForgeInput) -> String {
    let dependency_source = [Some(input.intent.as_str()), input.context.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if dependency_signal(&dependency_source) {
        format!(
            "## Dependencies\n\n- Potential dependency requires operator confirmation: {}",
            dependency_source.trim()
        )
    } else {
        "## Dependencies\n\n- No blocking dependencies identified by Issue Forge from the supplied intent or context."
            .into()
    }
}

fn focused_interactive_question(
    input: &InteractiveForgeInput,
    skill: &IssueForgeSkill,
    validation: &ForgeValidationReport,
) -> Option<ClarificationQuestion> {
    if let Some(question) = next_clarification_question(&validation.decision) {
        return Some(question);
    }

    if input.intent.split_whitespace().count() < 5 {
        return Some(ClarificationQuestion {
            question: format!(
                "What concrete behavior should the `{}` issue change first?",
                skill.key
            ),
            why_it_matters:
                "The generated issue is structurally valid, but the operator intent is still thin."
                    .into(),
        });
    }

    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn reflective_signal(line: &str) -> bool {
    let normalized = line.to_lowercase();
    contains_any(
        &normalized,
        &[
            "not implemented",
            "follow-up",
            "todo",
            "missing",
            "must exist",
            "next issue",
            "recommended next",
        ],
    )
}

fn dependency_signal(text: &str) -> bool {
    let normalized = text.to_lowercase();
    contains_any(
        &normalized,
        &[
            "depends on",
            "dependency",
            "dependencies",
            "blocked by",
            "overlap",
            "overlapping",
            "supersede",
            "parallel-safe with",
            "parallel safe with",
        ],
    )
}

fn reflective_title(line: &str, index: usize) -> String {
    if let Some(title) = reflective_table_title(line) {
        return title;
    }

    let cleaned = line
        .trim_start_matches(|ch: char| {
            ch == '-' || ch == '*' || ch == '#' || ch.is_ascii_digit() || ch == '.'
        })
        .trim();
    let words = cleaned
        .split_whitespace()
        .take(9)
        .collect::<Vec<_>>()
        .join(" ");
    if words.is_empty() {
        format!("Reflective follow-up {index}")
    } else {
        format!("Follow-up: {words}")
    }
}

fn reflective_table_title(line: &str) -> Option<String> {
    if !line.contains('|') {
        return None;
    }

    let cells = line
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>();
    let capability = cells.first()?;
    let status = cells.get(1).copied().unwrap_or_default().to_lowercase();
    if status.contains("not implemented") || status.contains("remaining work") {
        Some(format!("Follow-up: close {} readiness gap", capability))
    } else {
        Some(format!("Follow-up: {}", capability))
    }
}

fn reflective_intent(line: &str) -> String {
    format!(
        "Turn this reflective readiness signal into an executable Jade Symphony issue: {}",
        line.trim()
    )
}

fn forge_issue(title: &str, markdown: &str) -> TrackerIssue {
    TrackerIssue {
        tracker_kind: "issue_forge".into(),
        id: "forge-local".into(),
        item_id: None,
        identifier: "forge-local".into(),
        title: title.to_string(),
        description: Some(markdown.to_string()),
        url: None,
        state: "Todo".into(),
        labels: Vec::new(),
        assignees: Vec::new(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: Vec::new(),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    }
}

fn record_gate_notes(markdown: &str, decision: &GateDecision) -> String {
    let mut repaired = markdown.trim_end().to_string();
    repaired.push_str("\n\n## Issue Forge Gate Notes\n\n");
    repaired.push_str(&format!("- Gate Decision: {:?}\n", decision.kind));
    if decision.assumptions.is_empty() {
        repaired.push_str("- Assumptions: None recorded.\n");
    } else {
        for assumption in &decision.assumptions {
            repaired.push_str(&format!("- Assumption: {assumption}\n"));
        }
    }
    repaired
}

fn repaired_draft(title: &str, markdown: &str, decision: &GateDecision) -> String {
    let goal = extract_first_sentence(markdown)
        .filter(|value| !value.is_empty())
        .unwrap_or(title);
    let mut draft = draft_from_template(title, goal);
    draft = draft.replace(
        "## Why Now\n\nTBD",
        "## Why Now\n\nThis issue needs clarification or structure before it can be safely dispatched.",
    );
    draft = draft.replace(
        "## Issue Context\n\nTBD",
        &format!(
            "## Issue Context\n\nSource input captured by Issue Forge:\n\n```md\n{}\n```",
            markdown.trim()
        ),
    );
    draft = draft.replace(
        "### Decisions\n\n- TBD",
        "### Decisions\n\n- Use Issue Forge repair to convert rough input into the Jade quality template.",
    );
    let assumptions = if decision.missing.is_empty() {
        "- Existing source input is sufficient for an initial executable draft.".to_string()
    } else {
        format!(
            "- Repaired draft still needs human confirmation for: {}.",
            decision.missing.join(", ")
        )
    };
    draft = draft.replace(
        "### Assumptions\n\n- TBD",
        &format!("### Assumptions\n\n{assumptions}"),
    );
    draft = draft.replace(
        "### Target Repository / Package\n\n- TBD",
        "### Target Repository / Package\n\n- Alive24/jade-symphony",
    );
    draft = draft.replace(
        "### Relevant Knowledge Sources\n\n- TBD",
        "### Relevant Knowledge Sources\n\n- docs/bootstrap/JADE_SYMPHONY_SPEC.md\n- docs/bootstrap/JADE_WORKFLOW.md\n- docs/bootstrap/ISSUE_QUALITY_GATE_TEMPLATE.md",
    );
    draft = draft.replace(
        "### Relevant Code Paths\n\n- TBD",
        "### Relevant Code Paths\n\n- TBD by implementer after source scan.",
    );
    draft = draft.replace(
        "## Current State\n\nTBD",
        "## Current State\n\nRough issue input exists and has been repaired into the Jade issue contract shape.",
    );
    draft = draft.replace(
        "## Deliverable Shape\n\nTBD",
        "## Deliverable Shape\n\nCode, docs, issue update, or validation artifact as appropriate for the final accepted scope.",
    );
    draft = draft.replace(
        "## Risks or Constraints\n\n- TBD",
        "## Risks or Constraints\n\n- Do not expand scope beyond the repaired issue contract without creating follow-up candidates.",
    );
    draft = draft.replace(
        "## Expected Outcome\n\nTBD",
        "## Expected Outcome\n\nAn executable issue contract that can pass the Issue Quality Gate before dispatch.",
    );
    draft = draft.replace(
        "### Completion Criteria\n\n- TBD",
        "### Completion Criteria\n\n- Issue contract has goal, scope, guardrails, references, and validation requirements.",
    );
    draft = draft.replace(
        "### Functional Verification\n\n- TBD",
        "### Functional Verification\n\n- Run `jade-symphony forge-validate` on the repaired draft.",
    );
    draft
}

fn extract_first_sentence(markdown: &str) -> Option<&str> {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GateDecision, GateDecisionKind};

    #[test]
    fn asks_one_missing_item() {
        let question = next_clarification_question(&GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec!["goal".into(), "scope".into()],
            assumptions: vec![],
            notes: vec![],
        })
        .unwrap();

        assert!(question.question.contains("goal"));
        assert!(!question.question.contains("scope"));
    }

    #[test]
    fn validates_thin_markdown_with_actionable_question() {
        let report = validate_markdown("Thin issue", "make forge better");

        assert_eq!(report.decision.kind, GateDecisionKind::NeedToClarify);
        assert!(report.question.unwrap().question.contains("goal"));
    }

    #[test]
    fn repairs_thin_markdown_into_gate_dispatchable_contract() {
        let report = repair_markdown("Implement Forge", "make forge better");
        let validation = validate_markdown("Implement Forge", &report.repaired_markdown);

        assert!(report
            .repaired_markdown
            .contains("## Decisions / Assumptions"));
        assert!(report.repaired_markdown.contains("## Issue Goal"));
        assert!(validation.decision.is_dispatchable());
    }

    #[test]
    fn discover_returns_one_local_candidate_without_gsd_terms() {
        let candidates = discover_candidates("add issue forge validate and repair commands");

        assert_eq!(candidates.len(), 1);
        assert!(matches!(
            candidates[0].classification,
            CandidateClassification::ReadyWithAssumptions
        ));
        assert!(!candidates[0].title.to_lowercase().contains("gsd"));
    }

    #[test]
    fn selects_skill_from_explicit_key_or_intent() {
        assert_eq!(select_issue_skill("anything", Some("review")).key, "review");
        assert_eq!(
            select_issue_skill("fix GitHub Project status writes", None).key,
            "tracker"
        );
        assert_eq!(
            select_issue_skill("update dogfood readiness docs", None).key,
            "docs"
        );
    }

    #[test]
    fn interactive_forge_builds_quality_gated_contract() {
        let report = interactive_forge(InteractiveForgeInput {
            title: "Add resume preflight".into(),
            intent: "run-loop should inspect runtime state before claiming new work".into(),
            skill: Some("runtime".into()),
            context: None,
            assignees: vec!["Alive24".into()],
        });

        assert_eq!(report.selected_skill.key, "runtime");
        assert!(report.validation.decision.is_dispatchable());
        assert!(report.issue_markdown.contains("## Issue Goal"));
        assert!(report.issue_markdown.contains("- Assignee: Alive24"));
        assert!(report.issue_markdown.contains("## Dependencies"));
        assert!(report.issue_markdown.contains("src/runtime_state.rs"));
        assert!(report.question.is_none());
    }

    #[test]
    fn conversational_title_is_inferred_from_operator_intent() {
        let title = conversational_title_from_intent(
            "make forge interactive accept natural language issue intent",
        );

        assert_eq!(
            title,
            "Issue candidate: make forge interactive accept natural language issue intent"
        );
    }

    #[test]
    fn interactive_forge_asks_focused_question_for_thin_intent() {
        let report = interactive_forge(InteractiveForgeInput {
            title: "Improve tracker".into(),
            intent: "fix tracker".into(),
            skill: Some("tracker".into()),
            context: None,
            assignees: Vec::new(),
        });

        assert!(report.validation.decision.is_dispatchable());
        assert!(report.question.unwrap().question.contains("tracker"));
    }

    #[test]
    fn reflective_candidates_are_conservative_and_quality_gated() {
        let candidates = reflective_candidates_from_context(
            "- Not implemented yet: live PR creation.\n- Already done: fixture planning.",
            None,
            3,
        );

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].validation.decision.is_dispatchable());
        assert!(candidates[0].title.contains("live PR creation"));
    }

    #[test]
    fn reflective_candidate_surfaces_potential_dependency_for_confirmation() {
        let candidates = reflective_candidates_from_context(
            "- Follow-up: add merge retry after #164 is done; overlaps issue #161.",
            None,
            1,
        );

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0]
            .issue_markdown
            .contains("Potential dependency requires operator confirmation"));
        assert_eq!(
            candidates[0].validation.decision.kind,
            GateDecisionKind::NeedToClarify
        );
    }
}
