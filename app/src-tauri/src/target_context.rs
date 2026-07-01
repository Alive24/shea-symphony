use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::workspace::WorkspaceProfile;

pub const DEFAULT_WORKFLOW_PATH: &str = "workflows/shea-symphony.md";
pub const DEFAULT_REPOSITORY: &str = "Alive24/shea-symphony";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetOptions {
    pub workflow_path: Option<String>,
    pub repository: Option<String>,
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TargetContext {
    pub workflow_path: String,
    pub repository: Option<String>,
    pub workspace_path: Option<String>,
    pub skills_path: Option<String>,
    pub self_workspace: bool,
    explicit_target: bool,
    workflow_exists: bool,
}

impl TargetContext {
    pub fn from_options(options: Option<&TargetOptions>) -> Self {
        let env_workflow = nonempty_env("SHEA_SYMPHONY_WORKFLOW");
        let workflow_path = options
            .and_then(|options| nonempty(options.workflow_path.as_deref()))
            .or(env_workflow.clone())
            .unwrap_or_else(|| DEFAULT_WORKFLOW_PATH.into());
        let workflow_file_path = workflow_file_path(&workflow_path);
        let workflow_exists = workflow_file_path.exists();
        let parsed = WorkflowTargetConfig::from_path(&workflow_file_path);
        let explicit_target = workflow_path != DEFAULT_WORKFLOW_PATH
            || env_workflow.is_some()
            || options
                .and_then(|options| nonempty(options.repository.as_deref()))
                .is_some();
        let repository = options
            .and_then(|options| nonempty(options.repository.as_deref()))
            .or_else(|| nonempty_env("SHEA_SYMPHONY_TARGET_REPOSITORY"))
            .or(parsed.repository)
            .or_else(|| (!explicit_target).then(|| DEFAULT_REPOSITORY.into()));
        let workspace_path = options
            .and_then(|options| nonempty(options.workspace_path.as_deref()))
            .or_else(|| nonempty_env("SHEA_SYMPHONY_TARGET_WORKSPACE"))
            .or(parsed.workspace_root);
        let self_workspace = repository.as_deref() == Some(DEFAULT_REPOSITORY)
            && workflow_path == DEFAULT_WORKFLOW_PATH
            && !explicit_target;
        let skills_path = workspace_path
            .as_deref()
            .map(|path| Path::new(path).join(".codex").join("skills"))
            .map(|path| path.display().to_string())
            .or_else(|| self_workspace.then(|| ".codex/skills".into()));

        Self {
            workflow_path,
            repository,
            workspace_path,
            skills_path,
            self_workspace,
            explicit_target,
            workflow_exists,
        }
    }

    pub fn from_workspace(workspace: &WorkspaceProfile) -> Self {
        let workflow_path = nonempty(Some(workspace.workflow_path.as_str()))
            .unwrap_or_else(|| DEFAULT_WORKFLOW_PATH.into());
        let workflow_file_path = workspace.workflow_file_path();
        let workflow_exists = workflow_file_path.exists();
        let parsed = WorkflowTargetConfig::from_path(&workflow_file_path);
        let self_workspace = workspace.target_root == workspace.engine_root;
        let repository = parsed
            .repository
            .or_else(|| self_workspace.then(|| DEFAULT_REPOSITORY.into()));
        let workspace_path = Some(workspace.target_root.clone());
        let skills_path = workspace_path
            .as_deref()
            .map(|path| Path::new(path).join(".codex").join("skills"))
            .map(|path| path.display().to_string());

        Self {
            workflow_path,
            repository,
            workspace_path,
            skills_path,
            self_workspace,
            explicit_target: !self_workspace,
            workflow_exists,
        }
    }

    pub fn readiness(&self) -> Value {
        let mut blockers = Vec::new();
        if !self.workflow_exists {
            blockers.push(format!(
                "Workflow path is not readable: {}",
                self.workflow_path
            ));
        }
        if self.repository.is_none() {
            blockers.push("Target GitHub repository is not configured.".into());
        }
        if self.explicit_target && self.workspace_path.is_none() {
            blockers.push("Target workspace path is not configured.".into());
        }

        json!({
            "status": if blockers.is_empty() { "ready" } else { "missingTargetConfig" },
            "blockers": blockers,
        })
    }

    pub fn to_value(&self) -> Value {
        json!({
            "workflowPath": self.workflow_path,
            "repository": self.repository,
            "workspacePath": self.workspace_path,
            "skillsPath": self.skills_path,
            "mode": if self.self_workspace { "self" } else { "target" },
            "selfWorkspace": self.self_workspace,
            "readiness": self.readiness(),
        })
    }
}

#[derive(Default)]
struct WorkflowTargetConfig {
    repository: Option<String>,
    workspace_root: Option<String>,
}

impl WorkflowTargetConfig {
    fn from_path(path: &Path) -> Self {
        let Ok(content) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Some(front_matter) = front_matter(&content) else {
            return Self::default();
        };
        let owner = yaml_section_value(front_matter, "tracker", "owner");
        let repo = yaml_section_value(front_matter, "tracker", "repo");
        let repository = owner
            .zip(repo)
            .map(|(owner, repo)| format!("{owner}/{repo}"));
        let workspace_root =
            yaml_section_value(front_matter, "workspace", "root").map(expand_env_prefix);
        Self {
            repository,
            workspace_root,
        }
    }
}

fn front_matter(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    rest.split_once("\n---")
        .map(|(front_matter, _)| front_matter)
}

fn yaml_section_value(content: &str, section: &str, key: &str) -> Option<String> {
    let mut current = "";
    for line in content.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && line.trim_end().ends_with(':') {
            current = line.trim().trim_end_matches(':');
            continue;
        }
        if current == section {
            let trimmed = line.trim();
            let Some((candidate, value)) = trimmed.split_once(':') else {
                continue;
            };
            if candidate.trim() == key {
                return nonempty(Some(value.trim().trim_matches(['"', '\''])));
            }
        }
    }
    None
}

