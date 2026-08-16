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
    ReviewInvalidHandoff,
    ReworkDiagnostic,
    ReviewFreshness,
    MergeRun,
    MergeRepair,
    DoctorTriage,
    HumanReviewRepair,
    ForgeReworkRun,
    ForgeReworkBlocked,
    LaneSession,
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
    RepositoryMarkdownDefault(&'static str),
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
            Self::ReviewInvalidHandoff => "review_invalid_handoff",
            Self::ReworkDiagnostic => "rework_diagnostic",
            Self::ReviewFreshness => "review_freshness",
            Self::MergeRun => "merge_run",
            Self::MergeRepair => "merge_repair",
            Self::DoctorTriage => "doctor_triage",
            Self::HumanReviewRepair => "human_review_repair",
            Self::ForgeReworkRun => "forge_rework_run",
            Self::ForgeReworkBlocked => "forge_rework_blocked",
            Self::LaneSession => "lane_session",
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
            Self::ReviewInvalidHandoff,
            Self::ReworkDiagnostic,
            Self::ReviewFreshness,
            Self::MergeRun,
            Self::MergeRepair,
            Self::DoctorTriage,
            Self::HumanReviewRepair,
            Self::ForgeReworkRun,
            Self::ForgeReworkBlocked,
            Self::LaneSession,
        ]
    }
}

