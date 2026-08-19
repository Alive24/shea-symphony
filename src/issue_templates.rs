//! Repository-owned executable-Issue template loading and rendering.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::prompt::{render_template_with_json, PromptError};

/// Stable workflow key for the one executable-Issue template.
pub const EXECUTABLE_ISSUE_TEMPLATE_KEY: &str = "executable";

/// A validated raw executable-Issue template and its exact repository source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableIssueTemplate {
    /// Trusted raw Markdown/Liquid, including same-file semantic intent.
    pub body: String,
    /// Exact selected repository path.
    pub path: PathBuf,
}

/// Fail-closed executable-Issue template errors.
#[derive(Debug, Error)]
pub enum IssueTemplateError {
    /// The selected repository file could not be read.
    #[error("executable-Issue template is unavailable at {path}: {source}")]
    Unavailable {
        /// Selected path.
        path: PathBuf,
        /// Filesystem failure.
        source: std::io::Error,
    },
    /// The selected repository file was empty.
    #[error("executable-Issue template is empty at {0}")]
    Empty(PathBuf),
    /// Strict Liquid rejected parsing, context, or rendering.
    #[error("executable-Issue template failed strict rendering at {path}: {source}")]
    Render {
        /// Selected path.
        path: PathBuf,
        /// Strict renderer failure.
        source: PromptError,
    },
    /// A render left Liquid syntax in the candidate body.
    #[error("executable-Issue template rendered unresolved Liquid syntax at {0}")]
    Unresolved(PathBuf),
}

/// Load the one workflow-selected executable-Issue template.
pub fn load_executable_issue_template(
    config: &RuntimeConfig,
) -> Result<ExecutableIssueTemplate, IssueTemplateError> {
    load_executable_issue_template_path(&config.issue_templates.executable)
}

/// Load the repository default used by compatibility APIs and focused tests.
pub fn load_repository_executable_issue_template(
) -> Result<ExecutableIssueTemplate, IssueTemplateError> {
    load_executable_issue_template_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".shea/template/issue/executable.md"),
    )
}

fn load_executable_issue_template_path(
    path: &Path,
) -> Result<ExecutableIssueTemplate, IssueTemplateError> {
    let body = fs::read_to_string(path).map_err(|source| IssueTemplateError::Unavailable {
        path: path.to_path_buf(),
        source,
    })?;
    if body.trim().is_empty() {
        return Err(IssueTemplateError::Empty(path.to_path_buf()));
    }
    Ok(ExecutableIssueTemplate {
        body: body.trim().to_string(),
        path: path.to_path_buf(),
    })
}

/// Render an executable Issue from trusted raw template plus typed values.
pub fn render_executable_issue_template(
    template: &ExecutableIssueTemplate,
    values: &Value,
) -> Result<String, IssueTemplateError> {
    let rendered = render_template_with_json(&template.body, values).map_err(|source| {
        IssueTemplateError::Render {
            path: template.path.clone(),
            source,
        }
    })?;
    if contains_liquid_syntax(&rendered) {
        return Err(IssueTemplateError::Unresolved(template.path.clone()));
    }
    Ok(rendered)
}

/// Smoke context covering every runtime-owned executable-Issue input.
pub fn executable_issue_smoke_values() -> Value {
    serde_json::json!({
        "uat_required": "No",
        "assignee": "Alive24",
        "dependencies": "None",
        "documentation_impact": "Update the operator contract documentation.",
        "related_context": "None",
        "parent_subissue": false,
        "goal": "Deliver one executable outcome.",
        "why_now": "A downstream slice depends on this contract.",
        "target_repository": "- `Alive24/shea-symphony`",
        "context": "Current repository evidence was inspected.",
        "guardrails": "- Preserve guarded writes and targeted readback.",
        "in_scope": "- Implement the accepted slice.",
        "out_of_scope": "- Unrelated redesign.",
        "knowledge_sources": "- `docs/README.md`",
        "code_paths": "- `src/lib.rs`",
        "current_state": "The relevant source exists on the current base.",
        "code_state_freshness": "Refresh the target branch before dispatch.",
        "deliverable_shape": "One reviewed pull request.",
        "risks": "- Avoid widening tracker authority.",
        "expected_outcome": "- [ ] The configured behavior is observable.",
        "completion_criteria": "- [ ] Focused tests pass.",
        "functional_verification": "- [ ] `cargo test`",
        "uat": "- [ ] Not required for this smoke render.",
        "context_verification": "- [ ] Recheck current base and relationships."
    })
}

