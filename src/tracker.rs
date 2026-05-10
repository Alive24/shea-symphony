use std::fs;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::RuntimeConfig;
use crate::model::{normalize_state, LinkedPullRequest, TrackerIssue};

pub trait TrackerAdapter {
    fn kind(&self) -> &'static str;
    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError>;
    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError>;
    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError>;
    fn set_state(&self, issue_ref: &str, normalized_state: &str) -> Result<(), TrackerError>;
    fn upsert_workpad(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError>;
    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError>;
    fn add_issue_to_project(&self, issue_id: &str) -> Result<(), TrackerError>;
    fn link_pull_request(&self, issue_ref: &str, pr_ref: &str) -> Result<(), TrackerError>;
    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError>;
    fn integration_gaps(&self) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUpIssueInput {
    pub title: String,
    pub body: String,
    pub project_id: Option<String>,
    pub related_issue_ref: Option<String>,
    pub blocked_by_issue_ref: Option<String>,
}

#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("tracker fixture failed: {0}")]
    Fixture(String),
    #[error("tracker integration is unavailable: {0}")]
    IntegrationUnavailable(String),
    #[error("tracker operation is not implemented yet: {0}")]
    NotImplemented(String),
}

pub fn adapter_from_config(config: &RuntimeConfig) -> Box<dyn TrackerAdapter> {
    match config.tracker.kind.as_str() {
        "memory" => Box::new(MemoryTracker::from_config(config)),
        "linear" => Box::new(LinearAdapter::new(config.clone())),
        _ => Box::new(GithubProjectV2Adapter::new(config.clone())),
    }
}

#[derive(Debug, Clone)]
pub struct MemoryTracker {
    issues: Vec<TrackerIssue>,
}

impl MemoryTracker {
    pub fn from_config(config: &RuntimeConfig) -> Self {
        let issues = load_fixture(config).unwrap_or_default();
        Self { issues }
    }

    pub fn new(issues: Vec<TrackerIssue>) -> Self {
        Self { issues }
    }
}

impl TrackerAdapter for MemoryTracker {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(self.issues.clone())
    }

    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        Ok(self
            .issues
            .iter()
            .find(|issue| issue.id == issue_ref || issue.identifier == issue_ref)
            .cloned())
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        let wanted: Vec<String> = states.iter().map(|state| normalize_state(state)).collect();
        Ok(self
            .issues
            .iter()
            .filter(|issue| wanted.contains(&issue.normalized_state()))
            .cloned()
            .collect())
    }

    fn set_state(&self, _issue_ref: &str, _normalized_state: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    fn upsert_workpad(&self, _issue_ref: &str, _markdown: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError> {
        Ok(format!("dry-run:{}", input.title))
    }

    fn add_issue_to_project(&self, _issue_id: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    fn link_pull_request(&self, _issue_ref: &str, _pr_ref: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError> {
        Ok(self
            .get_issue(issue_ref)?
            .map(|issue| issue.linked_pull_requests)
            .unwrap_or_default())
    }
}

#[derive(Debug, Clone)]
pub struct GithubProjectV2Adapter {
    config: RuntimeConfig,
    fixture_issues: Vec<TrackerIssue>,
}

impl GithubProjectV2Adapter {
    pub fn new(config: RuntimeConfig) -> Self {
        let fixture_issues = load_fixture(&config).unwrap_or_default();
        Self {
            config,
            fixture_issues,
        }
    }
}

impl TrackerAdapter for GithubProjectV2Adapter {
    fn kind(&self) -> &'static str {
        "github_project_v2"
    }

    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(self.fixture_issues.clone())
    }

    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        Ok(self
            .fixture_issues
            .iter()
            .find(|issue| issue.id == issue_ref || issue.identifier == issue_ref)
            .cloned())
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        MemoryTracker::new(self.fixture_issues.clone()).fetch_issues_by_states(states)
    }

    fn set_state(&self, _issue_ref: &str, _normalized_state: &str) -> Result<(), TrackerError> {
        Err(TrackerError::IntegrationUnavailable(
            "GitHub Project v2 state updates require GraphQL credentials and option ID lookup"
                .into(),
        ))
    }

    fn upsert_workpad(&self, _issue_ref: &str, _markdown: &str) -> Result<(), TrackerError> {
        Err(TrackerError::IntegrationUnavailable(
            "GitHub issue comment workpad upsert requires live GitHub API access".into(),
        ))
    }

    fn create_follow_up_issue(&self, _input: FollowUpIssueInput) -> Result<String, TrackerError> {
        Err(TrackerError::IntegrationUnavailable(
            "GitHub follow-up issue creation requires live GitHub API access".into(),
        ))
    }

    fn add_issue_to_project(&self, _issue_id: &str) -> Result<(), TrackerError> {
        Err(TrackerError::IntegrationUnavailable(
            "GitHub Project v2 item creation requires live GitHub API access".into(),
        ))
    }

    fn link_pull_request(&self, _issue_ref: &str, _pr_ref: &str) -> Result<(), TrackerError> {
        Err(TrackerError::IntegrationUnavailable(
            "GitHub pull request linking requires live GitHub API access".into(),
        ))
    }

    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError> {
        Ok(self
            .get_issue(issue_ref)?
            .map(|issue| issue.linked_pull_requests)
            .unwrap_or_default())
    }

    fn integration_gaps(&self) -> Vec<String> {
        let mut gaps = vec![
            "GitHub Project v2 live GraphQL adapter is not implemented in this slice; using fixtures/dry-run records."
                .to_string(),
        ];
        if self.config.tracker.api_key.is_none() {
            gaps.push(
                "GitHub token not detected; live Project v2 operations would be skipped.".into(),
            );
        }
        gaps
    }
}

