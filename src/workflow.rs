use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowDefinition {
    pub path: PathBuf,
    pub config: Value,
    pub prompt_template: String,
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

        Ok(Self {
            path: path.as_ref().to_path_buf(),
            config,
            prompt_template: prompt.trim().to_string(),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
