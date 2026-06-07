use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::prompt::{render_template_with_values, smoke_render_template, PromptError};
use crate::workflow::WorkflowDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkpadTemplateId {
    MainHandoff,
    MainHandoffFailure,
    MainAssigneeOwnership,
    MainQualityGate,
    MainRuntimeOwnership,
    MainUsageLimitPause,
    ParentTopology,
    WorkspaceAdoption,
    WorkspaceEnsure,
    AgentReviewRun,
    AgentReviewHandoff,
    RepeatedReviewFailure,
    ManualReview,
    MergeRun,
    MergeRepair,
    DoctorTriage,
    HumanReviewRepair,
    ForgeReworkRun,
    ForgeReworkBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkpadTemplate {
    pub id: WorkpadTemplateId,
    pub body: String,
    pub source: WorkpadTemplateSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkpadTemplateSource {
    WorkflowFile(PathBuf),
    CentralizedFallback,
    MissingOrInvalid { path: PathBuf, error: String },
}

impl WorkpadTemplateId {
    pub fn key(self) -> &'static str {
        match self {
            Self::MainHandoff => "main_handoff",
            Self::MainHandoffFailure => "main_handoff_failure",
            Self::MainAssigneeOwnership => "main_assignee_ownership",
            Self::MainQualityGate => "main_quality_gate",
            Self::MainRuntimeOwnership => "main_runtime_ownership",
            Self::MainUsageLimitPause => "main_usage_limit_pause",
            Self::ParentTopology => "parent_topology",
            Self::WorkspaceAdoption => "workspace_adoption",
            Self::WorkspaceEnsure => "workspace_ensure",
            Self::AgentReviewRun => "agent_review_run",
            Self::AgentReviewHandoff => "agent_review_handoff",
            Self::RepeatedReviewFailure => "repeated_review_failure",
            Self::ManualReview => "manual_review",
            Self::MergeRun => "merge_run",
            Self::MergeRepair => "merge_repair",
            Self::DoctorTriage => "doctor_triage",
            Self::HumanReviewRepair => "human_review_repair",
            Self::ForgeReworkRun => "forge_rework_run",
            Self::ForgeReworkBlocked => "forge_rework_blocked",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::MainHandoff,
            Self::MainHandoffFailure,
            Self::MainAssigneeOwnership,
            Self::MainQualityGate,
            Self::MainRuntimeOwnership,
            Self::MainUsageLimitPause,
            Self::ParentTopology,
            Self::WorkspaceAdoption,
            Self::WorkspaceEnsure,
            Self::AgentReviewRun,
            Self::AgentReviewHandoff,
            Self::RepeatedReviewFailure,
            Self::ManualReview,
            Self::MergeRun,
            Self::MergeRepair,
            Self::DoctorTriage,
            Self::HumanReviewRepair,
            Self::ForgeReworkRun,
            Self::ForgeReworkBlocked,
        ]
    }
}

impl WorkpadTemplateSource {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::WorkflowFile(_) => "workflow_template_file",
            Self::CentralizedFallback => "centralized_fallback_template",
            Self::MissingOrInvalid { .. } => "missing_or_invalid_template",
        }
    }

    pub fn path_display(&self) -> String {
        match self {
            Self::WorkflowFile(path) | Self::MissingOrInvalid { path, .. } => {
                path.display().to_string()
            }
            Self::CentralizedFallback => "<centralized fallback>".into(),
        }
    }

    pub fn diagnostic(&self) -> Option<&str> {
        match self {
            Self::MissingOrInvalid { error, .. } => Some(error),
            _ => None,
        }
    }
}

pub fn workpad_template_for(
    workflow: Option<&WorkflowDefinition>,
    id: WorkpadTemplateId,
) -> WorkpadTemplate {
    if let Some(workflow) = workflow {
        if let Some(path) = configured_template_path(workflow, id) {
            return match fs::read_to_string(&path) {
                Ok(body) if !body.trim().is_empty() => WorkpadTemplate {
                    id,
                    body: body.trim().to_string(),
                    source: WorkpadTemplateSource::WorkflowFile(path),
                },
                Ok(_) => WorkpadTemplate {
                    id,
                    body: fallback_template(id).to_string(),
                    source: WorkpadTemplateSource::MissingOrInvalid {
                        path,
                        error: "template file is empty; fallback will be used".into(),
                    },
                },
                Err(error) => WorkpadTemplate {
                    id,
                    body: fallback_template(id).to_string(),
                    source: WorkpadTemplateSource::MissingOrInvalid {
                        path,
                        error: error.to_string(),
                    },
                },
            };
        }
    }

    WorkpadTemplate {
        id,
        body: fallback_template(id).to_string(),
        source: WorkpadTemplateSource::CentralizedFallback,
    }
}