#[derive(Debug, Clone)]
pub struct LinearAdapter {
    config: RuntimeConfig,
}

impl LinearAdapter {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }
}

impl TrackerAdapter for LinearAdapter {
    fn kind(&self) -> &'static str {
        "linear"
    }

    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        Ok(load_fixture(&self.config).unwrap_or_default())
    }

    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        Ok(load_fixture(&self.config)?
            .into_iter()
            .find(|issue| issue.id == issue_ref || issue.identifier == issue_ref))
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        MemoryTracker::new(load_fixture(&self.config)?).fetch_issues_by_states(states)
    }

    fn set_state(&self, _issue_ref: &str, _normalized_state: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(
            "Linear live adapter is preserved as a required future adapter".into(),
        ))
    }

    fn upsert_workpad(&self, _issue_ref: &str, _markdown: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(
            "Linear workpad comments are preserved behind the tracker trait".into(),
        ))
    }

    fn create_follow_up_issue(&self, _input: FollowUpIssueInput) -> Result<String, TrackerError> {
        Err(TrackerError::NotImplemented(
            "Linear follow-up creation is preserved behind the tracker trait".into(),
        ))
    }

    fn add_issue_to_project(&self, _issue_id: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(
            "Linear project linking is preserved behind the tracker trait".into(),
        ))
    }

    fn link_pull_request(&self, _issue_ref: &str, _pr_ref: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(
            "Linear PR attachment support is preserved behind the tracker trait".into(),
        ))
    }

    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError> {
        Ok(self
            .get_issue(issue_ref)?
            .map(|issue| issue.linked_pull_requests)
            .unwrap_or_default())
    }

    fn integration_gaps(&self) -> Vec<String> {
        vec!["Linear live adapter is intentionally stubbed in the GitHub-first slice.".into()]
    }
}

fn load_fixture(config: &RuntimeConfig) -> Result<Vec<TrackerIssue>, TrackerError> {
    let Some(path) = config.tracker.fixture_path.as_ref() else {
        return Ok(Vec::new());
    };

    let content = fs::read_to_string(path)
        .map_err(|error| TrackerError::Fixture(format!("{}: {error}", path.display())))?;
    serde_json::from_str(&content)
        .map_err(|error| TrackerError::Fixture(format!("{}: {error}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(state: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "id".into(),
            item_id: None,
            identifier: "#1".into(),
            title: "Title".into(),
            description: None,
            url: None,
            state: state.into(),
            labels: vec![],
            assignees: vec![],
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn memory_tracker_filters_by_state() {
        let tracker = MemoryTracker::new(vec![issue("Todo"), issue("Done")]);
        let found = tracker.fetch_issues_by_states(&["todo".into()]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].state, "Todo");
    }
}