impl WorkpadTemplateSource {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::WorkflowFile(_) => "workflow_template_file",
            Self::RepositoryMarkdownDefault(_) => "repository_markdown_default",
            Self::MissingOrInvalid { .. } => "missing_or_invalid_template",
        }
    }

    pub fn path_display(&self) -> String {
        match self {
            Self::WorkflowFile(path) | Self::MissingOrInvalid { path, .. } => {
                path.display().to_string()
            }
            Self::RepositoryMarkdownDefault(path) => (*path).into(),
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
        if workflow.config.get("workpad_templates").is_none() {
            let (path, body) = repository_markdown_default(id);
            return WorkpadTemplate {
                id,
                body: body.trim().to_string(),
                source: WorkpadTemplateSource::RepositoryMarkdownDefault(path),
            };
        }
        return if let Some(path) = configured_template_path(workflow, id) {
            match fs::read_to_string(&path) {
                Ok(body) if !body.trim().is_empty() => WorkpadTemplate {
                    id,
                    body: body.trim().to_string(),
                    source: WorkpadTemplateSource::WorkflowFile(path),
                },
                Ok(_) => WorkpadTemplate {
                    id,
                    body: String::new(),
                    source: WorkpadTemplateSource::MissingOrInvalid {
                        path,
                        error: format!(
                            "required workpad template `{}` is empty; repair workpad_templates.{} in {}",
                            id.key(),
                            id.key(),
                            workflow.path.display()
                        ),
                    },
                },
                Err(error) => WorkpadTemplate {
                    id,
                    body: String::new(),
                    source: WorkpadTemplateSource::MissingOrInvalid {
                        path: path.clone(),
                        error: format!(
                            "required workpad template `{}` could not be read at {}: {error}; repair workpad_templates.{} in {}",
                            id.key(),
                            path.display(),
                            id.key(),
                            workflow.path.display()
                        ),
                    },
                },
            }
        } else {
            WorkpadTemplate {
                id,
                body: String::new(),
                source: WorkpadTemplateSource::MissingOrInvalid {
                    path: workflow.path.clone(),
                    error: format!(
                        "required workpad_templates.{} is missing from {}; configure a repository-owned Markdown file",
                        id.key(),
                        workflow.path.display()
                    ),
                },
            }
        };
    }

    let (path, body) = repository_markdown_default(id);
    WorkpadTemplate {
        id,
        body: body.trim().to_string(),
        source: WorkpadTemplateSource::RepositoryMarkdownDefault(path),
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
    let template = workpad_template_for(workflow, id);
    if let Some(diagnostic) = template.source.diagnostic() {
        return Err(PromptError::Context(diagnostic.to_string()));
    }
    render_template_with_values(&template.body, values)
}

pub fn smoke_render_workpad_template(
    template: &WorkpadTemplate,
    values: &[(&str, String)],
) -> Result<(), PromptError> {
    if let Some(diagnostic) = template.source.diagnostic() {
        return Err(PromptError::Context(diagnostic.to_string()));
    }
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

fn repository_markdown_default(id: WorkpadTemplateId) -> (&'static str, &'static str) {
    match id {
        WorkpadTemplateId::MainHandoff => (
            ".shea/template/workpad/main-handoff.md",
            include_str!("../.shea/template/workpad/main-handoff.md"),
        ),
        WorkpadTemplateId::MainHandoffFailure => (
            ".shea/template/workpad/main-handoff-failure.md",
            include_str!("../.shea/template/workpad/main-handoff-failure.md"),
        ),
        WorkpadTemplateId::MainAssigneeOwnership => (
            ".shea/template/workpad/main-assignee-ownership.md",
            include_str!("../.shea/template/workpad/main-assignee-ownership.md"),
        ),
        WorkpadTemplateId::MainQualityGate => (
            ".shea/template/workpad/main-quality-gate.md",
            include_str!("../.shea/template/workpad/main-quality-gate.md"),
        ),
        WorkpadTemplateId::MainRuntimeOwnership => (
            ".shea/template/workpad/main-runtime-ownership.md",
            include_str!("../.shea/template/workpad/main-runtime-ownership.md"),
        ),
        WorkpadTemplateId::MainUsageLimitPause => (
            ".shea/template/workpad/main-usage-limit-pause.md",
            include_str!("../.shea/template/workpad/main-usage-limit-pause.md"),
        ),
        WorkpadTemplateId::ParentTopology => (
            ".shea/template/workpad/parent-topology.md",
            include_str!("../.shea/template/workpad/parent-topology.md"),
        ),
        WorkpadTemplateId::WorkspaceAdoption => (
            ".shea/template/workpad/workspace-adoption.md",
            include_str!("../.shea/template/workpad/workspace-adoption.md"),
        ),
        WorkpadTemplateId::WorkspaceEnsure => (
            ".shea/template/workpad/workspace-ensure.md",
            include_str!("../.shea/template/workpad/workspace-ensure.md"),
        ),
        WorkpadTemplateId::AgentReviewRun => (
            ".shea/template/workpad/agent-review.md",
            include_str!("../.shea/template/workpad/agent-review.md"),
        ),
        WorkpadTemplateId::AgentReviewHandoff => (
            ".shea/template/workpad/agent-review-handoff.md",
            include_str!("../.shea/template/workpad/agent-review-handoff.md"),
        ),
        WorkpadTemplateId::RepeatedReviewFailure => (
            ".shea/template/workpad/repeated-review-failure.md",
            include_str!("../.shea/template/workpad/repeated-review-failure.md"),
        ),
        WorkpadTemplateId::ManualReview => (
            ".shea/template/workpad/manual-review.md",
            include_str!("../.shea/template/workpad/manual-review.md"),
        ),
        WorkpadTemplateId::ReviewInvalidHandoff => (
            ".shea/template/workpad/review-invalid-handoff.md",
            include_str!("../.shea/template/workpad/review-invalid-handoff.md"),
        ),
        WorkpadTemplateId::ReworkDiagnostic => (
            ".shea/template/workpad/rework-diagnostic.md",
            include_str!("../.shea/template/workpad/rework-diagnostic.md"),
        ),
        WorkpadTemplateId::ReviewFreshness => (
            ".shea/template/workpad/review-freshness.md",
            include_str!("../.shea/template/workpad/review-freshness.md"),
        ),
        WorkpadTemplateId::MergeRun => (
            ".shea/template/workpad/merge-run.md",
            include_str!("../.shea/template/workpad/merge-run.md"),
        ),
        WorkpadTemplateId::MergeRepair => (
            ".shea/template/workpad/merge-repair.md",
            include_str!("../.shea/template/workpad/merge-repair.md"),
        ),
        WorkpadTemplateId::DoctorTriage => (
            ".shea/template/workpad/doctor-triage.md",
            include_str!("../.shea/template/workpad/doctor-triage.md"),
        ),
        WorkpadTemplateId::HumanReviewRepair => (
            ".shea/template/workpad/human-review-repair.md",
            include_str!("../.shea/template/workpad/human-review-repair.md"),
        ),
        WorkpadTemplateId::ForgeReworkRun => (
            ".shea/template/workpad/forge-rework-run.md",
            include_str!("../.shea/template/workpad/forge-rework-run.md"),
        ),
        WorkpadTemplateId::ForgeReworkBlocked => (
            ".shea/template/workpad/forge-rework-blocked.md",
            include_str!("../.shea/template/workpad/forge-rework-blocked.md"),
        ),
        WorkpadTemplateId::LaneSession => (
            ".shea/template/workpad/lane-session.md",
            include_str!("../.shea/template/workpad/lane-session.md"),
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_markdown_registry_covers_every_template_id() {
        for id in WorkpadTemplateId::all() {
            let template = workpad_template_for(None, *id);
            assert!(!template.body.trim().is_empty(), "missing {id:?}");
            assert!(matches!(
                template.source,
                WorkpadTemplateSource::RepositoryMarkdownDefault(_)
            ));
        }
    }

    #[test]
    fn renders_configured_workflow_template_file() {
        let dir = tempfile::tempdir().unwrap();
        let template_path = dir.path().join("main.md");
        fs::write(&template_path, "Configured {{issue_ref}}").unwrap();
        let workflow_path = dir.path().join("WORKFLOW.md");
        let mut workflow = WorkflowDefinition::parse(&workflow_path, "Prompt").unwrap();
        workflow.config = serde_json::json!({"workpad_templates": {"main_handoff": "main.md"}});

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
        let mut workflow = WorkflowDefinition::parse(&workflow_path, "Prompt").unwrap();
        workflow.config = serde_json::json!({"workpad_templates": {"main_handoff": "main.md"}});

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
            let mut workflow = WorkflowDefinition::parse(&workflow_path, "Prompt").unwrap();
            workflow.config =
                serde_json::json!({"workpad_templates": {"main_handoff": template_name}});

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
    fn missing_configured_template_fails_closed_with_repair_diagnostic() {
        let mut workflow = WorkflowDefinition::parse("/tmp/WORKFLOW.md", "Prompt").unwrap();
        workflow.config = serde_json::json!({"workpad_templates": {"main_handoff": "missing.md"}});
        let template = workpad_template_for(Some(&workflow), WorkpadTemplateId::MainHandoff);

        assert!(template.body.is_empty());
        assert!(matches!(
            template.source,
            WorkpadTemplateSource::MissingOrInvalid { .. }
        ));
        let error = render_workpad_template(
            Some(&workflow),
            WorkpadTemplateId::MainHandoff,
            &[("issue_ref", "#435".into())],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("required workpad template `main_handoff`"));
        assert!(error.contains("repair workpad_templates.main_handoff"));
    }

    #[test]
    fn canonical_workflow_readback_lists_every_workpad_surface() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".shea/workflows/shea-symphony.md");
        let workflow = WorkflowDefinition::load(path).unwrap();
        let readback = workpad_template_readback(&workflow);

        assert_eq!(readback.len(), WorkpadTemplateId::all().len());
        for id in WorkpadTemplateId::all() {
            let template = readback.iter().find(|template| template.id == *id).unwrap();
            assert!(!template.body.trim().is_empty(), "empty {id:?}");
        }
        assert!(readback
            .iter()
            .all(|template| matches!(template.source, WorkpadTemplateSource::WorkflowFile(_))));
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
            assert!(
                !source.contains("render_workpad_template(\n        None"),
                "{path} bypasses the active workflow template configuration"
            );
            for heading in forbidden {
                assert!(
                    !source.contains(heading),
                    "{path} reintroduced scattered workpad layout heading {heading}"
                );
            }
        }
    }
}