pub fn workpad_template_readback(workflow: &WorkflowDefinition) -> Vec<WorkpadTemplate> {
    WorkpadTemplateId::all()
        .iter()
        .map(|id| workpad_template_for(Some(workflow), *id))
        .collect()
}

pub fn render_workpad_template(
    workflow: Option<&WorkflowDefinition>,
    id: WorkpadTemplateId,
    values: &[(&str, String)],
) -> Result<String, PromptError> {
    render_template_with_values(&workpad_template_for(workflow, id).body, values)
}

pub fn smoke_render_workpad_template(
    template: &WorkpadTemplate,
    values: &[(&str, String)],
) -> Result<(), PromptError> {
    smoke_render_template(&template.body, values)
}

fn configured_template_path(
    workflow: &WorkflowDefinition,
    id: WorkpadTemplateId,
) -> Option<PathBuf> {
    let config = workflow.config.get("workpad_templates")?;
    let map = config.as_object()?;
    let relative = map.get(id.key())?.as_str()?.trim();
    (!relative.is_empty()).then(|| resolve_workflow_relative_path(&workflow.path, relative))
}

fn resolve_workflow_relative_path(workflow_path: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        workflow_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

fn fallback_template(id: WorkpadTemplateId) -> &'static str {
    match id {
        WorkpadTemplateId::MainHandoff => MAIN_HANDOFF,
        WorkpadTemplateId::MainHandoffFailure => MAIN_HANDOFF_FAILURE,
        WorkpadTemplateId::MainAssigneeOwnership => MAIN_ASSIGNEE_OWNERSHIP,
        WorkpadTemplateId::MainQualityGate => MAIN_QUALITY_GATE,
        WorkpadTemplateId::MainRuntimeOwnership => MAIN_RUNTIME_OWNERSHIP,
        WorkpadTemplateId::MainUsageLimitPause => MAIN_USAGE_LIMIT_PAUSE,
        WorkpadTemplateId::ParentTopology => PARENT_TOPOLOGY,
        WorkpadTemplateId::WorkspaceAdoption => WORKSPACE_ADOPTION,
        WorkpadTemplateId::WorkspaceEnsure => WORKSPACE_ENSURE,
        WorkpadTemplateId::AgentReviewRun => AGENT_REVIEW_RUN,
        WorkpadTemplateId::AgentReviewHandoff => AGENT_REVIEW_HANDOFF,
        WorkpadTemplateId::RepeatedReviewFailure => REPEATED_REVIEW_FAILURE,
        WorkpadTemplateId::ManualReview => MANUAL_REVIEW,
        WorkpadTemplateId::MergeRun => MERGE_RUN,
        WorkpadTemplateId::MergeRepair => MERGE_REPAIR,
        WorkpadTemplateId::DoctorTriage => DOCTOR_TRIAGE,
        WorkpadTemplateId::HumanReviewRepair => HUMAN_REVIEW_REPAIR,
        WorkpadTemplateId::ForgeReworkRun => FORGE_REWORK_RUN,
        WorkpadTemplateId::ForgeReworkBlocked => FORGE_REWORK_BLOCKED,
    }
}

pub fn configured_workpad_template_paths(config: &Value) -> BTreeMap<String, String> {
    config
        .get("workpad_templates")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|path| (key.clone(), path.into())))
                .collect()
        })
        .unwrap_or_default()
}

const MAIN_HANDOFF: &str = r#"## Shea Symphony Workpad

### Context
- Issue: {{issue_ref}} {{issue_title}}
- Source: `shea-symphony main loop`

