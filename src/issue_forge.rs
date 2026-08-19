use serde::{Deserialize, Serialize};

use crate::issue_templates::{
    load_repository_executable_issue_template, render_executable_issue_template,
    ExecutableIssueTemplate,
};
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
                "docs/milestones/2607-hardening/README.md".into(),
                ".shea/contracts/workflow-capability.v1.md".into(),
            ],
            code_paths: vec![
                "src/main.rs".into(),
                "src/runtime_state.rs".into(),
                "src/orchestrator.rs".into(),
            ],
        },
        IssueForgeSkill {
            key: "tracker".into(),
            label: "Tracker integration".into(),
            description:
                "GitHub Project v2, Linear, status transitions, workpads, and issue creation."
                    .into(),
            knowledge_sources: vec![
                "docs/github-access-policy.md".into(),
                ".shea/contracts/workflow-capability.v1.md".into(),
            ],
            code_paths: vec!["src/tracker.rs".into(), "src/config.rs".into()],
        },
        IssueForgeSkill {
            key: "backend".into(),
            label: "Agent backend".into(),
            description: "Codex, Claude Code, dry-run, and subprocess execution backends.".into(),
            knowledge_sources: vec![
                "docs/codex-app-server-transport.md".into(),
                "docs/claude-code-stream-json.md".into(),
            ],
            code_paths: vec!["src/agent.rs".into(), "src/prompt.rs".into()],
        },
        IssueForgeSkill {
            key: "review".into(),
            label: "Agent Review".into(),
            description:
                "Review agent lifecycle, finding classification, and review-state ownership.".into(),
            knowledge_sources: vec![
                "docs/README.md".into(),
                ".shea/contracts/workflow-capability.v1.md".into(),
            ],
            code_paths: vec!["src/review.rs".into(), "src/main.rs".into()],
        },
        IssueForgeSkill {
            key: "docs".into(),
            label: "Docs and readiness".into(),
            description: "README, dogfood readiness, workflow docs, and operator-facing examples."
                .into(),
            knowledge_sources: vec![
                "docs/README.md".into(),
                ".agents/skills/shea-issue-forge/references/contract.md".into(),
            ],
            code_paths: vec!["README.md".into(), "docs/README.md".into()],
        },
        IssueForgeSkill {
            key: "integration-test".into(),
            label: "Integration test".into(),
            description: "Credential-gated smoke tests, fixtures, and dry-run/live verification."
                .into(),
            knowledge_sources: vec![
                "docs/live-github-smoke-tests.md".into(),
                "docs/README.md".into(),
            ],
            code_paths: vec![
                "tests/fixtures/".into(),
                "tests/".into(),
                "src/tracker.rs".into(),
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
    let template = load_repository_executable_issue_template()
        .expect("repository executable-Issue template must load");
    interactive_forge_with_template(input, &template)
}

/// Build a Forge candidate from an active workflow's selected template.
pub fn interactive_forge_with_template(
    input: InteractiveForgeInput,
    template: &ExecutableIssueTemplate,
) -> InteractiveForgeReport {
    let selected_skill = select_issue_skill(&input.intent, input.skill.as_deref());
    let issue_markdown = issue_markdown_from_interactive_input(&input, &selected_skill, template);
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
        why_it_matters: "Shea Symphony needs this before dispatch so the agent does not invent execution intent.".into(),
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
    let template = load_repository_executable_issue_template()
        .expect("repository executable-Issue template must load");
    repair_markdown_with_template(title, markdown, &template)
}

/// Repair rough input through an active workflow's selected template.
pub fn repair_markdown_with_template(
    title: &str,
    markdown: &str,
    template: &ExecutableIssueTemplate,
) -> RepairReport {
    let validation = validate_markdown(title, markdown);
    let repaired_markdown = repaired_draft(title, markdown, &validation.decision, template);

    RepairReport {
        validation,
        repaired_markdown,
    }
}

pub fn draft_from_template(title: &str, goal: &str) -> String {
    let template = load_repository_executable_issue_template()
        .expect("repository executable-Issue template must load");
    render_executable_issue_template(&template, &draft_values(title, goal, None, false))
        .expect("repository executable-Issue template must render")
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
    template: &ExecutableIssueTemplate,
) -> String {
    let parent_subissue = parent_subissue_batch_signal(input);
    let values = serde_json::json!({
        "uat_required": if parent_subissue { "Yes" } else { "No" },
        "assignee": format_interactive_assignee(input),
        "dependencies": format_interactive_dependencies(input),
        "documentation_impact": "Update tests and documentation that directly describe the accepted capability.",
        "related_context": input.context.as_deref().unwrap_or("None recorded"),
        "parent_subissue": parent_subissue,
        "goal": input.intent.trim(),
        "why_now": "This follow-up is needed to make the requested repository behavior executable.",
        "target_repository": "- `Alive24/shea-symphony`",
        "context": format_interactive_context(input, skill),
        "guardrails": format!("- Keep the implementation within the selected `{}` surface.\n- Keep external mutations behind explicit guarded write authority.", skill.key),
        "in_scope": format!("- Implement the smallest executable slice for: {}.\n- Update focused tests and directly affected documentation.", input.title),
        "out_of_scope": "- Unrelated product or roadmap work.",
        "knowledge_sources": markdown_bullets(&skill.knowledge_sources),
        "code_paths": markdown_bullets(&skill.code_paths),
        "current_state": "Operator intent has been captured by Issue Forge; implementation evidence remains to be gathered.",
        "code_state_freshness": "Refresh the target branch and relevant relationships before dispatch.",
        "deliverable_shape": "A focused code, test, fixture, or documentation change matching the accepted scope.",
        "risks": "- Do not widen scope without a separate follow-up.\n- Preserve guarded mutation and readback boundaries.",
        "expected_outcome": "- [ ] A locally verifiable slice is ready for independent Agent Review.",
        "completion_criteria": "- [ ] The candidate passes the configured deterministic and semantic gate modes.\n- [ ] Implementation and documentation stay within accepted scope.",
        "functional_verification": "- [ ] `cargo test`\n- [ ] `cargo fmt --check`\n- [ ] `cargo clippy --all-targets --all-features -- -D warnings`",
        "uat": "- [ ] Complete operator UAT when the change is observable; otherwise record why it is not required.",
        "context_verification": "- [ ] Confirm current base, native relationships, and relevant recent work before dispatch."
    });
    render_executable_issue_template(template, &values)
        .expect("selected executable-Issue template must render")
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
            "Potential dependency requires operator confirmation: {}",
            dependency_source.trim()
        )
    } else {
        "None identified from supplied intent or context".into()
    }
}