fn contains_liquid_syntax(rendered: &str) -> bool {
    ["{{", "}}", "{%", "%}"]
        .iter()
        .any(|marker| rendered.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_template_hides_same_file_intent_from_rendered_issue() {
        let template = load_repository_executable_issue_template().unwrap();
        assert!(template.body.contains("semantic validation intent"));

        let rendered =
            render_executable_issue_template(&template, &executable_issue_smoke_values()).unwrap();

        assert!(rendered.contains("## Issue Goal"));
        assert!(!rendered.contains("semantic validation intent"));
        assert!(!contains_liquid_syntax(&rendered));
    }

    #[test]
    fn customized_non_english_layout_renders_without_code_changes() {
        let template = ExecutableIssueTemplate {
            body: "{% comment %}Require one verifiable goal.{% endcomment %}## Objetivo\n\n{{ goal }}\n\n## Alcance\n\n{{ in_scope }}".into(),
            path: PathBuf::from("custom.md"),
        };

        let rendered =
            render_executable_issue_template(&template, &executable_issue_smoke_values()).unwrap();

        assert_eq!(
            rendered,
            "## Objetivo\n\nDeliver one executable outcome.\n\n## Alcance\n\n- Implement the accepted slice."
        );
    }

    #[test]
    fn unknown_input_and_unresolved_output_fail_closed() {
        let unknown = ExecutableIssueTemplate {
            body: "{{ repository_policy_missing }}".into(),
            path: PathBuf::from("unknown.md"),
        };
        assert!(matches!(
            render_executable_issue_template(&unknown, &executable_issue_smoke_values()),
            Err(IssueTemplateError::Render { .. })
        ));

        let unresolved = ExecutableIssueTemplate {
            body: "{{ goal }}".into(),
            path: PathBuf::from("unresolved.md"),
        };
        let mut values = executable_issue_smoke_values();
        values["goal"] = "{{ unresolved }}".into();
        assert!(matches!(
            render_executable_issue_template(&unresolved, &values),
            Err(IssueTemplateError::Unresolved(_))
        ));
    }

    #[test]
    fn production_rust_does_not_duplicate_executable_issue_policy() {
        let forge = include_str!("issue_forge.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let gate = include_str!("quality_gate.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for forbidden in [
            "## Issue Setup",
            "## Issue Goal",
            "Target Repository / Package",
            "UAT Required field",
            "REQUIRED_SECTIONS",
            "contains_any_dependency_marker",
        ] {
            assert!(!forge.contains(forbidden), "Forge duplicates `{forbidden}`");
            assert!(!gate.contains(forbidden), "gate duplicates `{forbidden}`");
        }

        let forge_command = include_str!("commands/forge.rs");
        for forbidden_parser in [
            "strip_prefix(\"Assignee:\")",
            "strip_prefix(\"Assignees:\")",
            "strip_prefix(\"UAT Required:\")",
        ] {
            assert!(
                !forge_command.contains(forbidden_parser),
                "Forge parses template-owned field label `{forbidden_parser}`"
            );
        }

        let issue_template_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".shea/template/issue");
        let templates = fs::read_dir(issue_template_root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
            .count();
        assert_eq!(
            templates, 1,
            "executable-Issue policy needs one Markdown owner"
        );
    }
}