### Plan
- [x] Read the issue contract, Project state, Main Workpad, and timeline evidence.
- [x] Prepare or resume the isolated issue workspace and branch.
- [x] Run the configured Main Agent backend for the implementation slice.
- [x] Verify handoff evidence and prepare the PR for Agent Review.

### Work Log
- Run `{{run_id}}` executed with backend `{{backend}}`.
- Workspace `{{workspace_path}}` was used for implementation evidence.
- Backend message: {{message}}

### Run Evidence
{{run_evidence}}

### Planned Handoff
{{planned_handoff}}

### Main-Agent Boundary
- Locally complete main-agent work stops at `Agent Review`.
- `Human Review` is reserved for independent Review Agent pass evidence.

{{runtime_ownership_marker}}"#;

const MAIN_RUNTIME_OWNERSHIP: &str = r#"## Shea Symphony Workpad

### Runtime Ownership
- Issue: {{issue_ref}} {{issue_title}}
- Event: `{{event}}`
- Run: `{{run}}`
- Claim: `{{claim}}`
- This marker is advisory tracker-visible ownership for active `In Progress` work.
- Another main loop profile should not resume this issue when the marker differs.

{{runtime_ownership_marker}}"#;

const MAIN_HANDOFF_FAILURE: &str = r#"## Shea Symphony Workpad

### Context
- Issue: {{issue_ref}} {{issue_title}}
- Source: `shea-symphony main loop`

### Handoff Planning Blocker
- Error: `{{error}}`
- Backend execution was skipped before claim/run to avoid mixing issue scope.

### Required Human Decision
- Confirm the correct branch/workspace ownership before retrying."#;

const MAIN_ASSIGNEE_OWNERSHIP: &str = r#"## Shea Symphony Workpad

### Assignee Ownership Blocker
- Issue: {{issue_ref}} {{issue_title}}
- Reason: {{reason}}
- Issue assignees: `{{assignees}}`

### Boundary
- Shea Symphony did not claim this issue or move it to `In Progress`.
- Assign the issue to the active GitHub identity or selected execution profile before retrying."#;

const MAIN_QUALITY_GATE: &str = r#"## Shea Symphony Workpad

### Context
- Issue: {{issue_ref}} {{issue_title}}
- Current state: {{current_state}}

### Decisions / Assumptions
{{assumptions}}

### Quality Gate
- Decision: {{decision}}
{{missing}}
{{notes}}

### Plan
- [ ] Resolve quality-gate findings before dispatch.

### Validation
- [ ] Re-run `shea-symphony forge validate --issue` after issue updates."#;

const MAIN_USAGE_LIMIT_PAUSE: &str = r#"## Shea Symphony Workpad

### Usage-Limit Pause
- Issue: {{issue_ref}} {{issue_title}}
- Source: `shea-symphony main loop`
- Backend: `{{backend}}`
- Classifier: `{{classifier}}`
- Evidence: {{evidence}}
- Retry backoff: `{{retry_delay_ms}}ms`

### State Safety
- Tracker state was not advanced to `Agent Review`.
- Runtime state keeps the active issue and next retry time.
- The main loop will skip this issue until retry backoff expires or an operator intervenes."#;

const PARENT_TOPOLOGY: &str = r#"## Shea Symphony Workpad

### Parent Topology
- Parent issue: {{parent_issue_ref}} {{parent_issue_title}}
- First observed subissue: {{issue_ref}} {{issue_title}}
- Parent integration branch: `{{parent_integration_branch}}`
- Parent final base branch: `{{parent_final_base_branch}}`
- Source: `shea-symphony main loop parent topology ensure`
- Purpose: durable branch evidence before native subissue PR handoff."#;

const WORKSPACE_ADOPTION: &str = r#"## Shea Symphony Workspace Adoption

- Issue: {{issue_ref}} {{issue_title}}
- Workspace path: `{{workspace_path}}`
- Branch: `{{branch}}`
- Head: `{{head}}`
- Evidence summary: {{evidence_summary}}"#;

const WORKSPACE_ENSURE: &str = r#"## Shea Symphony Workspace Ensure

- Issue: {{issue_ref}} {{issue_title}}
- Workspace path: `{{workspace_path}}`
- Branch: `{{branch}}`
- Source: `workspace ensure`
- Evidence summary: {{evidence_summary}}"#;