fn parent_subissue_batch_signal(input: &InteractiveForgeInput) -> bool {
    let text = [Some(input.intent.as_str()), input.context.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    contains_any(
        &text,
        &[
            "parent/subissue",
            "parent subissue",
            "parent/sub-issue",
            "parent sub-issue",
            "native subissue",
            "native sub-issue",
            "subissue batch",
            "sub-issue batch",
        ],
    )
}

fn markdown_bullets(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("- `{}`", value.trim().trim_matches('`')))
        .collect::<Vec<_>>()
        .join("\n")
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
        Some(format!("Follow-up: close {capability} readiness gap"))
    } else {
        Some(format!("Follow-up: {capability}"))
    }
}

fn reflective_intent(line: &str) -> String {
    format!(
        "Turn this reflective readiness signal into an executable Shea Symphony issue: {}",
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

fn repaired_draft(
    title: &str,
    markdown: &str,
    decision: &GateDecision,
    template: &ExecutableIssueTemplate,
) -> String {
    let goal = extract_first_sentence(markdown)
        .filter(|value| !value.is_empty())
        .unwrap_or(title);
    let unresolved = if decision.missing.is_empty() {
        "No deterministic input-safety findings remain.".to_string()
    } else {
        format!(
            "Human confirmation remains required for: {}.",
            decision.missing.join(", ")
        )
    };
    let mut values = draft_values(title, goal, Some(markdown), false);
    let object = values
        .as_object_mut()
        .expect("draft values are a JSON object");
    object.insert(
        "current_state".into(),
        format!("Rough candidate input was captured for repair. {unresolved}").into(),
    );
    render_executable_issue_template(template, &values)
        .expect("selected executable-Issue template must render repair")
}

fn draft_values(
    title: &str,
    goal: &str,
    source_context: Option<&str>,
    parent_subissue: bool,
) -> serde_json::Value {
    serde_json::json!({
        "uat_required": if parent_subissue { "Yes" } else { "No" },
        "assignee": "Alive24",
        "dependencies": "None identified; confirm native relationships before dispatch",
        "documentation_impact": "Update documentation that directly describes the accepted behavior, or record why no change is required.",
        "related_context": "None recorded",
        "parent_subissue": parent_subissue,
        "goal": goal,
        "why_now": "This candidate needs an executable repository contract before dispatch.",
        "target_repository": "- `Alive24/shea-symphony`",
        "context": source_context.map(|source| format!("Source input captured by Issue Forge:\n\n```md\n{}\n```", source.trim())).unwrap_or_else(|| "No additional context was supplied.".into()),
        "guardrails": "- Keep implementation within the accepted issue contract.\n- Preserve guarded writes and targeted readback.",
        "in_scope": format!("- Implement the smallest executable slice for: {title}."),
        "out_of_scope": "- Unrelated product or roadmap work.",
        "knowledge_sources": "- `.agents/skills/shea-issue-forge/references/contract.md`",
        "code_paths": "- Confirm focused paths from current repository evidence before dispatch.",
        "current_state": "Issue Forge produced this candidate from currently supplied input.",
        "code_state_freshness": "Refresh the target base and relevant relationships before dispatch.",
        "deliverable_shape": "A focused code, test, documentation, or tracker contract change matching accepted scope.",
        "risks": "- Do not invent missing product decisions.\n- Create separate follow-ups for broader work.",
        "expected_outcome": "- [ ] The accepted issue can be implemented and independently verified.",
        "completion_criteria": "- [ ] The configured deterministic and semantic gates accept the final candidate.",
        "functional_verification": "- [ ] Run repository-owned verification for the accepted surface.",
        "uat": "- [ ] Complete operator UAT when observable; otherwise record why it is not required.",
        "context_verification": "- [ ] Recheck current base, native relationships, and recent relevant work."
    })
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
    fn deterministic_validation_keeps_semantic_policy_out_of_rust() {
        let report = validate_markdown("Thin issue", "make forge better");

        assert_eq!(report.decision.kind, GateDecisionKind::Ready);
        assert!(report.question.is_none());
    }

    #[test]
    fn repairs_thin_markdown_into_gate_dispatchable_contract() {
        let report = repair_markdown("Implement Forge", "make forge better");
        let validation = validate_markdown("Implement Forge", &report.repaired_markdown);

        assert!(report.repaired_markdown.contains("## Issue Goal"));
        assert!(report.repaired_markdown.contains("Documentation Impact"));
        assert!(!report.repaired_markdown.contains("docs/bootstrap/"));
        assert!(validation.decision.is_dispatchable());
    }

    #[test]
    fn issue_skill_registry_uses_current_context_sources() {
        let sources = issue_skill_registry()
            .into_iter()
            .flat_map(|skill| skill.knowledge_sources)
            .collect::<Vec<_>>();

        assert!(sources
            .iter()
            .all(|source| !source.contains("docs/bootstrap/")));
        assert!(sources.iter().any(|source| source == "docs/README.md"));
        assert!(sources
            .iter()
            .any(|source| source == ".shea/contracts/workflow-capability.v1.md"));
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
            intent: "main loop should inspect runtime state before claiming new work".into(),
            skill: Some("runtime".into()),
            context: None,
            assignees: vec!["Alive24".into()],
        });

        assert_eq!(report.selected_skill.key, "runtime");
        assert!(report.validation.decision.is_dispatchable());
        assert!(report.issue_markdown.contains("## Issue Goal"));
        assert!(report.issue_markdown.contains("- Assignee: Alive24"));
        assert!(report.issue_markdown.contains("- Dependencies:"));
        assert!(report.issue_markdown.contains("src/runtime_state.rs"));
        assert!(report.question.is_none());
    }

    #[test]
    fn interactive_forge_records_parent_owned_subissue_review_contract() {
        let report = interactive_forge(InteractiveForgeInput {
            title: "Split tracker hardening into native subissue batch".into(),
            intent: "create a parent/subissue batch for tracker hardening slices".into(),
            skill: Some("tracker".into()),
            context: None,
            assignees: vec!["Alive24".into()],
        });

        assert!(report.validation.decision.is_dispatchable());
        assert!(report
            .issue_markdown
            .contains("the parent owns final Human Review and UAT"));
        assert!(report
            .issue_markdown
            .contains("the parent owns final Human Review and UAT"));
        assert!(report
            .issue_markdown
            .contains("route from independent Agent Review to Merging"));
        assert!(report
            .issue_markdown
            .contains("Subissue Human Review Exception: <reason>"));
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
            GateDecisionKind::Ready
        );
    }

    #[test]
    fn reflective_candidate_preserves_parent_subissue_contract_defaults() {
        let candidates = reflective_candidates_from_context(
            "- Follow-up: create a native subissue batch for review-loop repair.",
            None,
            1,
        );

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0]
            .issue_markdown
            .contains("the parent owns final Human Review and UAT"));
        assert!(candidates[0]
            .issue_markdown
            .contains("routine native subissues route from independent Agent Review to Merging"));
    }
}
