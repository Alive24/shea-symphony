use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowDefinition {
    pub path: PathBuf,
    pub config: Value,
    pub workflow_index: String,
    pub prompt_template: String,
    pub lane_prompts: LanePromptTemplates,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanePromptTemplates {
    pub main_agent: String,
    pub review_agent: String,
    pub merge_agent: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentLane {
    MainAgent,
    ReviewAgent,
    MergeAgent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowStore {
    path: PathBuf,
    active: WorkflowDefinition,
    last_reload_error: Option<String>,
}

#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("missing WORKFLOW.md at {path}: {source}")]
    MissingWorkflowFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse WORKFLOW.md front matter: {0}")]
    Parse(#[from] serde_yaml::Error),
    #[error("workflow front matter must decode to a map/object")]
    FrontMatterNotMap,
    #[error("invalid lane prompt configuration: {0}")]
    InvalidLanePromptConfig(String),
    #[error("missing {lane} prompt at {path}: {source}")]
    MissingLanePrompt {
        lane: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl WorkflowDefinition {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, WorkflowError> {
        let path = path.as_ref();
        let content =
            fs::read_to_string(path).map_err(|source| WorkflowError::MissingWorkflowFile {
                path: path.to_path_buf(),
                source,
            })?;
        Self::parse(path, &content)
    }

    pub fn parse(path: impl AsRef<Path>, content: &str) -> Result<Self, WorkflowError> {
        let (front_matter, prompt) = split_front_matter(content);
        let config = parse_front_matter(&front_matter)?;
        let workflow_index = prompt.trim().to_string();
        let lane_prompts = load_lane_prompts(path.as_ref(), &config, &workflow_index)?;

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            config,
            prompt_template: lane_prompts.main_agent.clone(),
            workflow_index,
            lane_prompts,
        })
    }

    pub fn prompt_for_lane(&self, lane: AgentLane) -> &str {
        match lane {
            AgentLane::MainAgent => &self.lane_prompts.main_agent,
            AgentLane::ReviewAgent => &self.lane_prompts.review_agent,
            AgentLane::MergeAgent => &self.lane_prompts.merge_agent,
        }
    }
}

impl WorkflowStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, WorkflowError> {
        let path = path.as_ref().to_path_buf();
        let active = WorkflowDefinition::load(&path)?;

        Ok(Self {
            path,
            active,
            last_reload_error: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn active(&self) -> &WorkflowDefinition {
        &self.active
    }

    pub fn last_reload_error(&self) -> Option<&str> {
        self.last_reload_error.as_deref()
    }

    pub fn reload(&mut self) -> Result<&WorkflowDefinition, WorkflowError> {
        match WorkflowDefinition::load(&self.path) {
            Ok(workflow) => {
                self.active = workflow;
                self.last_reload_error = None;
                Ok(&self.active)
            }
            Err(error) => {
                self.last_reload_error = Some(error.to_string());
                Err(error)
            }
        }
    }
}

fn split_front_matter(content: &str) -> (String, String) {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");

    if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some(prompt) = rest.strip_prefix("---\n") {
            (String::new(), prompt.to_string())
        } else if rest == "---" {
            (String::new(), String::new())
        } else if let Some(end) = rest.find("\n---") {
            let (front, after_front) = rest.split_at(end);
            let prompt = after_front
                .strip_prefix("\n---\n")
                .or_else(|| after_front.strip_prefix("\n---"))
                .unwrap_or("");
            (front.to_string(), prompt.to_string())
        } else {
            (rest.to_string(), String::new())
        }
    } else if normalized.trim() == "---" {
        (String::new(), String::new())
    } else {
        (String::new(), normalized)
    }
}

fn parse_front_matter(front_matter: &str) -> Result<Value, WorkflowError> {
    if front_matter.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }

    let value: Value = serde_yaml::from_str(front_matter)?;

    if value.is_object() {
        Ok(value)
    } else {
        Err(WorkflowError::FrontMatterNotMap)
    }
}

fn load_lane_prompts(
    workflow_path: &Path,
    config: &Value,
    inline_prompt: &str,
) -> Result<LanePromptTemplates, WorkflowError> {
    let Some(prompt_config) = config.get("prompts") else {
        return Ok(LanePromptTemplates {
            main_agent: inline_prompt.to_string(),
            review_agent: inline_prompt.to_string(),
            merge_agent: inline_prompt.to_string(),
        });
    };

    let prompt_config = prompt_config.as_object().ok_or_else(|| {
        WorkflowError::InvalidLanePromptConfig("prompts must be a map/object".into())
    })?;

    Ok(LanePromptTemplates {
        main_agent: read_lane_prompt(workflow_path, prompt_config, "main_agent")?,
        review_agent: read_lane_prompt(workflow_path, prompt_config, "review_agent")?,
        merge_agent: read_lane_prompt(workflow_path, prompt_config, "merge_agent")?,
    })
}

fn read_lane_prompt(
    workflow_path: &Path,
    prompt_config: &serde_json::Map<String, Value>,
    lane: &'static str,
) -> Result<String, WorkflowError> {
    let relative_path = prompt_config
        .get(lane)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            WorkflowError::InvalidLanePromptConfig(format!(
                "prompts.{lane} is required when prompts is configured"
            ))
        })?;
    let path = resolve_workflow_relative_path(workflow_path, relative_path);
    fs::read_to_string(&path)
        .map(|content| content.trim().to_string())
        .map_err(|source| WorkflowError::MissingLanePrompt { lane, path, source })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackerIssue;
    use crate::prompt::render_prompt;

    #[test]
    fn parses_front_matter_and_prompt() {
        let workflow = WorkflowDefinition::parse(
            "WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n---\nHello {{ issue.identifier }}\n",
        )
        .unwrap();

        assert_eq!(workflow.config["tracker"]["kind"], "memory");
        assert_eq!(workflow.prompt_template, "Hello {{ issue.identifier }}");
    }

    #[test]
    fn treats_missing_front_matter_as_prompt() {
        let workflow = WorkflowDefinition::parse("WORKFLOW.md", "Only prompt").unwrap();
        assert!(workflow.config.as_object().unwrap().is_empty());
        assert_eq!(workflow.prompt_template, "Only prompt");
        assert_eq!(
            workflow.prompt_for_lane(AgentLane::MainAgent),
            "Only prompt"
        );
        assert_eq!(
            workflow.prompt_for_lane(AgentLane::ReviewAgent),
            "Only prompt"
        );
        assert_eq!(
            workflow.prompt_for_lane(AgentLane::MergeAgent),
            "Only prompt"
        );
    }

    #[test]
    fn loads_lane_prompts_from_workflow_relative_paths() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        fs::create_dir(temp.path().join("prompts")).unwrap();
        fs::write(
            temp.path().join("prompts/main.md"),
            "Main {{ issue.identifier }}",
        )
        .unwrap();
        fs::write(
            temp.path().join("prompts/review.md"),
            "Review {{ issue.identifier }}",
        )
        .unwrap();
        fs::write(
            temp.path().join("prompts/merge.md"),
            "Merge {{ issue.identifier }}",
        )
        .unwrap();

        let workflow = WorkflowDefinition::parse(
            &workflow_path,
            "---\nprompts:\n  main_agent: prompts/main.md\n  review_agent: prompts/review.md\n  merge_agent: prompts/merge.md\n---\nWorkflow index",
        )
        .unwrap();

        assert_eq!(workflow.workflow_index, "Workflow index");
        assert_eq!(
            workflow.prompt_for_lane(AgentLane::MainAgent),
            "Main {{ issue.identifier }}"
        );
        assert_eq!(
            workflow.prompt_for_lane(AgentLane::ReviewAgent),
            "Review {{ issue.identifier }}"
        );
        assert_eq!(
            workflow.prompt_for_lane(AgentLane::MergeAgent),
            "Merge {{ issue.identifier }}"
        );
        assert_eq!(workflow.prompt_template, "Main {{ issue.identifier }}");
    }

    #[test]
    fn lane_prompt_config_requires_all_lanes_when_configured() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        fs::create_dir(temp.path().join("prompts")).unwrap();
        fs::write(temp.path().join("prompts/main.md"), "Main").unwrap();

        let error = WorkflowDefinition::parse(
            &workflow_path,
            "---\nprompts:\n  main_agent: prompts/main.md\n---\nWorkflow index",
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("prompts.review_agent is required"));
    }

    #[test]
    fn workflow_store_successful_reload_replaces_active_workflow() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("WORKFLOW.md");
        fs::write(&path, "---\ntracker:\n  kind: memory\n---\nOld prompt").unwrap();

        let mut store = WorkflowStore::load(&path).unwrap();
        assert_eq!(store.path(), path.as_path());
        assert_eq!(store.active().prompt_template, "Old prompt");

        fs::write(&path, "---\ntracker:\n  kind: memory\n---\nNew prompt").unwrap();

        let reloaded = store.reload().unwrap();
        assert_eq!(reloaded.prompt_template, "New prompt");
        assert_eq!(store.active().prompt_template, "New prompt");
        assert_eq!(store.last_reload_error(), None);
    }

    #[test]
    fn workflow_store_failed_reload_keeps_last_known_good_workflow() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("WORKFLOW.md");
        fs::write(&path, "---\ntracker:\n  kind: memory\n---\nStable prompt").unwrap();

        let mut store = WorkflowStore::load(&path).unwrap();

        fs::write(&path, "---\ntracker: [").unwrap();

        let error = store.reload().unwrap_err();
        assert!(matches!(error, WorkflowError::Parse(_)));
        assert_eq!(store.active().prompt_template, "Stable prompt");
        assert!(store
            .last_reload_error()
            .unwrap()
            .contains("failed to parse WORKFLOW.md front matter"));
    }

    fn fixture_issue() -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "github_project_v2".into(),
            id: "ISSUE_48".into(),
            item_id: Some("PROJECT_ITEM_48".into()),
            identifier: "#48".into(),
            title: "Replace placeholder dogfood workflow with real Jade Symphony prompt".into(),
            description: Some(
                [
                    "## Issue Goal",
                    "Replace the placeholder live workflow prompt.",
                    "## Verification",
                    "- cargo test",
                ]
                .join("\n"),
            ),
            url: Some("https://github.com/Alive24/jade-symphony/issues/48".into()),
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

    fn github_project_workflow() -> WorkflowDefinition {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("workflows/jade-symphony.md");
        WorkflowDefinition::load(path).unwrap()
    }

    #[test]
    fn github_project_workflow_prompt_is_not_placeholder_thin() {
        let workflow = github_project_workflow();

        assert!(workflow.workflow_index.contains("Workflow Index"));
        assert!(workflow.prompt_template.len() > 3_000);
        assert!(workflow.prompt_template.contains("## Operating Loop"));
        assert!(workflow.prompt_template.contains("## Workpad Discipline"));
        assert!(workflow.prompt_template.contains("Issue Quality Gate"));
        assert!(workflow.prompt_template.contains("one issue per branch"));
        assert!(workflow
            .prompt_template
            .contains("The main implementation agent must never set `Human Review`."));
        assert!(!workflow
            .prompt_template
            .contains("stop before\nmaking live agent changes"));
        assert!(workflow
            .prompt_for_lane(AgentLane::ReviewAgent)
            .contains("independent Review Agent"));
        assert!(workflow
            .prompt_for_lane(AgentLane::MergeAgent)
            .contains("Merge Agent"));
    }

    #[test]
    fn github_project_workflow_prompt_renders_for_fixture_issue() {
        let workflow = github_project_workflow();
        let rendered = render_prompt(&workflow.prompt_template, &fixture_issue(), Some(2)).unwrap();

        assert!(rendered.contains("Jade Symphony issue #48"));
        assert!(rendered.contains("Replace placeholder dogfood workflow"));
        assert!(rendered.contains("This is attempt 2."));
        assert!(rendered.contains("## Issue Goal\nReplace the placeholder live workflow prompt."));
        assert!(rendered.contains("Move locally complete main-agent work to `Agent Review` only."));
        assert!(rendered.contains("Do not set `Human Review`."));
        assert!(!rendered.contains("{{"));
        assert!(!rendered.contains("{%"));
    }
}