const AGENT_REVIEW_RUN: &str = include_str!("../workflows/template/workpad/agent-review.md");

const AGENT_REVIEW_HANDOFF: &str = r#"## Shea Symphony Agent Review Handoff

- Issue: {{issue_ref}} {{issue_title}}
- Status: `{{status}}`
- Target state after handoff: `{{target_state}}`
- PR: `{{pull_request}}`
- Project linked PR verified: `{{project_pr_link_verified}}`
- PR draft: `{{pull_request_is_draft}}`
- Validation: {{validation_summary}}
- Last transition: {{last_transition}}

### Missing Handoff Evidence
{{missing}}

### Boundary
- Main implementation agent stops at `Agent Review`.
- Independent Review Agent owns review evidence and any later Human Review routing."#;

const REPEATED_REVIEW_FAILURE: &str = r#"## Shea Symphony Agent Review Run

### Repeated Backend Failure
- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Worker key: `{{worker_key}}`
- Reviewer backend: `{{reviewer_backend}}`
- Job state: `{{job_state}}`
- Current job id: `{{job_id}}`
- First same-cause job id: `{{first_job_id}}`
- Previous same-cause job id: `{{previous_job_id}}`
- Same-cause repeat count: `{{repeat_count}}`
- Failure signature: `{{signature}}`
- Decision: {{decision}}
- Target state after review routing: `{{target_state}}`
- Evidence policy: compact repeat line only; full diagnostic was already recorded for the first same-cause attempt.
{{ledger_line}}
{{gemini_health_lines}}"#;

const MANUAL_REVIEW: &str = r#"## Shea Symphony Agent Review Run

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `review`
- Actor role: `review_agent`
- Actor: `{{actor}}`
- Run ID: `{{run_id}}`
- Input state: `Agent Review`
- Reviewer backend: manual-operator
- Decision: Manual independent review {{decision}}.
- Target state after review routing: `{{target_state}}`
- Result: `{{result}}`
- PR: `{{pr}}`
- Review Agent claim: `{{current_claim}}`
- Terminal Review Agent claim: `{{terminal_claim}}`
- Evidence summary: manual review evidence captured below.

### Manual Review Evidence
````md
{{evidence}}
````
{{result_note}}"#;

const MERGE_RUN: &str = r#"## Shea Symphony Merge Run

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `merge`
- Actor role: `merge_agent`
- Result: `{{result}}`
- Target state after merge routing: `{{target_state}}`
- PR: `{{pr}}`
- Decision: `{{decision}}`
- Evidence summary: {{evidence_summary}}

### Preflight
{{preflight}}

### Merge Action
{{merge_action}}

### Post-Merge Readback
{{post_merge_readback}}

### Merge Repair Evidence
{{merge_repair_evidence}}

{{required_human_input}}"#;

const MERGE_REPAIR: &str = r#"## Shea Symphony Merge Repair

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- PR: `{{pr}}`
- Result: `{{result}}`
- Evidence summary: {{evidence_summary}}

### Repair Evidence
{{repair_evidence}}

### Boundary
- Merge-lane repair records evidence before tracker routing."#;

const DOCTOR_TRIAGE: &str = r#"## Shea Symphony Doctor Triage

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `doctor`
- Actor role: `doctor`
- Actor: `shea-symphony doctor`
- Run ID: `{{run_id}}`
- Input state: `{{input_state}}`
- Target state after repair: `{{target_state}}`
- Result: `{{result}}`
- Requested action: `{{action}}`
{{extra_lines}}
- Evidence summary: {{evidence_summary}}

### Doctor Findings
{{doctor_findings}}

### State Boundary
- Doctor repair records evidence before any tracker mutation.
- This repair does not delete worktrees, discard local work, or bypass review/merge lane authority."#;

const HUMAN_REVIEW_REPAIR: &str = r#"## Shea Symphony Doctor Triage

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `doctor`
- Actor role: `doctor`
- Actor: `shea-symphony doctor`
- Run ID: `doctor-human-review-repair`
- Input state: `{{input_state}}`
- Target state after repair: `Agent Review`
- Result: `repair_recorded`
- PR evidence: `not recorded`
- Violation: `{{violation_code}}`
- Previous state: `{{input_state}}`
- Message: {{message}}
- Repair: {{repair}}
- Evidence summary: invalid Human Review boundary repair evidence recorded before tracker mutation.