fn expand_env_prefix(value: String) -> String {
    let Some(rest) = value.strip_prefix('$') else {
        return value;
    };
    let split = rest.find(['/', '\\']).unwrap_or(rest.len());
    let (name, suffix) = rest.split_at(split);
    env::var(name)
        .map(|prefix| format!("{prefix}{suffix}"))
        .unwrap_or(value)
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn nonempty_env(name: &str) -> Option<String> {
    env::var(name).ok().and_then(|value| nonempty(Some(&value)))
}

fn workflow_file_path(path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        repo_root().join(path)
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_shea_self_workspace() {
        let context = TargetContext::from_options(None);

        assert_eq!(context.workflow_path, DEFAULT_WORKFLOW_PATH);
        assert_eq!(context.repository.as_deref(), Some(DEFAULT_REPOSITORY));
        assert!(context.self_workspace);
        assert_eq!(
            context
                .readiness()
                .get("status")
                .and_then(Value::as_str)
                .unwrap(),
            "ready"
        );
    }

    #[test]
    fn external_workflow_without_repo_reports_missing_config() {
        let context = TargetContext::from_options(Some(&TargetOptions {
            workflow_path: Some("/tmp/missing-target-workflow.md".into()),
            repository: None,
            workspace_path: None,
        }));

        assert_eq!(context.repository, None);
        assert_eq!(
            context
                .readiness()
                .get("status")
                .and_then(Value::as_str)
                .unwrap(),
            "missingTargetConfig"
        );
    }

    #[test]
    fn parses_target_repo_and_workspace_from_workflow_front_matter() {
        let dir = env::temp_dir().join(format!("shea-target-context-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let workflow = dir.join("WORKFLOW.md");
        fs::write(
            &workflow,
            "---\ntracker:\n  owner: Acme\n  repo: target-app\nworkspace:\n  root: /tmp/acme-target\n---\nPrompt",
        )
        .unwrap();

        let context = TargetContext::from_options(Some(&TargetOptions {
            workflow_path: Some(workflow.display().to_string()),
            repository: None,
            workspace_path: None,
        }));

        assert_eq!(context.repository.as_deref(), Some("Acme/target-app"));
        assert_eq!(context.workspace_path.as_deref(), Some("/tmp/acme-target"));
        assert_eq!(
            context.skills_path.as_deref(),
            Some("/tmp/acme-target/.codex/skills")
        );
    }
}