### State Boundary
- Main implementation agent is moving this issue back to `Agent Review`.
- This repair does not set `Human Review`; that state requires independent Review Agent pass evidence."#;

const FORGE_REWORK_RUN: &str = r#"## Shea Symphony Rework Run

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `main`
- Actor role: `human_review_revision`
- Actor: `operator`
- Run ID: `forge-rework`
- Run type: `human_review_rework_revision`
- Input state: `Human Review`
- Target state after run: `Rework`
- Result: `rework_revision_recorded`
- PR: `{{pr}}`
- Replacement Rework title/status: `{{rework_title}}` / `Rework`
- Operator confirmation: {{operator_confirmation}}
- Evidence summary: operator confirmation, replacement contract, and readback evidence recorded.
- Source state validated as `Human Review` before mutation.
- Terminal lane claims, when present, were preserved as audit pointers.
- Active lane claims in `Human Review` are rejected before content or status writes.
- Replacement body was written and read back before the final Project status mutation.
- Final Project status mutation is `Rework`.

### Rework Direction

{{evidence}}

### Verification Readback

{{readbacks}}

### Role Boundary

- Main Agent may claim `Rework`, repair the revised contract, and stop at `Agent Review`.
- `Human Review` remains reserved for independent Review Agent pass evidence."#;

const FORGE_REWORK_BLOCKED: &str = r#"## Shea Symphony Rework Run

- Generated at: `{{generated_at}}`
- Issue: {{issue_ref}} {{issue_title}}
- Lane: `main`
- Actor role: `human_review_revision`
- Actor: `operator`
- Run ID: `forge-rework`
- Run type: `human_review_rework_revision`
- Source state: `Human Review`
- Target state after run: `unchanged`
- Result: `blocked`
- PR: `{{pr}}`
- Blocker: {{reason}}
- Evidence summary: blocked rework revision recorded before any state mutation.
- No replacement body was written.
- Project status was not changed to `Rework`.
- Resolve or supersede the active lane claim before retrying `forge rework`."#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_registry_covers_every_template_id() {
        for id in WorkpadTemplateId::all() {
            let template = workpad_template_for(None, *id);
            assert!(!template.body.trim().is_empty(), "missing {:?}", id);
            assert_eq!(template.source, WorkpadTemplateSource::CentralizedFallback);
        }
    }

    #[test]
    fn renders_configured_workflow_template_file() {
        let dir = tempfile::tempdir().unwrap();
        let template_path = dir.path().join("main.md");
        fs::write(&template_path, "Configured {{issue_ref}}").unwrap();
        let workflow_path = dir.path().join("WORKFLOW.md");
        let workflow = WorkflowDefinition::parse(
            &workflow_path,
            "---\nworkpad_templates:\n  main_handoff: main.md\n---\nPrompt",
        )
        .unwrap();

        let rendered = render_workpad_template(
            Some(&workflow),
            WorkpadTemplateId::MainHandoff,
            &[("issue_ref", "#435".into())],
        );

        assert_eq!(rendered.unwrap(), "Configured #435");
        assert!(matches!(
            workpad_template_for(Some(&workflow), WorkpadTemplateId::MainHandoff).source,
            WorkpadTemplateSource::WorkflowFile(_)
        ));
    }

    #[test]
    fn configured_workpad_templates_support_liquid_loops_and_filters() {
        let dir = tempfile::tempdir().unwrap();
        let template_path = dir.path().join("main.md");
        fs::write(
            &template_path,
            "{% assign items = values | split: ',' %}{% for item in items %}{{ item | strip | upcase }}{% unless forloop.last %}/{{ items | size }}:{% endunless %}{% endfor %}",
        )
        .unwrap();
        let workflow_path = dir.path().join("WORKFLOW.md");
        let workflow = WorkflowDefinition::parse(
            &workflow_path,
            "---\nworkpad_templates:\n  main_handoff: main.md\n---\nPrompt",
        )
        .unwrap();

        let rendered = render_workpad_template(
            Some(&workflow),
            WorkpadTemplateId::MainHandoff,
            &[("values", "alpha, beta".into())],
        )
        .unwrap();

        assert_eq!(rendered, "ALPHA/2:BETA");
    }

    #[test]
    fn configured_workpad_templates_reject_unknown_variables_filters_and_malformed_liquid() {
        let dir = tempfile::tempdir().unwrap();
        let workflow_path = dir.path().join("WORKFLOW.md");

        for (template_name, body) in [
            ("unknown.md", "{{ missing_value }}"),
            ("filter.md", "{{ issue_ref | missing_filter }}"),
            ("malformed.md", "{% for item in values %}{{ item }}"),
        ] {
            fs::write(dir.path().join(template_name), body).unwrap();
            let workflow = WorkflowDefinition::parse(
                &workflow_path,
                &format!("---\nworkpad_templates:\n  main_handoff: {template_name}\n---\nPrompt"),
            )
            .unwrap();

            let error = render_workpad_template(
                Some(&workflow),
                WorkpadTemplateId::MainHandoff,
                &[("issue_ref", "#436".into()), ("values", "alpha".into())],
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("unknown template variable")
                    || error.to_string().contains("template parse error"),
                "unexpected error for {template_name}: {error}"
            );
        }
    }

    #[test]
    fn missing_configured_template_reports_diagnostic_and_uses_fallback() {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\nworkpad_templates:\n  main_handoff: missing.md\n---\nPrompt",
        )
        .unwrap();
        let template = workpad_template_for(Some(&workflow), WorkpadTemplateId::MainHandoff);

        assert!(template.body.contains("## Shea Symphony Workpad"));
        assert!(matches!(
            template.source,
            WorkpadTemplateSource::MissingOrInvalid { .. }
        ));
    }

    #[test]
    fn canonical_workflow_readback_lists_every_workpad_surface() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("workflows/shea-symphony.md");
        let workflow = WorkflowDefinition::load(path).unwrap();
        let readback = workpad_template_readback(&workflow);

        assert_eq!(readback.len(), WorkpadTemplateId::all().len());
        for id in WorkpadTemplateId::all() {
            let template = readback.iter().find(|template| template.id == *id).unwrap();
            assert!(!template.body.trim().is_empty(), "empty {:?}", id);
        }
        assert!(readback
            .iter()
            .any(|template| matches!(template.source, WorkpadTemplateSource::WorkflowFile(_))));
        assert!(readback
            .iter()
            .any(|template| matches!(template.source, WorkpadTemplateSource::CentralizedFallback)));
    }

    #[test]
    fn migrated_renderers_do_not_reintroduce_scattered_workpad_layout_headings() {
        let migrated_sources = [
            (
                "src/lanes/main_loop/handoff.rs",
                include_str!("lanes/main_loop/handoff.rs"),
            ),
            (
                "src/lanes/main_loop/write_candidate.rs",
                include_str!("lanes/main_loop/write_candidate.rs"),
            ),
            ("src/doctor/report.rs", include_str!("doctor/report.rs")),
            ("src/merge_lane.rs", include_str!("merge_lane.rs")),
            (
                "src/lanes/review/manual.rs",
                include_str!("lanes/review/manual.rs"),
            ),
            (
                "src/commands/forge/rework.rs",
                include_str!("commands/forge/rework.rs"),
            ),
            ("src/commands/gate.rs", include_str!("commands/gate.rs")),
        ];
        let forbidden = [
            "\"## Shea Symphony Workpad\"",
            "\"## Shea Symphony Merge Run\"",
            "\"## Shea Symphony Doctor Triage\"",
            "\"## Shea Symphony Rework Run\"",
            "\"### Runtime Ownership\"",
            "\"### Parent Topology\"",
            "\"### Manual Review Evidence\"",
            "\"### Usage-Limit Pause\"",
            "\"### Assignee Ownership Blocker\"",
            "\"### Handoff Planning Blocker\"",
        ];

        for (path, source) in migrated_sources {
            for heading in forbidden {
                assert!(
                    !source.contains(heading),
                    "{path} reintroduced scattered workpad layout heading {heading}"
                );
            }
        }
    }
}
