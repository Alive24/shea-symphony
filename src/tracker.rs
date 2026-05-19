use std::collections::BTreeMap;
#[cfg(test)]
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{AssigneeFilter, RuntimeConfig};
use crate::model::{normalize_state, BlockerRef, LinkedPullRequest, TrackerIssue};

pub trait TrackerAdapter {
    fn kind(&self) -> &'static str;
    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError>;
    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError>;
    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError>;
    fn set_state(&self, issue_ref: &str, normalized_state: &str) -> Result<(), TrackerError>;
    fn upsert_workpad(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError>;
    fn update_issue_content(
        &self,
        _issue_ref: &str,
        _title: &str,
        _body: &str,
    ) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support issue content updates",
            self.kind()
        )))
    }
    fn add_issue_comment(&self, _issue_ref: &str, _markdown: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support issue comments",
            self.kind()
        )))
    }
    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError>;
    fn add_issue_to_project(&self, issue_id: &str) -> Result<(), TrackerError>;
    fn add_issue_to_project_with_state(
        &self,
        issue_id: &str,
        normalized_state: &str,
    ) -> Result<(), TrackerError>;
    fn set_project_field(
        &self,
        _issue_ref: &str,
        _assignment: &ProjectFieldAssignment,
    ) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support Project field assignment",
            self.kind()
        )))
    }
    fn clear_project_field(&self, _issue_ref: &str, _field_name: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support Project field clearing",
            self.kind()
        )))
    }
    fn link_pull_request(&self, issue_ref: &str, pr_ref: &str) -> Result<(), TrackerError>;
    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError>;
    fn close_issue(&self, _issue_ref: &str) -> Result<(), TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support issue closure",
            self.kind()
        )))
    }
    fn integration_gaps(&self) -> Vec<String> {
        Vec::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFieldAssignment {
    pub name: String,
    pub value: String,
}

impl ProjectFieldAssignment {
    pub fn parse(raw: &str) -> Result<Self, TrackerError> {
        let Some((name, value)) = raw.split_once('=') else {
            return Err(TrackerError::Payload(format!(
                "Project field assignment {raw:?} must use NAME=VALUE"
            )));
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            return Err(TrackerError::Payload(format!(
                "Project field assignment {raw:?} must include non-empty name and value"
            )));
        }

        Ok(Self {
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FollowUpIssueInput {
    pub title: String,
    pub body: String,
    pub assignees: Vec<String>,
    pub project_id: Option<String>,
    pub related_issue_ref: Option<String>,
    pub blocked_by_issue_ref: Option<String>,
}

#[derive(Debug, Error)]
pub enum TrackerError {
    #[error("tracker fixture failed: {0}")]
    Fixture(String),
    #[error("tracker payload failed: {0}")]
    Payload(String),
    #[error("tracker integration is unavailable: {0}")]
    IntegrationUnavailable(String),
    #[error("tracker operation is not implemented yet: {0}")]
    NotImplemented(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectStateFailureKind {
    Auth,
    Network,
    RateLimit,
    ResourceLimit,
    Schema,
    PartialResponse,
    Payload,
    MissingCapability,
    Unknown,
}

impl ProjectStateFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Network => "network",
            Self::RateLimit => "rate_limit",
            Self::ResourceLimit => "resource_limit",
            Self::Schema => "schema",
            Self::PartialResponse => "partial_response",
            Self::Payload => "payload",
            Self::MissingCapability => "missing_capability",
            Self::Unknown => "unknown",
        }
    }
}

pub fn classify_project_state_error(error: &TrackerError) -> ProjectStateFailureKind {
    match error {
        TrackerError::Fixture(_) => ProjectStateFailureKind::Payload,
        TrackerError::Payload(message) => classify_project_state_failure_message(message),
        TrackerError::IntegrationUnavailable(message) => {
            classify_project_state_failure_message(message)
        }
        TrackerError::NotImplemented(_) => ProjectStateFailureKind::MissingCapability,
    }
}

pub fn classify_project_state_failure_message(message: &str) -> ProjectStateFailureKind {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("rate limit")
        || normalized.contains("secondary rate")
        || normalized.contains("too many requests")
        || normalized.contains("http 429")
    {
        ProjectStateFailureKind::RateLimit
    } else if normalized.contains("resource limit")
        || normalized.contains("resource limitation")
        || normalized.contains("maximum node limit")
        || normalized.contains("max node limit")
        || normalized.contains("exceeds maximum")
        || normalized.contains("query has complexity")
        || normalized.contains("query is too complex")
    {
        ProjectStateFailureKind::ResourceLimit
    } else if normalized.contains("authentication")
        || normalized.contains("authenticate")
        || normalized.contains("auth login")
        || normalized.contains("bad credentials")
        || normalized.contains("unauthorized")
        || normalized.contains("http 401")
        || normalized.contains("http 403")
    {
        ProjectStateFailureKind::Auth
    } else if normalized.contains("could not resolve host")
        || normalized.contains("failed to connect")
        || normalized.contains("connection timed out")
        || normalized.contains("connection reset")
        || normalized.contains("network")
        || normalized.contains("tls")
    {
        ProjectStateFailureKind::Network
    } else if normalized.contains("missing projectv2")
        || normalized.contains("partial projectv2")
        || normalized.contains("missing status field")
        || normalized.contains("missing fieldvalues")
        || normalized.contains("missing pageinfo")
    {
        ProjectStateFailureKind::PartialResponse
    } else if normalized.contains("could not resolve to a projectv2")
        || normalized.contains("field ")
        || normalized.contains("doesn't exist")
        || normalized.contains("schema")
    {
        ProjectStateFailureKind::Schema
    } else if normalized.contains("invalid gh graphql json")
        || normalized.contains("invalid github graphql json")
        || normalized.contains("invalid gh api json")
        || normalized.contains("invalid github api json")
    {
        ProjectStateFailureKind::Payload
    } else if normalized.contains("does not support")
        || normalized.contains("not implemented")
        || normalized.contains("missing cli capability")
        || normalized.contains("cli gap")
    {
        ProjectStateFailureKind::MissingCapability
    } else {
        ProjectStateFailureKind::Unknown
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimDecision {
    AlreadyInProgress,
    Claimable,
    StopAndReplan { current_state: String },
}

pub fn claim_decision(issue: &TrackerIssue, config: &RuntimeConfig) -> ClaimDecision {
    let state = tracker_state_key(&issue.state);
    let state_map = &config.tracker.state_map;

    if state == tracker_state_key(&state_map.in_progress) {
        ClaimDecision::AlreadyInProgress
    } else if state == tracker_state_key(&state_map.todo)
        || state == tracker_state_key(&state_map.rework)
    {
        ClaimDecision::Claimable
    } else {
        ClaimDecision::StopAndReplan {
            current_state: issue.state.clone(),
        }
    }
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

    fn update_issue_content(
        &self,
        _issue_ref: &str,
        _title: &str,
        _body: &str,
    ) -> Result<(), TrackerError> {
        Ok(())
    }

    fn add_issue_comment(&self, _issue_ref: &str, _markdown: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError> {
        Ok(format!("dry-run:{}", input.title))
    }

    fn add_issue_to_project(&self, _issue_id: &str) -> Result<(), TrackerError> {
        Ok(())
    }

    fn add_issue_to_project_with_state(
        &self,
        _issue_id: &str,
        _normalized_state: &str,
    ) -> Result<(), TrackerError> {
        Ok(())
    }

    fn set_project_field(
        &self,
        _issue_ref: &str,
        _assignment: &ProjectFieldAssignment,
    ) -> Result<(), TrackerError> {
        Ok(())
    }

    fn clear_project_field(&self, _issue_ref: &str, _field_name: &str) -> Result<(), TrackerError> {
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

    fn close_issue(&self, _issue_ref: &str) -> Result<(), TrackerError> {
        Ok(())
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

    fn load_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        let mut issues = apply_github_read_filters(self.load_mapped_issues()?, &self.config);
        self.enrich_native_issue_blockers_for_claimable_issues(&mut issues)?;
        Ok(issues)
    }

    fn load_mapped_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        let issues =
            if !self.fixture_issues.is_empty() || self.config.tracker.fixture_path.is_some() {
                self.fixture_issues.clone()
            } else {
                GithubProjectV2GhClient::new(&self.config).fetch_project_issues()?
            };

        Ok(apply_github_status_filters(issues, &self.config))
    }

    fn enrich_native_issue_blockers_for_claimable_issues(
        &self,
        issues: &mut [TrackerIssue],
    ) -> Result<(), TrackerError> {
        if !self.fixture_issues.is_empty() || self.config.tracker.fixture_path.is_some() {
            return Ok(());
        }

        let client = GithubProjectV2GhClient::new(&self.config);
        for issue in issues
            .iter_mut()
            .filter(|issue| github_issue_needs_native_blocker_prefetch(issue, &self.config))
        {
            client.enrich_native_issue_blockers(std::slice::from_mut(issue))?;
        }

        Ok(())
    }
}

fn apply_github_status_filters(
    issues: Vec<TrackerIssue>,
    config: &RuntimeConfig,
) -> Vec<TrackerIssue> {
    issues
        .into_iter()
        .filter(|issue| status_is_mapped(&issue.state, config))
        .collect()
}

fn apply_github_read_filters(
    issues: Vec<TrackerIssue>,
    config: &RuntimeConfig,
) -> Vec<TrackerIssue> {
    apply_github_status_filters(issues, config)
        .into_iter()
        .filter(|issue| issue_matches_assignee_filter(issue, &config.tracker.assignee_filter))
        .collect()
}

fn issue_matches_assignee_filter(issue: &TrackerIssue, filter: &AssigneeFilter) -> bool {
    if issue.assignees.is_empty() {
        return filter.allow_unassigned;
    }

    if filter.assignees.is_empty() {
        return true;
    }

    let allowed: Vec<String> = filter
        .assignees
        .iter()
        .map(|assignee| normalize_state(assignee))
        .collect();

    issue
        .assignees
        .iter()
        .any(|assignee| allowed.contains(&normalize_state(assignee)))
}

fn github_issue_needs_native_blocker_prefetch(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
) -> bool {
    let state = tracker_state_key(&issue.state);
    state == tracker_state_key(&config.tracker.state_map.todo)
        || state == tracker_state_key(&config.tracker.state_map.rework)
}

fn status_is_mapped(status: &str, config: &RuntimeConfig) -> bool {
    mapped_status_names(config)
        .iter()
        .any(|mapped| tracker_state_key(mapped) == tracker_state_key(status))
}

fn mapped_status_names(config: &RuntimeConfig) -> Vec<&str> {
    let state_map = &config.tracker.state_map;
    vec![
        state_map.backlog.as_str(),
        state_map.todo.as_str(),
        state_map.need_to_clarify.as_str(),
        state_map.in_progress.as_str(),
        state_map.need_human_input.as_str(),
        state_map.agent_review.as_str(),
        state_map.human_review.as_str(),
        state_map.rework.as_str(),
        state_map.merging.as_str(),
        state_map.done.as_str(),
    ]
}

impl TrackerAdapter for GithubProjectV2Adapter {
    fn kind(&self) -> &'static str {
        "github_project_v2"
    }

    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.load_issues()
    }

    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        let mut issue = self
            .load_mapped_issues()?
            .into_iter()
            .find(|issue| issue.id == issue_ref || issue.identifier == issue_ref)
            .map(|mut issue| {
                if self.fixture_issues.is_empty() && self.config.tracker.fixture_path.is_none() {
                    GithubProjectV2GhClient::new(&self.config)
                        .enrich_native_issue_blockers(std::slice::from_mut(&mut issue))?;
                }
                Ok(issue)
            })
            .transpose()?;

        Ok(issue.take())
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        let mut issues =
            MemoryTracker::new(self.load_mapped_issues()?).fetch_issues_by_states(states)?;
        self.enrich_native_issue_blockers_for_claimable_issues(&mut issues)?;
        Ok(issues)
    }

    fn set_state(&self, issue_ref: &str, normalized_state: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot update live status".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).set_state(issue_ref, normalized_state)
    }

    fn upsert_workpad(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot upsert live workpads".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).upsert_workpad(issue_ref, markdown)
    }

    fn update_issue_content(
        &self,
        issue_ref: &str,
        title: &str,
        body: &str,
    ) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot update live issue content".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).update_issue_content(issue_ref, title, body)
    }

    fn add_issue_comment(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot add live issue comments".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).add_issue_comment(issue_ref, markdown)
    }

    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot create live follow-up issues".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).create_follow_up_issue(input)
    }

    fn add_issue_to_project(&self, issue_id: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot add live project items".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).add_issue_to_project(issue_id)
    }

    fn add_issue_to_project_with_state(
        &self,
        issue_id: &str,
        normalized_state: &str,
    ) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot add live project items".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config)
            .add_issue_to_project_with_state(issue_id, normalized_state)
    }

    fn set_project_field(
        &self,
        issue_ref: &str,
        assignment: &ProjectFieldAssignment,
    ) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot update live project fields".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).set_project_field(issue_ref, assignment)
    }

    fn clear_project_field(&self, issue_ref: &str, field_name: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot clear live project fields".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).clear_project_field(issue_ref, field_name)
    }

    fn link_pull_request(&self, issue_ref: &str, pr_ref: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot link live pull requests".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config).link_pull_request(issue_ref, pr_ref)
    }

    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Ok(self
                .get_issue(issue_ref)?
                .map(|issue| issue.linked_pull_requests)
                .unwrap_or_default());
        }

        GithubProjectV2GhClient::new(&self.config).list_linked_pull_requests(issue_ref)
    }

    fn close_issue(&self, issue_ref: &str) -> Result<(), TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Ok(());
        }

        GithubProjectV2GhClient::new(&self.config).close_issue(issue_ref)
    }

    fn integration_gaps(&self) -> Vec<String> {
        let mut gaps = Vec::new();

        if self.config.tracker.fixture_path.is_some() {
            gaps.push(
                "GitHub Project v2 is using fixture issues because tracker.fixture_path is set."
                    .to_string(),
            );
        }

        if let Some(gap) = github_auth_gap(github_auth_mode(
            &self.config,
            gh_available(),
            github_graphql_auth_smoke,
        )) {
            gaps.push(gap);
        }

        gaps.push("GitHub Project v2 PR linking still uses an issue comment/autolink strategy rather than a first-class relationship.".into());
        gaps.push("GitHub Project v2 live write methods use `gh api graphql`; keep using `--write` for mutating CLI commands.".into());
        gaps
    }
}

#[derive(Debug, Clone)]
struct GithubProjectV2GhClient {
    config: RuntimeConfig,
}

impl GithubProjectV2GhClient {
    fn new(config: &RuntimeConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    fn fetch_project_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        if !gh_available() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 live reads require the `gh` CLI on PATH".into(),
            ));
        }

        let metadata = self.project_metadata()?;
        if metadata.status_options.is_empty() {
            return Err(TrackerError::Payload(format!(
                "ProjectV2 field {:?} is not a single-select field or has no options",
                self.config.tracker.status_field
            )));
        }

        let mut issues = Vec::new();
        let mut cursor = None;

        loop {
            let response = self.graphql_project_page(true, cursor.as_deref()).or_else(|org_error| {
                self.graphql_project_page(false, cursor.as_deref())
                    .map_err(|user_error| {
                        TrackerError::IntegrationUnavailable(format!(
                            "failed to query ProjectV2 as organization or user: org={org_error}; user={user_error}"
                        ))
                    })
            })?;

            let (mut page_issues, next_cursor, has_next_page) =
                issues_from_project_response(&response, &self.config)?;
            issues.append(&mut page_issues);

            if has_next_page {
                cursor = next_cursor;
            } else {
                break;
            }
        }

        Ok(issues)
    }

    fn enrich_native_issue_blockers(
        &self,
        issues: &mut [TrackerIssue],
    ) -> Result<(), TrackerError> {
        for issue in issues {
            let Some(number) = github_issue_number(&issue.identifier) else {
                continue;
            };
            let native_blockers = self.fetch_native_issue_blockers(number)?;
            merge_blocker_refs(&mut issue.blocked_by, native_blockers);
        }

        Ok(())
    }

    fn fetch_native_issue_blockers(
        &self,
        issue_number: u64,
    ) -> Result<Vec<BlockerRef>, TrackerError> {
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;
        let response = run_gh_api_json(vec![
            "api".into(),
            format!("repos/{owner}/{repo}/issues/{issue_number}/dependencies/blocked_by"),
        ])?;

        github_native_blocker_refs_from_response(&response, issue_number)
    }

    fn set_state(&self, issue_ref: &str, normalized_state: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let option_name = self.state_option_name(normalized_state)?;
        if !status_update_required(&issue, &option_name) {
            return Ok(());
        }

        let item_id = issue.item_id.ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "issue {issue_ref} has no ProjectV2 item id"
            ))
        })?;
        let metadata = self.project_metadata()?;
        let option_id =
            status_option_id(&metadata, &option_name, &self.config.tracker.status_field)?;

        self.graphql(
            GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
            &[
                ("projectId", metadata.project_id),
                ("itemId", item_id),
                ("fieldId", metadata.status_field_id),
                ("optionId", option_id),
            ],
        )?;
        Ok(())
    }

    fn upsert_workpad(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let marker = &self.config.tracker.workpad.marker;
        let comments = self.find_workpad_comments(&issue.id, marker)?;
        let body = ensure_workpad_marker(markdown, marker);

        if let Some((comment_id, existing_body)) = comments.first() {
            let body = merge_workpad_body(existing_body, &body, marker);
            self.graphql(
                GITHUB_UPDATE_ISSUE_COMMENT_MUTATION,
                &[("commentId", comment_id.clone()), ("body", body)],
            )?;
        } else {
            self.graphql(
                GITHUB_ADD_COMMENT_MUTATION,
                &[("subjectId", issue.id), ("body", body)],
            )?;
        }

        for (duplicate_id, _) in comments.iter().skip(1) {
            self.graphql(
                GITHUB_UPDATE_ISSUE_COMMENT_MUTATION,
                &[
                    ("commentId", duplicate_id.clone()),
                    ("body", duplicate_workpad_body(marker)),
                ],
            )?;
        }

        Ok(())
    }

    fn add_issue_comment(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        self.graphql(
            GITHUB_ADD_COMMENT_MUTATION,
            &[("subjectId", issue.id), ("body", markdown.to_string())],
        )?;
        Ok(())
    }

    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError> {
        let repository_id = self.repository_id()?;
        let body = follow_up_issue_body(&input);
        let response = self.graphql(
            GITHUB_CREATE_ISSUE_MUTATION,
            &[
                ("repositoryId", repository_id),
                ("title", input.title),
                ("body", body),
            ],
        )?;
        let id = response
            .pointer("/data/createIssue/issue/id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TrackerError::Payload("createIssue response missing issue id".into()))?;
        let number = response
            .pointer("/data/createIssue/issue/number")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                TrackerError::Payload("createIssue response missing issue number".into())
            })?;
        self.assign_issue(number, &input.assignees)?;
        Ok(id.to_string())
    }

    fn assign_issue(&self, number: u64, assignees: &[String]) -> Result<(), TrackerError> {
        if assignees.is_empty() {
            return Ok(());
        }
        if !gh_available() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub issue assignment requires the `gh` CLI on PATH".into(),
            ));
        }
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;

        for assignee in assignees {
            GithubCliAccess::run_status(
                vec![
                    "issue".into(),
                    "edit".into(),
                    number.to_string(),
                    "--repo".into(),
                    format!("{owner}/{repo}"),
                    "--add-assignee".into(),
                    assignee.clone(),
                ],
                "GitHub issue assignment",
            )?;
        }

        Ok(())
    }

    fn update_issue_content(
        &self,
        issue_ref: &str,
        title: &str,
        body: &str,
    ) -> Result<(), TrackerError> {
        if !gh_available() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub issue editing requires the `gh` CLI on PATH".into(),
            ));
        }
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.owner is required".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::Payload("tracker.repo is required".into()))?;
        let number = github_issue_number(issue_ref).ok_or_else(|| {
            TrackerError::Payload(format!(
                "issue ref {issue_ref:?} is not a GitHub issue number"
            ))
        })?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let body_path =
            std::env::temp_dir().join(format!("jade-symphony-issue-body-{number}-{nonce}.md"));
        fs::write(&body_path, body)
            .map_err(|error| TrackerError::IntegrationUnavailable(error.to_string()))?;
        let result = GithubCliAccess::run_status(
            vec![
                "issue".into(),
                "edit".into(),
                number.to_string(),
                "--repo".into(),
                format!("{owner}/{repo}"),
                "--title".into(),
                title.to_string(),
                "--body-file".into(),
                body_path.to_string_lossy().to_string(),
            ],
            "GitHub issue edit",
        );
        let _ = fs::remove_file(&body_path);
        result?;

        Ok(())
    }

    fn add_issue_to_project(&self, issue_id: &str) -> Result<(), TrackerError> {
        self.add_issue_to_project_with_state(issue_id, "todo")
    }

    fn add_issue_to_project_with_state(
        &self,
        issue_id: &str,
        normalized_state: &str,
    ) -> Result<(), TrackerError> {
        let metadata = self.project_metadata()?;
        let option_name = self.state_option_name(normalized_state)?;
        let option_id =
            status_option_id(&metadata, &option_name, &self.config.tracker.status_field)?;
        let response = self.graphql(
            GITHUB_ADD_PROJECT_ITEM_MUTATION,
            &[
                ("projectId", metadata.project_id.clone()),
                ("contentId", issue_id.to_string()),
            ],
        )?;
        let item_id = project_item_id_from_add_response(&response)?;
        self.graphql(
            GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
            &[
                ("projectId", metadata.project_id),
                ("itemId", item_id),
                ("fieldId", metadata.status_field_id),
                ("optionId", option_id),
            ],
        )?;
        Ok(())
    }

    fn set_project_field(
        &self,
        issue_ref: &str,
        assignment: &ProjectFieldAssignment,
    ) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let item_id = issue.item_id.ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "issue {issue_ref} is not a ProjectV2 item; add it to the project before setting fields"
            ))
        })?;
        let metadata = self.project_metadata()?;
        let field = metadata.field(&assignment.name).ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "ProjectV2 field {:?} was not found",
                assignment.name
            ))
        })?;
        let project_id = metadata.project_id.clone();
        let field_id = field.id.clone();
        match field.kind {
            ProjectFieldKind::SingleSelect => {
                let option_id = field.option_id(&assignment.value).ok_or_else(|| {
                    TrackerError::IntegrationUnavailable(format!(
                        "option {:?} was not found in ProjectV2 field {:?}",
                        assignment.value, assignment.name
                    ))
                })?;
                self.graphql(
                    GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
                    &[
                        ("projectId", project_id),
                        ("itemId", item_id),
                        ("fieldId", field_id),
                        ("optionId", option_id),
                    ],
                )?;
            }
            ProjectFieldKind::Text => {
                self.graphql(
                    GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
                    &[
                        ("projectId", project_id),
                        ("itemId", item_id),
                        ("fieldId", field_id),
                        ("text", assignment.value.clone()),
                    ],
                )?;
            }
            _ => {
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "ProjectV2 field {:?} is {:?}; only single-select and text field assignment are currently supported",
                    assignment.name, field.kind
                )));
            }
        }
        Ok(())
    }

    fn clear_project_field(&self, issue_ref: &str, field_name: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let item_id = issue.item_id.ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "issue {issue_ref} is not a ProjectV2 item; add it to the project before clearing fields"
            ))
        })?;
        let metadata = self.project_metadata()?;
        let field = metadata.field(field_name).ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "ProjectV2 field {field_name:?} was not found"
            ))
        })?;
        let project_id = metadata.project_id.clone();
        let field_id = field.id.clone();
        self.graphql(
            GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION,
            &[
                ("projectId", project_id),
                ("itemId", item_id),
                ("fieldId", field_id),
            ],
        )?;
        Ok(())
    }

    fn resolve_issue(&self, issue_ref: &str) -> Result<TrackerIssue, TrackerError> {
        self.fetch_project_issues()?
            .into_iter()
            .find(|issue| issue.id == issue_ref || issue.identifier == issue_ref)
            .ok_or_else(|| {
                TrackerError::IntegrationUnavailable(format!(
                    "issue {issue_ref} was not found in configured ProjectV2"
                ))
            })
    }

    fn state_option_name(&self, normalized_state: &str) -> Result<String, TrackerError> {
        let state_map = &self.config.tracker.state_map;
        let option = match normalized_state {
            "backlog" => &state_map.backlog,
            "todo" => &state_map.todo,
            "need_to_clarify" | "need to clarify" => &state_map.need_to_clarify,
            "in_progress" | "in progress" => &state_map.in_progress,
            "need_human_input" | "need human input" => &state_map.need_human_input,
            "agent_review" | "agent review" => &state_map.agent_review,
            "human_review" | "human review" => &state_map.human_review,
            "rework" => &state_map.rework,
            "merging" => &state_map.merging,
            "done" => &state_map.done,
            other => {
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "unsupported normalized state {other:?}"
                )))
            }
        };
        Ok(option.clone())
    }

    fn link_pull_request(&self, issue_ref: &str, pr_ref: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        self.graphql(
            GITHUB_ADD_COMMENT_MUTATION,
            &[
                ("subjectId", issue.id),
                (
                    "body",
                    format!("Jade Symphony linked pull request: {pr_ref}"),
                ),
            ],
        )?;
        Ok(())
    }

    fn list_linked_pull_requests(
        &self,
        issue_ref: &str,
    ) -> Result<Vec<LinkedPullRequest>, TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        Ok(issue.linked_pull_requests)
    }

    fn close_issue(&self, issue_ref: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        if issue
            .project_fields
            .get("GitHub Issue State")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|state| normalize_state(state) == "closed")
        {
            return Ok(());
        }

        self.graphql(GITHUB_CLOSE_ISSUE_MUTATION, &[("issueId", issue.id)])?;
        Ok(())
    }

    fn find_workpad_comments(
        &self,
        issue_id: &str,
        marker: &str,
    ) -> Result<Vec<(String, String)>, TrackerError> {
        let query = github_issue_comments_query();
        let response = self.graphql(&query, &[("issueId", issue_id.to_string())])?;
        Ok(response
            .pointer("/data/node/comments/nodes")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|comment| {
                let body = comment.get("body")?.as_str()?;
                if body.contains(marker) {
                    let id = comment.get("id")?.as_str()?;
                    Some((id.to_string(), body.to_string()))
                } else {
                    None
                }
            })
            .collect())
    }

    fn repository_id(&self) -> Result<String, TrackerError> {
        let owner = self
            .config
            .tracker
            .owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing owner".into()))?;
        let repo = self
            .config
            .tracker
            .repo
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing repo".into()))?;
        let response = self.graphql(
            GITHUB_REPOSITORY_ID_QUERY,
            &[("owner", owner.to_string()), ("repo", repo.to_string())],
        )?;
        response
            .pointer("/data/repository/id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| TrackerError::Payload("repository query missing id".into()))
    }

    fn project_metadata(&self) -> Result<ProjectMetadata, TrackerError> {
        let owner = self
            .config
            .tracker
            .project_owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_owner".into()))?;
        let number =
            self.config.tracker.project_number.ok_or_else(|| {
                TrackerError::IntegrationUnavailable("missing project_number".into())
            })?;

        let response = self
            .graphql_project_metadata(true, owner, number)
            .or_else(|org_error| {
                self.graphql_project_metadata(false, owner, number)
                    .map_err(|user_error| {
                        TrackerError::IntegrationUnavailable(format!(
                            "failed to query ProjectV2 metadata as organization or user: org={org_error}; user={user_error}"
                        ))
                    })
            })?;

        project_metadata_from_response(&response, &self.config.tracker.status_field)
    }

    fn graphql_project_metadata(
        &self,
        organization_owner: bool,
        owner: &str,
        number: u64,
    ) -> Result<serde_json::Value, TrackerError> {
        let query = if organization_owner {
            github_project_metadata_query("organization")
        } else {
            github_project_metadata_query("user")
        };

        self.graphql_magic(
            &query,
            &[("owner", owner.to_string()), ("number", number.to_string())],
            &["number"],
        )
    }

    fn graphql_project_page(
        &self,
        organization_owner: bool,
        cursor: Option<&str>,
    ) -> Result<serde_json::Value, TrackerError> {
        let owner = self
            .config
            .tracker
            .project_owner
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_owner".into()))?;
        let number =
            self.config.tracker.project_number.ok_or_else(|| {
                TrackerError::IntegrationUnavailable("missing project_number".into())
            })?;

        let mut args = vec![
            "api".to_string(),
            "graphql".to_string(),
            "-f".to_string(),
            format!(
                "query={}",
                if organization_owner {
                    github_project_query("organization")
                } else {
                    github_project_query("user")
                }
            ),
            "-F".to_string(),
            format!("owner={owner}"),
            "-F".to_string(),
            format!("number={number}"),
        ];

        if let Some(cursor) = cursor {
            args.push("-F".to_string());
            args.push(format!("cursor={cursor}"));
        }

        run_gh_graphql(args)
    }

    fn graphql(
        &self,
        query: &str,
        variables: &[(&str, String)],
    ) -> Result<serde_json::Value, TrackerError> {
        self.graphql_magic(query, variables, &[])
    }

    fn graphql_magic(
        &self,
        query: &str,
        variables: &[(&str, String)],
        magic_fields: &[&str],
    ) -> Result<serde_json::Value, TrackerError> {
        if !gh_available() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 live operations require the `gh` CLI on PATH".into(),
            ));
        }

        let mut args = vec![
            "api".to_string(),
            "graphql".to_string(),
            "-f".to_string(),
            format!("query={query}"),
        ];

        for (name, value) in variables {
            if magic_fields.contains(name) {
                args.push("-F".to_string());
            } else {
                args.push("-f".to_string());
            }
            args.push(format!("{name}={value}"));
        }

        run_gh_graphql(args)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectMetadata {
    project_id: String,
    status_field_id: String,
    status_options: Vec<(String, String)>,
    fields: Vec<ProjectFieldMetadata>,
}

impl ProjectMetadata {
    fn field(&self, name: &str) -> Option<&ProjectFieldMetadata> {
        self.fields.iter().find(|field| field.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectFieldMetadata {
    id: String,
    name: String,
    kind: ProjectFieldKind,
    options: Vec<(String, String)>,
}

impl ProjectFieldMetadata {
    fn option_id(&self, option_name: &str) -> Option<String> {
        self.options
            .iter()
            .find_map(|(id, name)| (name == option_name).then_some(id.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectFieldKind {
    SingleSelect,
    Text,
    Number,
    Date,
    Iteration,
    Unknown,
}

fn gh_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GithubCliApi {
    Graphql,
    RestJson,
}

impl GithubCliApi {
    fn operation_label(self) -> &'static str {
        match self {
            Self::Graphql => "GitHub GraphQL operation",
            Self::RestJson => "GitHub REST operation",
        }
    }

    fn invalid_json_label(self) -> &'static str {
        match self {
            Self::Graphql => "invalid GitHub GraphQL JSON",
            Self::RestJson => "invalid GitHub API JSON",
        }
    }

    fn validate_response(self, response: &serde_json::Value) -> Result<(), TrackerError> {
        if self == Self::Graphql {
            if let Some(message) = graphql_error_message(response) {
                return Err(TrackerError::IntegrationUnavailable(message));
            }
        }
        Ok(())
    }
}

struct GithubCliAccess;

impl GithubCliAccess {
    const MAX_ATTEMPTS: usize = 3;

    fn run_json(api: GithubCliApi, args: Vec<String>) -> Result<serde_json::Value, TrackerError> {
        let mut last_error = None;

        for attempt in 1..=Self::MAX_ATTEMPTS {
            match Self::run_json_once(api, &args) {
                Ok(response) => return Ok(response),
                Err(error) if project_state_error_is_retryable(&error) => {
                    last_error = Some(error);
                    if attempt < Self::MAX_ATTEMPTS {
                        thread::sleep(project_state_retry_delay(attempt));
                    } else {
                        break;
                    }
                }
                Err(error) => return Err(error),
            }
        }

        let error = last_error.unwrap_or_else(|| {
            TrackerError::IntegrationUnavailable(format!("{} failed", api.operation_label()))
        });
        let kind = classify_project_state_error(&error);
        Err(TrackerError::IntegrationUnavailable(format!(
            "{} failed after {} attempts kind={}: {error}",
            api.operation_label(),
            Self::MAX_ATTEMPTS,
            kind.as_str()
        )))
    }

    fn run_status(args: Vec<String>, operation: &str) -> Result<(), TrackerError> {
        let output = Command::new("gh")
            .args(args)
            .output()
            .map_err(|error| TrackerError::IntegrationUnavailable(error.to_string()))?;

        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let kind = classify_project_state_failure_message(&message);
            return Err(TrackerError::IntegrationUnavailable(format!(
                "{operation} failed kind={}: {message}",
                kind.as_str()
            )));
        }

        Ok(())
    }

    fn run_json_once(
        api: GithubCliApi,
        args: &[String],
    ) -> Result<serde_json::Value, TrackerError> {
        let output = Command::new("gh")
            .args(args)
            .output()
            .map_err(|error| TrackerError::IntegrationUnavailable(error.to_string()))?;

        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let kind = classify_project_state_failure_message(&message);
            return Err(TrackerError::IntegrationUnavailable(format!(
                "{} failed kind={}: {message}",
                api.operation_label(),
                kind.as_str()
            )));
        }

        let response: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|error| {
                TrackerError::Payload(format!("{}: {error}", api.invalid_json_label()))
            })?;
        api.validate_response(&response)?;
        Ok(response)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GithubAuthMode {
    Fixture,
    EnvToken,
    GhCli,
    MissingGh,
    Unauthenticated { reason: Option<String> },
}

fn github_auth_mode<F>(
    config: &RuntimeConfig,
    gh_installed: bool,
    gh_auth_check: F,
) -> GithubAuthMode
where
    F: FnOnce() -> Result<(), String>,
{
    if config.tracker.fixture_path.is_some() {
        return GithubAuthMode::Fixture;
    }

    if config.tracker.api_key.is_some() {
        return GithubAuthMode::EnvToken;
    }

    if !gh_installed {
        return GithubAuthMode::MissingGh;
    }

    match gh_auth_check() {
        Ok(()) => GithubAuthMode::GhCli,
        Err(error) => GithubAuthMode::Unauthenticated {
            reason: Some(error),
        },
    }
}

fn github_graphql_auth_smoke() -> Result<(), String> {
    run_gh_graphql(vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        "query=query { viewer { login } }".into(),
    ])
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn github_auth_gap(mode: GithubAuthMode) -> Option<String> {
    match mode {
        GithubAuthMode::Fixture | GithubAuthMode::EnvToken | GithubAuthMode::GhCli => None,
        GithubAuthMode::MissingGh => {
            Some("GitHub Project v2 live reads require the `gh` CLI on PATH.".into())
        }
        GithubAuthMode::Unauthenticated { reason } => {
            let suffix = reason
                .filter(|message| !message.is_empty())
                .map(|message| format!(" Last auth check error: {message}"))
                .unwrap_or_default();
            Some(format!(
                "GitHub Project v2 live reads require `gh auth login` or GITHUB_TOKEN/GH_TOKEN; no usable GitHub auth was detected.{suffix}"
            ))
        }
    }
}

fn run_gh_graphql(args: Vec<String>) -> Result<serde_json::Value, TrackerError> {
    GithubCliAccess::run_json(GithubCliApi::Graphql, args)
}

fn run_gh_api_json(args: Vec<String>) -> Result<serde_json::Value, TrackerError> {
    GithubCliAccess::run_json(GithubCliApi::RestJson, args)
}

fn project_state_error_is_retryable(error: &TrackerError) -> bool {
    matches!(
        classify_project_state_error(error),
        ProjectStateFailureKind::Network | ProjectStateFailureKind::RateLimit
    )
}

fn project_state_retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(match attempt {
        0 | 1 => 250,
        2 => 1_000,
        _ => 2_000,
    })
}

const GITHUB_PROJECT_ITEM_PAGE_SIZE: usize = 25;
const GITHUB_PROJECT_FIELD_VALUE_PAGE_SIZE: usize = 30;
const GITHUB_PROJECT_LABEL_PAGE_SIZE: usize = 25;
const GITHUB_PROJECT_ASSIGNEE_PAGE_SIZE: usize = 10;
const GITHUB_PROJECT_LINKED_PR_PAGE_SIZE: usize = 10;
const GITHUB_PROJECT_COMMENT_PAGE_SIZE: usize = 100;
const GITHUB_PROJECT_METADATA_FIELD_PAGE_SIZE: usize = 50;
const GITHUB_WORKPAD_COMMENT_PAGE_SIZE: usize = 50;

fn github_project_query(owner_field: &str) -> String {
    format!(
        r#"
query JadeSymphonyProject($owner: String!, $number: Int!, $cursor: String) {{
  {owner_field}(login: $owner) {{
    projectV2(number: $number) {{
      items(first: {GITHUB_PROJECT_ITEM_PAGE_SIZE}, after: $cursor) {{
        pageInfo {{
          hasNextPage
          endCursor
        }}
        nodes {{
          id
          fieldValues(first: {GITHUB_PROJECT_FIELD_VALUE_PAGE_SIZE}) {{
            nodes {{
              ... on ProjectV2ItemFieldSingleSelectValue {{
                name
                field {{
                  ... on ProjectV2SingleSelectField {{
                    name
                  }}
                }}
              }}
              ... on ProjectV2ItemFieldTextValue {{
                text
                field {{
                  ... on ProjectV2FieldCommon {{
                    name
                  }}
                }}
              }}
              ... on ProjectV2ItemFieldNumberValue {{
                number
                field {{
                  ... on ProjectV2FieldCommon {{
                    name
                  }}
                }}
              }}
            }}
          }}
          content {{
            __typename
            ... on Issue {{
              id
              number
              title
              body
              url
              state
              createdAt
              updatedAt
              labels(first: {GITHUB_PROJECT_LABEL_PAGE_SIZE}) {{
                nodes {{
                  name
                }}
              }}
              assignees(first: {GITHUB_PROJECT_ASSIGNEE_PAGE_SIZE}) {{
                nodes {{
                  login
                }}
              }}
              closedByPullRequestsReferences(first: {GITHUB_PROJECT_LINKED_PR_PAGE_SIZE}) {{
                nodes {{
                  id
                  number
                  url
                  state
                  isDraft
                  baseRefName
                  headRefName
                }}
              }}
              comments(first: {GITHUB_PROJECT_COMMENT_PAGE_SIZE}) {{
                nodes {{
                  body
                }}
              }}
              recentComments: comments(last: {GITHUB_PROJECT_COMMENT_PAGE_SIZE}) {{
                nodes {{
                  body
                }}
              }}
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    )
}

fn github_project_metadata_query(owner_field: &str) -> String {
    format!(
        r#"
query JadeSymphonyProjectMetadata($owner: String!, $number: Int!) {{
  {owner_field}(login: $owner) {{
    projectV2(number: $number) {{
      id
      fields(first: {GITHUB_PROJECT_METADATA_FIELD_PAGE_SIZE}) {{
        nodes {{
          ... on ProjectV2FieldCommon {{
            id
            name
          }}
          __typename
          ... on ProjectV2SingleSelectField {{
            id
            name
            options {{
              id
              name
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    )
}

const GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION: &str = r#"
mutation JadeSymphonyUpdateProjectStatus($projectId: ID!, $itemId: ID!, $fieldId: ID!, $optionId: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId,
    value: { singleSelectOptionId: $optionId }
  }) {
    projectV2Item {
      id
    }
  }
}
"#;

const GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION: &str = r#"
mutation JadeSymphonyUpdateProjectTextField($projectId: ID!, $itemId: ID!, $fieldId: ID!, $text: String!) {
  updateProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId,
    value: { text: $text }
  }) {
    projectV2Item {
      id
    }
  }
}
"#;

const GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION: &str = r#"
mutation JadeSymphonyClearProjectField($projectId: ID!, $itemId: ID!, $fieldId: ID!) {
  clearProjectV2ItemFieldValue(input: {
    projectId: $projectId,
    itemId: $itemId,
    fieldId: $fieldId
  }) {
    projectV2Item {
      id
    }
  }
}
"#;

fn github_issue_comments_query() -> String {
    format!(
        r#"
query JadeSymphonyIssueComments($issueId: ID!) {{
  node(id: $issueId) {{
    ... on Issue {{
      comments(first: {GITHUB_WORKPAD_COMMENT_PAGE_SIZE}) {{
        nodes {{
          id
          body
        }}
      }}
    }}
  }}
}}
"#
    )
}

const GITHUB_UPDATE_ISSUE_COMMENT_MUTATION: &str = r#"
mutation JadeSymphonyUpdateIssueComment($commentId: ID!, $body: String!) {
  updateIssueComment(input: { id: $commentId, body: $body }) {
    issueComment {
      id
    }
  }
}
"#;

const GITHUB_ADD_COMMENT_MUTATION: &str = r#"
mutation JadeSymphonyAddComment($subjectId: ID!, $body: String!) {
  addComment(input: { subjectId: $subjectId, body: $body }) {
    commentEdge {
      node {
        id
      }
    }
  }
}
"#;

const GITHUB_CLOSE_ISSUE_MUTATION: &str = r#"
mutation JadeSymphonyCloseIssue($issueId: ID!) {
  closeIssue(input: { issueId: $issueId, stateReason: COMPLETED }) {
    issue {
      id
      state
    }
  }
}
"#;

const GITHUB_REPOSITORY_ID_QUERY: &str = r#"
query JadeSymphonyRepositoryId($owner: String!, $repo: String!) {
  repository(owner: $owner, name: $repo) {
    id
  }
}
"#;

const GITHUB_CREATE_ISSUE_MUTATION: &str = r#"
mutation JadeSymphonyCreateIssue($repositoryId: ID!, $title: String!, $body: String!) {
  createIssue(input: { repositoryId: $repositoryId, title: $title, body: $body }) {
    issue {
      id
      number
      url
    }
  }
}
"#;

const GITHUB_ADD_PROJECT_ITEM_MUTATION: &str = r#"
mutation JadeSymphonyAddProjectItem($projectId: ID!, $contentId: ID!) {
  addProjectV2ItemById(input: { projectId: $projectId, contentId: $contentId }) {
    item {
      id
    }
  }
}
"#;

fn issues_from_project_response(
    response: &serde_json::Value,
    config: &RuntimeConfig,
) -> Result<(Vec<TrackerIssue>, Option<String>, bool), TrackerError> {
    let project = response
        .pointer("/data/organization/projectV2")
        .or_else(|| response.pointer("/data/user/projectV2"))
        .ok_or_else(|| TrackerError::Payload("missing ProjectV2 payload".into()))?;
    let items = project
        .pointer("/items/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TrackerError::Payload("missing ProjectV2 item nodes".into()))?;

    let mut issues = Vec::new();
    for item in items {
        if let Some(issue) = issue_from_project_item(item, config)? {
            issues.push(issue);
        }
    }

    let page_info = project.pointer("/items/pageInfo").ok_or_else(|| {
        TrackerError::Payload("partial ProjectV2 response missing pageInfo".into())
    })?;
    let has_next_page = page_info
        .get("hasNextPage")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| {
            TrackerError::Payload("partial ProjectV2 response missing pageInfo.hasNextPage".into())
        })?;
    let next_cursor = page_info
        .get("endCursor")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    if has_next_page && next_cursor.is_none() {
        return Err(TrackerError::Payload(
            "partial ProjectV2 response missing pageInfo.endCursor".into(),
        ));
    }

    Ok((issues, next_cursor, has_next_page))
}

fn project_metadata_from_response(
    response: &serde_json::Value,
    status_field: &str,
) -> Result<ProjectMetadata, TrackerError> {
    let project = response
        .pointer("/data/organization/projectV2")
        .or_else(|| response.pointer("/data/user/projectV2"))
        .ok_or_else(|| TrackerError::Payload("missing ProjectV2 metadata payload".into()))?;
    let project_id = project
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| TrackerError::Payload("ProjectV2 metadata missing project id".into()))?;
    let fields = project
        .pointer("/fields/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TrackerError::Payload("ProjectV2 metadata missing fields".into()))?;

    let fields = fields
        .iter()
        .filter_map(project_field_metadata)
        .collect::<Vec<_>>();

    if let Some(status_field_metadata) = fields.iter().find(|field| field.name == status_field) {
        return Ok(ProjectMetadata {
            project_id: project_id.to_string(),
            status_field_id: status_field_metadata.id.clone(),
            status_options: status_field_metadata.options.clone(),
            fields,
        });
    }

    Err(TrackerError::Payload(format!(
        "ProjectV2 status field {status_field:?} was not found"
    )))
}

fn project_field_metadata(field: &serde_json::Value) -> Option<ProjectFieldMetadata> {
    let id = field.get("id")?.as_str()?.to_string();
    let name = field.get("name")?.as_str()?.to_string();
    let kind = match field
        .get("__typename")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
    {
        "ProjectV2SingleSelectField" => ProjectFieldKind::SingleSelect,
        "ProjectV2Field" => ProjectFieldKind::Text,
        "ProjectV2IterationField" => ProjectFieldKind::Iteration,
        "ProjectV2FieldCommon" => ProjectFieldKind::Unknown,
        "ProjectV2NumberField" => ProjectFieldKind::Number,
        "ProjectV2DateField" => ProjectFieldKind::Date,
        _ => ProjectFieldKind::Unknown,
    };
    let options = field
        .get("options")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some((
                option.get("id")?.as_str()?.to_string(),
                option.get("name")?.as_str()?.to_string(),
            ))
        })
        .collect();

    Some(ProjectFieldMetadata {
        id,
        name,
        kind,
        options,
    })
}

fn ensure_workpad_marker(markdown: &str, marker: &str) -> String {
    if markdown.contains(marker) {
        markdown.to_string()
    } else {
        format!("{marker}\n{markdown}")
    }
}

fn merge_workpad_body(existing: &str, incoming: &str, marker: &str) -> String {
    let existing = ensure_workpad_marker(existing, marker);
    let incoming = ensure_workpad_marker(incoming, marker);
    let (mut merged, incoming_remainder) = replace_singleton_workpad_blocks(&existing, &incoming);
    let incoming_content = strip_workpad_marker(&incoming_remainder, marker);

    for entry in split_workpad_entries(incoming_content) {
        merged = merge_workpad_entry(&merged, &entry, marker);
    }

    merged
}

fn merge_workpad_entry(existing: &str, incoming_entry: &str, marker: &str) -> String {
    let incoming_entry = incoming_entry.trim();
    if incoming_entry.is_empty() || existing.contains(incoming_entry) {
        return existing.to_string();
    }

    if let Some(key) = workpad_entry_key(incoming_entry) {
        return replace_or_append_workpad_entry(existing, incoming_entry, marker, &key);
    }

    append_workpad_entry(existing, incoming_entry)
}

fn replace_or_append_workpad_entry(
    existing: &str,
    incoming_entry: &str,
    marker: &str,
    incoming_key: &str,
) -> String {
    let content = strip_workpad_marker(existing, marker);
    let mut replaced = false;
    let mut entries = Vec::new();

    for entry in split_workpad_entries(content) {
        if workpad_entry_key(&entry).as_deref() == Some(incoming_key) {
            if !replaced {
                entries.push(incoming_entry.to_string());
                replaced = true;
            }
        } else {
            entries.push(entry);
        }
    }

    if !replaced {
        entries.push(incoming_entry.to_string());
    }

    render_workpad_entries(marker, &entries)
}

fn append_workpad_entry(existing: &str, incoming_entry: &str) -> String {
    let mut merged = existing.trim_end().to_string();
    merged.push_str("\n\n---\n\n");
    merged.push_str(incoming_entry);
    merged
}

fn split_workpad_entries(markdown: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = Vec::new();

    for line in markdown.lines() {
        if line.trim() == "---" {
            let entry = current.join("\n").trim().to_string();
            if !entry.is_empty() {
                entries.push(entry);
            }
            current.clear();
        } else {
            current.push(line);
        }
    }

    let entry = current.join("\n").trim().to_string();
    if !entry.is_empty() {
        entries.push(entry);
    }

    entries
}

fn render_workpad_entries(marker: &str, entries: &[String]) -> String {
    if entries.is_empty() {
        marker.to_string()
    } else {
        format!("{marker}\n{}", entries.join("\n\n---\n\n"))
    }
}

fn workpad_entry_key(entry: &str) -> Option<String> {
    let mut h2 = None;
    let mut h3 = None;

    for line in entry.lines() {
        let line = line.trim();
        if h2.is_none() && line.starts_with("## ") && !line.starts_with("### ") {
            h2 = Some(line.to_string());
            continue;
        }
        if h2.is_some() && line.starts_with("### ") {
            h3 = Some(line.to_string());
            break;
        }
    }

    h2.map(|h2| match h3 {
        Some(h3) => format!("{h2}\n{h3}"),
        None => h2,
    })
}

fn strip_workpad_marker<'a>(markdown: &'a str, marker: &str) -> &'a str {
    markdown
        .strip_prefix(marker)
        .map(str::trim_start)
        .unwrap_or(markdown)
}

fn replace_singleton_workpad_blocks(existing: &str, incoming: &str) -> (String, String) {
    const RUNTIME_OWNERSHIP: (&str, &str, bool) = (
        "<!-- jade-symphony-runtime-ownership -->",
        "<!-- /jade-symphony-runtime-ownership -->",
        true,
    );
    const WORKSPACE_ADOPTION: (&str, &str, bool) = (
        "<!-- jade-symphony-workspace-adoption -->",
        "<!-- /jade-symphony-workspace-adoption -->",
        false,
    );

    let mut merged = existing.to_string();
    let mut remainder = incoming.to_string();
    for (start, end, strip_when_missing) in [RUNTIME_OWNERSHIP, WORKSPACE_ADOPTION] {
        let Some(incoming_block) = marked_block(&remainder, start, end).map(str::to_string) else {
            continue;
        };
        if let Some(existing_block) = marked_block(&merged, start, end).map(str::to_string) {
            merged = merged.replacen(&existing_block, &incoming_block, 1);
            remainder = remainder.replacen(&incoming_block, "", 1);
        } else if strip_when_missing {
            remainder = remainder.replacen(&incoming_block, "", 1);
        }
    }

    (merged, remainder)
}

fn marked_block<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_index = text.find(start)?;
    let after_start = &text[start_index + start.len()..];
    let end_offset = after_start.find(end)?;
    let end_index = start_index + start.len() + end_offset + end.len();
    Some(&text[start_index..end_index])
}

fn status_option_id(
    metadata: &ProjectMetadata,
    option_name: &str,
    status_field: &str,
) -> Result<String, TrackerError> {
    metadata
        .status_options
        .iter()
        .find_map(|(id, name)| (name == option_name).then_some(id.clone()))
        .ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "status option {option_name:?} was not found in ProjectV2 field {status_field}"
            ))
        })
}

fn project_item_id_from_add_response(response: &serde_json::Value) -> Result<String, TrackerError> {
    response
        .pointer("/data/addProjectV2ItemById/item/id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            TrackerError::Payload("addProjectV2ItemById response missing item id".into())
        })
}

fn status_update_required(issue: &TrackerIssue, target_state: &str) -> bool {
    tracker_state_key(&issue.state) != tracker_state_key(target_state)
}

fn tracker_state_key(state: &str) -> String {
    normalize_state(state)
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn duplicate_workpad_body(_marker: &str) -> String {
    "Superseded Jade Symphony workpad comment. The canonical marker was removed from this duplicate."
        .to_string()
}

fn follow_up_issue_body(input: &FollowUpIssueInput) -> String {
    let mut body = input.body.clone();
    let mut context = Vec::new();

    if let Some(issue_ref) = &input.related_issue_ref {
        context.push(format!("- Related issue: {issue_ref}"));
    }
    if let Some(issue_ref) = &input.blocked_by_issue_ref {
        context.push(format!("- Blocked by: {issue_ref}"));
    }
    if let Some(project_id) = &input.project_id {
        context.push(format!("- Project id: {project_id}"));
    }

    if context.is_empty() {
        return body;
    }

    if !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("\n## Jade Symphony Context\n");
    body.push_str(&context.join("\n"));
    body
}

fn graphql_error_message(response: &serde_json::Value) -> Option<String> {
    let errors = response.get("errors")?.as_array()?;
    if errors.is_empty() {
        return None;
    }

    let messages = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();

    if messages.is_empty() {
        Some(format!("GitHub GraphQL returned errors: {errors:?}"))
    } else {
        Some(format!(
            "GitHub GraphQL returned errors: {}",
            messages.join("; ")
        ))
    }
}

fn issue_from_project_item(
    item: &serde_json::Value,
    config: &RuntimeConfig,
) -> Result<Option<TrackerIssue>, TrackerError> {
    let Some(content) = item.get("content") else {
        return Ok(None);
    };
    if content
        .get("__typename")
        .and_then(serde_json::Value::as_str)
        != Some("Issue")
    {
        return Ok(None);
    }

    let number = content
        .get("number")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| TrackerError::Payload("partial ProjectV2 issue missing number".into()))?;
    let state = project_status(item, &config.tracker.status_field).ok_or_else(|| {
        TrackerError::Payload(format!(
            "partial ProjectV2 response missing status field {:?} for issue #{number}",
            config.tracker.status_field
        ))
    })?;
    let mut project_fields = project_fields(item);
    project_fields.insert(
        config.tracker.status_field.clone(),
        serde_json::Value::String(state.clone()),
    );
    if let Some(issue_state) = content.get("state").and_then(serde_json::Value::as_str) {
        project_fields.insert(
            "GitHub Issue State".into(),
            serde_json::Value::String(issue_state.to_string()),
        );
    }
    let blocked_by = blocker_refs_from_project_fields(&project_fields);

    Ok(Some(TrackerIssue {
        tracker_kind: "github_project_v2".into(),
        id: content
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                TrackerError::Payload(format!("partial ProjectV2 issue #{number} missing id"))
            })?
            .to_string(),
        item_id: item
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        identifier: format!("#{number}"),
        title: content
            .get("title")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                TrackerError::Payload(format!("partial ProjectV2 issue #{number} missing title"))
            })?
            .to_string(),
        description: github_issue_description_with_workpad(content, &config.tracker.workpad.marker),
        url: content
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        state,
        labels: string_nodes(content.pointer("/labels/nodes"), "name")
            .into_iter()
            .map(|label| label.to_lowercase())
            .collect(),
        assignees: string_nodes(content.pointer("/assignees/nodes"), "login"),
        priority: project_fields.get("Priority").and_then(json_number_to_i64),
        branch_name: None,
        linked_pull_requests: pull_requests_from_issue(content),
        blocked_by,
        project_fields,
        created_at: content
            .get("createdAt")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        updated_at: content
            .get("updatedAt")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    }))
}

fn github_issue_description_with_workpad(
    content: &serde_json::Value,
    marker: &str,
) -> Option<String> {
    let body = content
        .get("body")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let comment_bodies = github_issue_comment_bodies(content);
    let workpad = canonical_workpad_comment_body(&comment_bodies, marker);
    let timeline_comments = jade_symphony_timeline_comment_bodies(&comment_bodies, marker);

    let mut sections = Vec::new();
    if !body.trim().is_empty() {
        sections.push(body);
    }
    if let Some(workpad) = workpad {
        sections.push(workpad);
    }
    sections.extend(timeline_comments);

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

fn github_issue_comment_bodies(content: &serde_json::Value) -> Vec<&str> {
    let mut bodies = Vec::new();
    for pointer in ["/comments/nodes", "/recentComments/nodes"] {
        if let Some(nodes) = content
            .pointer(pointer)
            .and_then(serde_json::Value::as_array)
        {
            for comment in nodes {
                if let Some(body) = comment.get("body").and_then(serde_json::Value::as_str) {
                    if !bodies.contains(&body) {
                        bodies.push(body);
                    }
                }
            }
        }
    }
    bodies
}

fn canonical_workpad_comment_body(comment_bodies: &[&str], marker: &str) -> Option<String> {
    comment_bodies
        .iter()
        .find(|body| body.contains(marker) && !body.contains("Superseded Jade Symphony workpad"))
        .map(|body| (*body).to_string())
}

fn jade_symphony_timeline_comment_bodies(comment_bodies: &[&str], marker: &str) -> Vec<String> {
    comment_bodies
        .iter()
        .filter(|body| !body.contains(marker))
        .filter(|body| is_jade_symphony_timeline_comment(body))
        .map(|body| (*body).to_string())
        .collect()
}

fn is_jade_symphony_timeline_comment(body: &str) -> bool {
    [
        "## Jade Symphony Agent Review Run",
        "## Jade Symphony Rework Run",
        "## Jade Symphony Merge Run",
        "## Jade Symphony Human Review Decision",
        "## Jade Symphony Doctor Triage",
        "## Manual Agent Review Evidence",
    ]
    .iter()
    .any(|heading| body.contains(heading))
}

fn blocker_refs_from_project_fields(
    project_fields: &BTreeMap<String, serde_json::Value>,
) -> Vec<BlockerRef> {
    project_fields
        .iter()
        .filter(|(name, _)| is_blocker_field(name))
        .flat_map(|(_, value)| blocker_refs_from_value(value))
        .collect()
}

fn github_native_blocker_refs_from_response(
    response: &serde_json::Value,
    issue_number: u64,
) -> Result<Vec<BlockerRef>, TrackerError> {
    let blockers = response.as_array().ok_or_else(|| {
        TrackerError::Payload(format!(
            "GitHub issue #{issue_number} native dependency response was not an array"
        ))
    })?;

    blockers
        .iter()
        .map(|blocker| {
            let number = blocker
                .get("number")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    TrackerError::Payload(format!(
                        "GitHub issue #{issue_number} native dependency response missing blocker number"
                    ))
                })?;
            let id = blocker
                .get("node_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    blocker
                        .get("id")
                        .and_then(serde_json::Value::as_u64)
                        .map(|id| id.to_string())
                });
            let state = blocker
                .get("state")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);

            Ok(BlockerRef {
                id,
                identifier: Some(format!("#{number}")),
                state,
            })
        })
        .collect()
}

fn merge_blocker_refs(existing: &mut Vec<BlockerRef>, incoming: Vec<BlockerRef>) {
    for incoming_blocker in incoming {
        if let Some(existing_blocker) = existing
            .iter_mut()
            .find(|existing_blocker| same_blocker_ref(existing_blocker, &incoming_blocker))
        {
            if existing_blocker.id.is_none() {
                existing_blocker.id = incoming_blocker.id.clone();
            }
            if existing_blocker.identifier.is_none() {
                existing_blocker.identifier = incoming_blocker.identifier.clone();
            }
            if existing_blocker.state.is_none() {
                existing_blocker.state = incoming_blocker.state.clone();
            }
        } else {
            existing.push(incoming_blocker);
        }
    }
}

fn same_blocker_ref(left: &BlockerRef, right: &BlockerRef) -> bool {
    let same_identifier = match (&left.identifier, &right.identifier) {
        (Some(left_identifier), Some(right_identifier)) => {
            github_issue_number(left_identifier) == github_issue_number(right_identifier)
                && github_issue_number(left_identifier).is_some()
        }
        _ => false,
    };
    let same_id = match (&left.id, &right.id) {
        (Some(left_id), Some(right_id)) => left_id == right_id,
        _ => false,
    };

    same_identifier || same_id
}

fn github_issue_number(identifier: &str) -> Option<u64> {
    identifier
        .trim()
        .strip_prefix('#')
        .unwrap_or(identifier.trim())
        .parse()
        .ok()
}

fn is_blocker_field(name: &str) -> bool {
    matches!(
        normalize_state(name).as_str(),
        "blocked by" | "blocked_by" | "blockers" | "dependencies"
    )
}

fn blocker_refs_from_value(value: &serde_json::Value) -> Vec<BlockerRef> {
    let Some(raw) = value.as_str() else {
        return Vec::new();
    };

    raw.split([',', '\n'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| BlockerRef {
            id: None,
            identifier: Some(part.to_string()),
            state: None,
        })
        .collect()
}

fn json_number_to_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number as i64))
}

fn project_status(item: &serde_json::Value, status_field: &str) -> Option<String> {
    field_values(item).find_map(|value| {
        let field_name = value.pointer("/field/name")?.as_str()?;
        if field_name == status_field {
            value.get("name")?.as_str().map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

fn project_fields(item: &serde_json::Value) -> BTreeMap<String, serde_json::Value> {
    let mut fields = BTreeMap::new();
    for value in field_values(item) {
        let Some(name) = value
            .pointer("/field/name")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
            fields.insert(
                name.to_string(),
                serde_json::Value::String(text.to_string()),
            );
        } else if let Some(select) = value.get("name").and_then(serde_json::Value::as_str) {
            fields.insert(
                name.to_string(),
                serde_json::Value::String(select.to_string()),
            );
        } else if let Some(number) = value.get("number").and_then(serde_json::Value::as_f64) {
            if let Some(number) = serde_json::Number::from_f64(number) {
                fields.insert(name.to_string(), serde_json::Value::Number(number));
            }
        }
    }
    fields
}

fn field_values(item: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    item.pointer("/fieldValues/nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
}

fn string_nodes(nodes: Option<&serde_json::Value>, field: &str) -> Vec<String> {
    nodes
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|node| node.get(field).and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn pull_requests_from_issue(issue: &serde_json::Value) -> Vec<LinkedPullRequest> {
    issue
        .pointer("/closedByPullRequestsReferences/nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|node| LinkedPullRequest {
            id: node
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            number: node.get("number").and_then(serde_json::Value::as_u64),
            url: node
                .get("url")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            state: node
                .get("state")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            is_draft: node.get("isDraft").and_then(serde_json::Value::as_bool),
            base_ref_name: node
                .get("baseRefName")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            head_ref_name: node
                .get("headRefName")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
fn linked_pull_requests_from_workpads(workpad_bodies: &[String]) -> Vec<LinkedPullRequest> {
    let mut seen = BTreeSet::new();
    let mut linked = Vec::new();
    for body in workpad_bodies {
        for url in github_pull_request_urls(body) {
            if seen.insert(url.clone()) {
                linked.push(linked_pull_request_from_url(&url));
            }
        }
    }
    linked
}

#[cfg(test)]
fn merge_linked_pull_requests(
    existing: Vec<LinkedPullRequest>,
    discovered: Vec<LinkedPullRequest>,
) -> Vec<LinkedPullRequest> {
    let mut seen_urls = BTreeSet::new();
    let mut merged = Vec::new();
    for pr in existing.into_iter().chain(discovered) {
        if let Some(url) = &pr.url {
            if !seen_urls.insert(url.clone()) {
                continue;
            }
        }
        merged.push(pr);
    }
    merged
}

#[cfg(test)]
fn github_pull_request_urls(text: &str) -> Vec<String> {
    text.split(|character: char| character.is_whitespace() || character == '<' || character == '>')
        .filter_map(clean_github_pull_request_url)
        .collect()
}

#[cfg(test)]
fn clean_github_pull_request_url(raw: &str) -> Option<String> {
    let value = raw.trim_matches(|character: char| {
        matches!(
            character,
            '`' | '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.'
        )
    });
    let marker = "/pull/";
    let marker_index = value.find(marker)?;
    let number_start = marker_index + marker.len();
    let number_end = value[number_start..]
        .find(|character: char| !character.is_ascii_digit())
        .map(|offset| number_start + offset)
        .unwrap_or(value.len());
    if number_end == number_start {
        return None;
    }
    let base = &value[..number_end];
    (base.starts_with("https://github.com/") || base.starts_with("http://github.com/"))
        .then(|| base.to_string())
}

#[cfg(test)]
fn linked_pull_request_from_url(url: &str) -> LinkedPullRequest {
    LinkedPullRequest {
        id: None,
        number: url
            .rsplit_once('/')
            .and_then(|(_, number)| number.parse::<u64>().ok()),
        url: Some(url.to_string()),
        state: None,
        is_draft: None,
        merge_state_status: None,
        review_decision: None,
        base_ref_name: None,
        head_ref_name: None,
    }
}

#[derive(Debug, Clone)]
pub struct LinearAdapter {
    config: RuntimeConfig,
    fixture_issues: Vec<TrackerIssue>,
}

impl LinearAdapter {
    pub fn new(config: RuntimeConfig) -> Self {
        let fixture_issues = load_fixture(&config).unwrap_or_default();
        Self {
            config,
            fixture_issues,
        }
    }

    fn fixture_mode(&self) -> bool {
        self.config.tracker.fixture_path.is_some()
    }

    fn load_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        let issues = if self.fixture_mode() {
            MemoryTracker::new(self.fixture_issues.clone()).fetch_issues_by_states(states)?
        } else {
            LinearGraphqlClient::new(&self.config).fetch_issues_by_states(states)?
        };

        Ok(apply_linear_read_filters(issues, &self.config))
    }

    fn resolve_issue(&self, issue_ref: &str) -> Result<TrackerIssue, TrackerError> {
        if self.fixture_mode() {
            return MemoryTracker::new(self.fixture_issues.clone())
                .get_issue(issue_ref)?
                .ok_or_else(|| {
                    TrackerError::IntegrationUnavailable(format!(
                        "Linear fixture issue {issue_ref} was not found"
                    ))
                });
        }

        LinearGraphqlClient::new(&self.config)
            .fetch_issue(issue_ref)?
            .ok_or_else(|| {
                TrackerError::IntegrationUnavailable(format!(
                    "Linear issue {issue_ref} was not found"
                ))
            })
    }
}

impl TrackerAdapter for LinearAdapter {
    fn kind(&self) -> &'static str {
        "linear"
    }

    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.load_issues_by_states(&self.config.tracker.active_states)
    }

    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        if self.fixture_mode() {
            return MemoryTracker::new(self.fixture_issues.clone()).get_issue(issue_ref);
        }

        LinearGraphqlClient::new(&self.config).fetch_issue(issue_ref)
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.load_issues_by_states(states)
    }

    fn set_state(&self, issue_ref: &str, normalized_state: &str) -> Result<(), TrackerError> {
        if self.fixture_mode() {
            return Err(TrackerError::IntegrationUnavailable(
                "Linear fixture mode cannot update live issue state".into(),
            ));
        }

        let issue = self.resolve_issue(issue_ref)?;
        LinearGraphqlClient::new(&self.config).set_state(&issue.id, normalized_state)
    }

    fn upsert_workpad(&self, issue_ref: &str, markdown: &str) -> Result<(), TrackerError> {
        if self.fixture_mode() {
            return Err(TrackerError::IntegrationUnavailable(
                "Linear fixture mode cannot upsert live workpads".into(),
            ));
        }

        let issue = self.resolve_issue(issue_ref)?;
        let body = ensure_workpad_marker(markdown, &self.config.tracker.workpad.marker);
        LinearGraphqlClient::new(&self.config).upsert_workpad(&issue.id, &body)
    }

    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError> {
        if self.fixture_mode() {
            return Err(TrackerError::IntegrationUnavailable(
                "Linear fixture mode cannot create live follow-up issues".into(),
            ));
        }

        LinearGraphqlClient::new(&self.config).create_follow_up_issue(input)
    }

    fn add_issue_to_project(&self, issue_id: &str) -> Result<(), TrackerError> {
        if self.fixture_mode() {
            return Err(TrackerError::IntegrationUnavailable(
                "Linear fixture mode cannot add live project issues".into(),
            ));
        }

        LinearGraphqlClient::new(&self.config).add_issue_to_project(issue_id)
    }

    fn add_issue_to_project_with_state(
        &self,
        issue_id: &str,
        normalized_state: &str,
    ) -> Result<(), TrackerError> {
        if self.fixture_mode() {
            return Err(TrackerError::IntegrationUnavailable(
                "Linear fixture mode cannot add live project issues".into(),
            ));
        }

        self.add_issue_to_project(issue_id)?;
        if normalize_state(normalized_state) != "todo" {
            LinearGraphqlClient::new(&self.config).set_state(issue_id, normalized_state)?;
        }
        Ok(())
    }

    fn link_pull_request(&self, issue_ref: &str, pr_ref: &str) -> Result<(), TrackerError> {
        if self.fixture_mode() {
            return Err(TrackerError::IntegrationUnavailable(
                "Linear fixture mode cannot link live pull requests".into(),
            ));
        }

        let issue = self.resolve_issue(issue_ref)?;
        LinearGraphqlClient::new(&self.config).create_comment(
            &issue.id,
            &format!("Jade Symphony linked pull request: {pr_ref}"),
        )
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
        let mut gaps = Vec::new();

        if self.fixture_mode() {
            gaps.push("Linear is using fixture issues because tracker.fixture_path is set.".into());
        }

        if !self.fixture_mode() && !curl_available() {
            gaps.push("Linear live operations require the `curl` CLI on PATH.".into());
        }

        if !self.fixture_mode() && self.config.tracker.api_key.is_none() {
            gaps.push(
                "Linear token not detected; set tracker.api_key or LINEAR_API_KEY for live reads."
                    .into(),
            );
        }

        gaps.push("Linear pull request linking currently records a tracker comment rather than a first-class Linear attachment.".into());
        gaps
    }
}

fn apply_linear_read_filters(
    issues: Vec<TrackerIssue>,
    config: &RuntimeConfig,
) -> Vec<TrackerIssue> {
    issues
        .into_iter()
        .filter(|issue| issue_matches_assignee_filter(issue, &config.tracker.assignee_filter))
        .collect()
}

#[derive(Debug, Clone)]
struct LinearGraphqlClient {
    config: RuntimeConfig,
}

impl LinearGraphqlClient {
    fn new(config: &RuntimeConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        let project_slug = self.project_slug()?;
        let mut issues = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let response = self.graphql(
                LINEAR_ISSUES_QUERY,
                serde_json::json!({
                    "projectSlug": project_slug,
                    "stateNames": states,
                    "first": 50,
                    "relationFirst": 50,
                    "after": cursor,
                }),
            )?;
            let (mut page_issues, next_cursor, has_next_page) =
                linear_issues_from_response(&response)?;
            issues.append(&mut page_issues);

            if has_next_page {
                cursor = Some(next_cursor.ok_or_else(|| {
                    TrackerError::Payload("Linear pageInfo missing endCursor".into())
                })?);
            } else {
                break;
            }
        }

        Ok(issues)
    }

    fn fetch_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        let response = self.graphql(
            LINEAR_ISSUE_QUERY,
            serde_json::json!({
                "issueId": issue_ref,
                "relationFirst": 50,
            }),
        )?;

        Ok(response
            .pointer("/data/issue")
            .and_then(linear_issue_from_node))
    }

    fn set_state(&self, issue_id: &str, normalized_state: &str) -> Result<(), TrackerError> {
        let state_name = linear_state_option_name(&self.config, normalized_state)?;
        let state_id = self.resolve_state_id(issue_id, &state_name)?;
        let response = self.graphql(
            LINEAR_UPDATE_ISSUE_STATE_MUTATION,
            serde_json::json!({
                "issueId": issue_id,
                "stateId": state_id,
            }),
        )?;
        expect_linear_success(&response, "/data/issueUpdate/success", "issueUpdate")
    }

    fn upsert_workpad(&self, issue_id: &str, body: &str) -> Result<(), TrackerError> {
        let comment_ids = self.find_workpad_comment_ids(issue_id)?;
        if let Some(comment_id) = comment_ids.first() {
            let response = self.graphql(
                LINEAR_UPDATE_COMMENT_MUTATION,
                serde_json::json!({
                    "commentId": comment_id,
                    "body": body,
                }),
            )?;
            expect_linear_success(&response, "/data/commentUpdate/success", "commentUpdate")?;
        } else {
            self.create_comment(issue_id, body)?;
        }
        Ok(())
    }

    fn create_comment(&self, issue_id: &str, body: &str) -> Result<(), TrackerError> {
        let response = self.graphql(
            LINEAR_CREATE_COMMENT_MUTATION,
            serde_json::json!({
                "issueId": issue_id,
                "body": body,
            }),
        )?;
        expect_linear_success(&response, "/data/commentCreate/success", "commentCreate")
    }

    fn create_follow_up_issue(&self, input: FollowUpIssueInput) -> Result<String, TrackerError> {
        if !input.assignees.is_empty() {
            return Err(TrackerError::NotImplemented(
                "Linear follow-up issue assignee assignment is not implemented".into(),
            ));
        }
        let project = self.resolve_project()?;
        let body = follow_up_issue_body(&input);
        let response = self.graphql(
            LINEAR_CREATE_ISSUE_MUTATION,
            serde_json::json!({
                "teamId": project.team_id,
                "projectId": project.project_id,
                "title": input.title,
                "description": body,
            }),
        )?;
        expect_linear_success(&response, "/data/issueCreate/success", "issueCreate")?;
        response
            .pointer("/data/issueCreate/issue/id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| TrackerError::Payload("Linear issueCreate response missing id".into()))
    }

    fn add_issue_to_project(&self, issue_id: &str) -> Result<(), TrackerError> {
        let project = self.resolve_project()?;
        let response = self.graphql(
            LINEAR_ADD_ISSUE_TO_PROJECT_MUTATION,
            serde_json::json!({
                "issueId": issue_id,
                "projectId": project.project_id,
            }),
        )?;
        expect_linear_success(&response, "/data/issueUpdate/success", "issueUpdate")
    }

    fn find_workpad_comment_ids(&self, issue_id: &str) -> Result<Vec<String>, TrackerError> {
        let marker = &self.config.tracker.workpad.marker;
        let response = self.graphql(
            LINEAR_ISSUE_COMMENTS_QUERY,
            serde_json::json!({
                "issueId": issue_id,
            }),
        )?;
        Ok(response
            .pointer("/data/issue/comments/nodes")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|comment| {
                let body = comment.get("body")?.as_str()?;
                if body.contains(marker) {
                    comment
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                } else {
                    None
                }
            })
            .collect())
    }

    fn resolve_state_id(&self, issue_id: &str, state_name: &str) -> Result<String, TrackerError> {
        let response = self.graphql(
            LINEAR_STATE_LOOKUP_QUERY,
            serde_json::json!({
                "issueId": issue_id,
                "stateName": state_name,
            }),
        )?;
        response
            .pointer("/data/issue/team/states/nodes/0/id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                TrackerError::IntegrationUnavailable(format!(
                    "Linear state {state_name:?} was not found for issue {issue_id}"
                ))
            })
    }

    fn resolve_project(&self) -> Result<LinearProjectRef, TrackerError> {
        let project_slug = self.project_slug()?;
        let response = self.graphql(
            LINEAR_PROJECT_LOOKUP_QUERY,
            serde_json::json!({
                "projectSlug": project_slug,
            }),
        )?;
        let project = response.pointer("/data/projects/nodes/0").ok_or_else(|| {
            TrackerError::Payload("Linear project lookup returned no project".into())
        })?;
        let project_id = project
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TrackerError::Payload("Linear project lookup missing id".into()))?;
        let team_id = project
            .pointer("/team/id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| TrackerError::Payload("Linear project lookup missing team id".into()))?;

        Ok(LinearProjectRef {
            project_id: project_id.to_string(),
            team_id: team_id.to_string(),
        })
    }

    fn project_slug(&self) -> Result<&str, TrackerError> {
        self.config
            .tracker
            .project_slug
            .as_deref()
            .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_slug".into()))
    }

    fn graphql(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<serde_json::Value, TrackerError> {
        let endpoint = self
            .config
            .tracker
            .endpoint
            .as_deref()
            .unwrap_or("https://api.linear.app/graphql");
        let token = self.config.tracker.api_key.as_deref().ok_or_else(|| {
            TrackerError::IntegrationUnavailable("missing Linear API token".into())
        })?;

        run_linear_graphql(endpoint, token, query, variables)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinearProjectRef {
    project_id: String,
    team_id: String,
}

fn curl_available() -> bool {
    Command::new("curl")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_linear_graphql(
    endpoint: &str,
    token: &str,
    query: &str,
    variables: serde_json::Value,
) -> Result<serde_json::Value, TrackerError> {
    if !curl_available() {
        return Err(TrackerError::IntegrationUnavailable(
            "Linear live operations require the `curl` CLI on PATH".into(),
        ));
    }

    let payload = serde_json::json!({
        "query": query,
        "variables": variables,
    });
    let output = Command::new("curl")
        .arg("-sS")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-H")
        .arg(format!("Authorization: {token}"))
        .arg("--data-binary")
        .arg(payload.to_string())
        .arg(endpoint)
        .output()
        .map_err(|error| TrackerError::IntegrationUnavailable(error.to_string()))?;

    if !output.status.success() {
        return Err(TrackerError::IntegrationUnavailable(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let response: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| TrackerError::Payload(format!("invalid Linear GraphQL JSON: {error}")))?;

    if let Some(message) = linear_graphql_error_message(&response) {
        return Err(TrackerError::IntegrationUnavailable(message));
    }

    Ok(response)
}

const LINEAR_ISSUES_QUERY: &str = r#"
query JadeSymphonyLinearIssues($projectSlug: String!, $stateNames: [String!]!, $first: Int!, $relationFirst: Int!, $after: String) {
  issues(filter: {project: {slugId: {eq: $projectSlug}}, state: {name: {in: $stateNames}}}, first: $first, after: $after) {
    nodes {
      id
      identifier
      title
      description
      priority
      state {
        name
      }
      branchName
      url
      assignee {
        id
      }
      labels {
        nodes {
          name
        }
      }
      inverseRelations(first: $relationFirst) {
        nodes {
          type
          issue {
            id
            identifier
            state {
              name
            }
          }
        }
      }
      createdAt
      updatedAt
    }
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}
"#;

const LINEAR_ISSUE_QUERY: &str = r#"
query JadeSymphonyLinearIssue($issueId: String!, $relationFirst: Int!) {
  issue(id: $issueId) {
    id
    identifier
    title
    description
    priority
    state {
      name
    }
    branchName
    url
    assignee {
      id
    }
    labels {
      nodes {
        name
      }
    }
    inverseRelations(first: $relationFirst) {
      nodes {
        type
        issue {
          id
          identifier
          state {
            name
          }
        }
      }
    }
    createdAt
    updatedAt
  }
}
"#;

const LINEAR_STATE_LOOKUP_QUERY: &str = r#"
query JadeSymphonyLinearResolveStateId($issueId: String!, $stateName: String!) {
  issue(id: $issueId) {
    team {
      states(filter: {name: {eq: $stateName}}, first: 1) {
        nodes {
          id
        }
      }
    }
  }
}
"#;

const LINEAR_ISSUE_COMMENTS_QUERY: &str = r#"
query JadeSymphonyLinearIssueComments($issueId: String!) {
  issue(id: $issueId) {
    comments(first: 100) {
      nodes {
        id
        body
      }
    }
  }
}
"#;

const LINEAR_PROJECT_LOOKUP_QUERY: &str = r#"
query JadeSymphonyLinearProject($projectSlug: String!) {
  projects(filter: {slugId: {eq: $projectSlug}}, first: 1) {
    nodes {
      id
      team {
        id
      }
    }
  }
}
"#;

const LINEAR_CREATE_COMMENT_MUTATION: &str = r#"
mutation JadeSymphonyLinearCreateComment($issueId: String!, $body: String!) {
  commentCreate(input: {issueId: $issueId, body: $body}) {
    success
  }
}
"#;

const LINEAR_UPDATE_COMMENT_MUTATION: &str = r#"
mutation JadeSymphonyLinearUpdateComment($commentId: String!, $body: String!) {
  commentUpdate(id: $commentId, input: {body: $body}) {
    success
  }
}
"#;

const LINEAR_UPDATE_ISSUE_STATE_MUTATION: &str = r#"
mutation JadeSymphonyLinearUpdateIssueState($issueId: String!, $stateId: String!) {
  issueUpdate(id: $issueId, input: {stateId: $stateId}) {
    success
  }
}
"#;

const LINEAR_ADD_ISSUE_TO_PROJECT_MUTATION: &str = r#"
mutation JadeSymphonyLinearAddIssueToProject($issueId: String!, $projectId: String!) {
  issueUpdate(id: $issueId, input: {projectId: $projectId}) {
    success
  }
}
"#;

const LINEAR_CREATE_ISSUE_MUTATION: &str = r#"
mutation JadeSymphonyLinearCreateIssue($teamId: String!, $projectId: String!, $title: String!, $description: String!) {
  issueCreate(input: {teamId: $teamId, projectId: $projectId, title: $title, description: $description}) {
    success
    issue {
      id
      identifier
      url
    }
  }
}
"#;

fn linear_issues_from_response(
    response: &serde_json::Value,
) -> Result<(Vec<TrackerIssue>, Option<String>, bool), TrackerError> {
    let issues_payload = response
        .pointer("/data/issues")
        .ok_or_else(|| TrackerError::Payload("missing Linear issues payload".into()))?;
    let nodes = issues_payload
        .pointer("/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| TrackerError::Payload("missing Linear issue nodes".into()))?;
    let issues = nodes.iter().filter_map(linear_issue_from_node).collect();
    let page_info = issues_payload
        .pointer("/pageInfo")
        .unwrap_or(&serde_json::Value::Null);
    let has_next_page = page_info
        .get("hasNextPage")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let next_cursor = page_info
        .get("endCursor")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);

    Ok((issues, next_cursor, has_next_page))
}

fn linear_issue_from_node(issue: &serde_json::Value) -> Option<TrackerIssue> {
    let state = issue.pointer("/state/name")?.as_str()?.to_string();
    let mut project_fields = BTreeMap::new();
    project_fields.insert("State".into(), serde_json::Value::String(state.clone()));
    if let Some(priority) = issue.get("priority").and_then(json_number_to_i64) {
        project_fields.insert(
            "Priority".into(),
            serde_json::Value::Number(priority.into()),
        );
    }

    Some(TrackerIssue {
        tracker_kind: "linear".into(),
        id: issue.get("id")?.as_str()?.to_string(),
        item_id: None,
        identifier: issue.get("identifier")?.as_str()?.to_string(),
        title: issue.get("title")?.as_str()?.to_string(),
        description: issue
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        url: issue
            .get("url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        state,
        labels: string_nodes(issue.pointer("/labels/nodes"), "name")
            .into_iter()
            .map(|label| label.to_lowercase())
            .collect(),
        assignees: issue
            .pointer("/assignee/id")
            .and_then(serde_json::Value::as_str)
            .map(|assignee| vec![assignee.to_string()])
            .unwrap_or_default(),
        priority: issue.get("priority").and_then(json_number_to_i64),
        branch_name: issue
            .get("branchName")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        linked_pull_requests: Vec::new(),
        blocked_by: linear_blocker_refs(issue),
        project_fields,
        created_at: issue
            .get("createdAt")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        updated_at: issue
            .get("updatedAt")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn linear_blocker_refs(issue: &serde_json::Value) -> Vec<BlockerRef> {
    issue
        .pointer("/inverseRelations/nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|relation| {
            let relation_type = relation.get("type")?.as_str()?;
            if normalize_state(relation_type) != "blocks" {
                return None;
            }
            let blocker_issue = relation.get("issue")?;
            Some(BlockerRef {
                id: blocker_issue
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                identifier: blocker_issue
                    .get("identifier")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
                state: blocker_issue
                    .pointer("/state/name")
                    .and_then(serde_json::Value::as_str)
                    .map(ToOwned::to_owned),
            })
        })
        .collect()
}

fn linear_state_option_name(
    config: &RuntimeConfig,
    normalized_state: &str,
) -> Result<String, TrackerError> {
    let state_map = &config.tracker.state_map;
    let option = match normalized_state {
        "backlog" => &state_map.backlog,
        "todo" => &state_map.todo,
        "need_to_clarify" | "need to clarify" => &state_map.need_to_clarify,
        "in_progress" | "in progress" => &state_map.in_progress,
        "need_human_input" | "need human input" => &state_map.need_human_input,
        "agent_review" | "agent review" => &state_map.agent_review,
        "human_review" | "human review" => &state_map.human_review,
        "rework" => &state_map.rework,
        "merging" => &state_map.merging,
        "done" => &state_map.done,
        other => {
            return Err(TrackerError::IntegrationUnavailable(format!(
                "unsupported normalized Linear state {other:?}"
            )))
        }
    };
    Ok(option.clone())
}

fn expect_linear_success(
    response: &serde_json::Value,
    success_pointer: &str,
    operation: &str,
) -> Result<(), TrackerError> {
    if response
        .pointer(success_pointer)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(TrackerError::Payload(format!(
            "Linear {operation} response did not report success"
        )))
    }
}

fn linear_graphql_error_message(response: &serde_json::Value) -> Option<String> {
    let errors = response.get("errors")?.as_array()?;
    if errors.is_empty() {
        return None;
    }

    let messages = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();

    if messages.is_empty() {
        Some(format!("Linear GraphQL returned errors: {errors:?}"))
    } else {
        Some(format!(
            "Linear GraphQL returned errors: {}",
            messages.join("; ")
        ))
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
    use crate::workflow::WorkflowDefinition;
    use std::path::Path;

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

    fn github_config(source: &str) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse("/tmp/WORKFLOW.md", source).unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    #[test]
    fn memory_tracker_filters_by_state() {
        let tracker = MemoryTracker::new(vec![issue("Todo"), issue("Done")]);
        let found = tracker.fetch_issues_by_states(&["todo".into()]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].state, "Todo");
    }

    #[test]
    fn github_auth_mode_distinguishes_fixture_env_token_and_gh_cli() {
        let mut config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );

        config.tracker.fixture_path = Some(Path::new("issues.json").to_path_buf());
        assert_eq!(
            github_auth_mode(&config, false, || Err("unused".into())),
            GithubAuthMode::Fixture
        );

        config.tracker.fixture_path = None;
        config.tracker.api_key = Some("redacted".into());
        assert_eq!(
            github_auth_mode(&config, false, || Err("unused".into())),
            GithubAuthMode::EnvToken
        );

        config.tracker.api_key = None;
        assert_eq!(
            github_auth_mode(&config, true, || Ok(())),
            GithubAuthMode::GhCli
        );
    }

    #[test]
    fn github_auth_gap_only_reports_missing_or_invalid_live_auth() {
        assert_eq!(github_auth_gap(GithubAuthMode::GhCli), None);
        assert_eq!(github_auth_gap(GithubAuthMode::EnvToken), None);
        assert_eq!(
            github_auth_gap(GithubAuthMode::MissingGh).as_deref(),
            Some("GitHub Project v2 live reads require the `gh` CLI on PATH.")
        );

        let gap = github_auth_gap(GithubAuthMode::Unauthenticated {
            reason: Some("invalid token".into()),
        })
        .unwrap();
        assert!(gap.contains("gh auth login"));
        assert!(gap.contains("GITHUB_TOKEN/GH_TOKEN"));
        assert!(gap.contains("invalid token"));
    }

    #[test]
    fn parses_github_project_v2_issue_items() {
        let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
        let response = serde_json::json!({
            "data": {
                "organization": {
                    "projectV2": {
                        "items": {
                            "pageInfo": {
                                "hasNextPage": false,
                                "endCursor": null
                            },
                            "nodes": [
                                {
                                    "id": "PVTI_1",
                                    "fieldValues": {
                                        "nodes": [
                                            {
                                                "name": "Todo",
                                                "field": {"name": "Status"}
                                            },
                                            {
                                                "number": 1.0,
                                                "field": {"name": "Priority"}
                                            }
                                        ]
                                    },
                                    "content": {
                                        "__typename": "Issue",
                                        "id": "GHI_1",
                                        "number": 42,
                                        "title": "Implement adapter",
                                        "body": "body",
                                        "url": "https://github.com/Alive24/jade-symphony/issues/42",
                                        "state": "OPEN",
                                        "createdAt": "2026-05-10T00:00:00Z",
                                        "updatedAt": "2026-05-10T01:00:00Z",
                                        "labels": {"nodes": [{"name": "Dogfood"}]},
                                        "assignees": {"nodes": [{"login": "codex"}]},
                                        "closedByPullRequestsReferences": {
                                            "nodes": [
                                                {
                                                    "id": "PR_1",
                                                    "number": 7,
                                                    "url": "https://github.com/Alive24/jade-symphony/pull/7",
                                                    "state": "OPEN"
                                                }
                                            ]
                                        }
                                    }
                                },
                                {
                                    "id": "PVTI_DRAFT",
                                    "fieldValues": {"nodes": []},
                                    "content": {"__typename": "DraftIssue"}
                                }
                            ]
                        }
                    }
                }
            }
        });

        let (issues, next_cursor, has_next) =
            issues_from_project_response(&response, &config).unwrap();

        assert!(!has_next);
        assert_eq!(next_cursor, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].identifier, "#42");
        assert_eq!(issues[0].item_id.as_deref(), Some("PVTI_1"));
        assert_eq!(issues[0].state, "Todo");
        assert_eq!(issues[0].labels, vec!["dogfood"]);
        assert_eq!(issues[0].assignees, vec!["codex"]);
        assert_eq!(issues[0].priority, Some(1));
        assert_eq!(
            issues[0]
                .project_fields
                .get("GitHub Issue State")
                .and_then(serde_json::Value::as_str),
            Some("OPEN")
        );
        assert_eq!(issues[0].linked_pull_requests[0].number, Some(7));
    }

    #[test]
    fn discovers_pull_request_urls_from_workpad_text() {
        let bodies = vec![format!(
            "{}\n- Live PR: `https://github.com/Alive24/jade-symphony/pull/98` (created: `true`)\n- Also see https://github.com/Alive24/jade-symphony/pull/100.",
            "<!-- jade-symphony-workpad -->"
        )];

        let prs = linked_pull_requests_from_workpads(&bodies);

        assert_eq!(prs.len(), 2);
        assert_eq!(
            prs[0].url.as_deref(),
            Some("https://github.com/Alive24/jade-symphony/pull/98")
        );
        assert_eq!(prs[0].number, Some(98));
        assert_eq!(prs[0].state, None);
        assert_eq!(
            prs[1].url.as_deref(),
            Some("https://github.com/Alive24/jade-symphony/pull/100")
        );
    }

    #[test]
    fn merge_linked_pull_requests_deduplicates_by_url() {
        let closing_ref = LinkedPullRequest {
            id: Some("PR_98".into()),
            number: Some(98),
            url: Some("https://github.com/Alive24/jade-symphony/pull/98".into()),
            state: Some("OPEN".into()),
            is_draft: None,
            merge_state_status: None,
            review_decision: None,
            base_ref_name: None,
            head_ref_name: None,
        };
        let discovered_duplicate =
            linked_pull_request_from_url("https://github.com/Alive24/jade-symphony/pull/98");
        let discovered_new =
            linked_pull_request_from_url("https://github.com/Alive24/jade-symphony/pull/100");

        let merged = merge_linked_pull_requests(
            vec![closing_ref],
            vec![discovered_duplicate, discovered_new],
        );

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id.as_deref(), Some("PR_98"));
        assert_eq!(merged[0].state.as_deref(), Some("OPEN"));
        assert_eq!(
            merged[1].url.as_deref(),
            Some("https://github.com/Alive24/jade-symphony/pull/100")
        );
    }

    #[test]
    fn github_issue_description_includes_canonical_workpad_comment() {
        let content = serde_json::json!({
            "body": "issue body",
            "comments": {
                "nodes": [
                    {"body": "ordinary comment"},
                    {"body": "<!-- jade-symphony-workpad -->\n## Workpad\n\n<!-- jade-symphony-runtime-ownership -->\n### Runtime Ownership\n<!-- /jade-symphony-runtime-ownership -->"}
                ]
            }
        });

        let description =
            github_issue_description_with_workpad(&content, "<!-- jade-symphony-workpad -->")
                .unwrap();

        assert!(description.contains("issue body"));
        assert!(description.contains("jade-symphony-runtime-ownership"));
    }

    #[test]
    fn github_issue_description_includes_timeline_comments_for_review_evidence() {
        let content = serde_json::json!({
            "body": "issue body",
            "comments": {
                "nodes": [
                    {"body": "<!-- jade-symphony-workpad -->\n## Jade Symphony Workpad"},
                    {"body": "ordinary comment"}
                ]
            },
            "recentComments": {
                "nodes": [
                    {"body": "ordinary recent comment"},
                    {"body": "## Jade Symphony Agent Review Run\n\nReview pass evidence: `recorded`"}
                ]
            }
        });

        let description =
            github_issue_description_with_workpad(&content, "<!-- jade-symphony-workpad -->")
                .unwrap();

        assert!(description.contains("## Jade Symphony Workpad"));
        assert!(description.contains("## Jade Symphony Agent Review Run"));
        assert!(description.contains("Review pass evidence: `recorded`"));
    }

    #[test]
    fn filters_github_read_issues_by_status_map_and_assignees() {
        let config = github_config(
            r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 1
  assignee_filter:
    source: issue_assignees
    allow_unassigned: false
    assignees:
      - codex
---
Prompt
"#,
        );

        let mut mapped_assigned = issue("Todo");
        mapped_assigned.assignees = vec!["Codex".into()];
        let mut unmapped_status = issue("Custom Status");
        unmapped_status.assignees = vec!["codex".into()];
        let mut wrong_assignee = issue("Todo");
        wrong_assignee.assignees = vec!["someone-else".into()];
        let unassigned = issue("Todo");

        let filtered = apply_github_read_filters(
            vec![mapped_assigned, unmapped_status, wrong_assignee, unassigned],
            &config,
        );

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].state, "Todo");
        assert_eq!(filtered[0].assignees, vec!["Codex"]);
    }

    #[test]
    fn github_state_reads_can_bypass_main_assignee_filter_for_merge_lane() {
        let config = github_config(
            r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 1
  assignee_filter:
    source: issue_assignees
    allow_unassigned: false
    assignees: []
---
Prompt
"#,
        );

        let unassigned_merging = issue("Merging");
        let dispatch_filtered =
            apply_github_read_filters(vec![unassigned_merging.clone()], &config);
        let state_filtered = apply_github_status_filters(vec![unassigned_merging], &config);

        assert!(dispatch_filtered.is_empty());
        assert_eq!(state_filtered.len(), 1);
        assert_eq!(state_filtered[0].state, "Merging");
    }

    #[test]
    fn github_assignee_filter_can_allow_unassigned_issues() {
        let filter = AssigneeFilter {
            source: "issue_assignees".into(),
            allow_unassigned: true,
            assignees: vec!["codex".into()],
        };

        assert!(issue_matches_assignee_filter(&issue("Todo"), &filter));
    }

    #[test]
    fn parses_blocker_refs_from_project_fields_when_present() {
        let mut fields = BTreeMap::new();
        fields.insert(
            "Blocked By".into(),
            serde_json::Value::String("#1, #2\nGHI_3".into()),
        );

        let blockers = blocker_refs_from_project_fields(&fields);

        assert_eq!(blockers.len(), 3);
        assert_eq!(blockers[0].identifier.as_deref(), Some("#1"));
        assert_eq!(blockers[1].identifier.as_deref(), Some("#2"));
        assert_eq!(blockers[2].identifier.as_deref(), Some("GHI_3"));
    }

    #[test]
    fn parses_github_native_blocker_refs_from_rest_response() {
        let response = serde_json::json!([
            {
                "id": 4453853955u64,
                "node_id": "I_kwDOSZP6c88AAAABCXhrAw",
                "number": 225,
                "state": "open"
            },
            {
                "id": 4453858123u64,
                "number": 226,
                "state": "closed"
            }
        ]);

        let blockers = github_native_blocker_refs_from_response(&response, 224).unwrap();

        assert_eq!(blockers.len(), 2);
        assert_eq!(blockers[0].id.as_deref(), Some("I_kwDOSZP6c88AAAABCXhrAw"));
        assert_eq!(blockers[0].identifier.as_deref(), Some("#225"));
        assert_eq!(blockers[0].state.as_deref(), Some("open"));
        assert_eq!(blockers[1].id.as_deref(), Some("4453858123"));
        assert_eq!(blockers[1].identifier.as_deref(), Some("#226"));
        assert_eq!(blockers[1].state.as_deref(), Some("closed"));
    }

    #[test]
    fn native_blocker_refs_enrich_project_field_blockers() {
        let mut existing = vec![BlockerRef {
            id: None,
            identifier: Some("#225".into()),
            state: None,
        }];
        let incoming = vec![
            BlockerRef {
                id: Some("I_225".into()),
                identifier: Some("#225".into()),
                state: Some("open".into()),
            },
            BlockerRef {
                id: Some("I_226".into()),
                identifier: Some("#226".into()),
                state: Some("closed".into()),
            },
        ];

        merge_blocker_refs(&mut existing, incoming);

        assert_eq!(existing.len(), 2);
        assert_eq!(existing[0].id.as_deref(), Some("I_225"));
        assert_eq!(existing[0].identifier.as_deref(), Some("#225"));
        assert_eq!(existing[0].state.as_deref(), Some("open"));
        assert_eq!(existing[1].identifier.as_deref(), Some("#226"));
    }

    #[test]
    fn parses_project_field_assignment() {
        let assignment = ProjectFieldAssignment::parse("Capability=CLI").unwrap();

        assert_eq!(assignment.name, "Capability");
        assert_eq!(assignment.value, "CLI");
        assert!(ProjectFieldAssignment::parse("Capability").is_err());
        assert!(ProjectFieldAssignment::parse("=CLI").is_err());
    }

    #[test]
    fn parses_project_metadata_status_options() {
        let response = serde_json::json!({
            "data": {
                "user": {
                    "projectV2": {
                        "id": "PVT_1",
                        "fields": {
                            "nodes": [
                                {
                                    "id": "FIELD_STATUS",
                                    "name": "Status",
                                    "__typename": "ProjectV2SingleSelectField",
                                    "options": [
                                        {"id": "OPT_TODO", "name": "Todo"},
                                        {"id": "OPT_DONE", "name": "Done"}
                                    ]
                                },
                                {
                                    "id": "FIELD_CAPABILITY",
                                    "name": "Capability",
                                    "__typename": "ProjectV2SingleSelectField",
                                    "options": [
                                        {"id": "OPT_CLI", "name": "CLI"},
                                        {"id": "OPT_TRACKER", "name": "Tracker"}
                                    ]
                                },
                                {
                                    "id": "FIELD_MAIN_AGENT",
                                    "name": "Main Agent",
                                    "__typename": "ProjectV2Field"
                                }
                            ]
                        }
                    }
                }
            }
        });

        let metadata = project_metadata_from_response(&response, "Status").unwrap();
        assert_eq!(metadata.project_id, "PVT_1");
        assert_eq!(metadata.status_field_id, "FIELD_STATUS");
        assert_eq!(
            metadata.status_options,
            vec![
                ("OPT_TODO".into(), "Todo".into()),
                ("OPT_DONE".into(), "Done".into())
            ]
        );
        let capability = metadata.field("Capability").unwrap();
        assert_eq!(capability.kind, ProjectFieldKind::SingleSelect);
        assert_eq!(capability.option_id("CLI").as_deref(), Some("OPT_CLI"));
        let main_agent = metadata.field("Main Agent").unwrap();
        assert_eq!(main_agent.kind, ProjectFieldKind::Text);
    }

    #[test]
    fn resolves_project_status_option_id() {
        let metadata = ProjectMetadata {
            project_id: "PVT_1".into(),
            status_field_id: "FIELD_STATUS".into(),
            status_options: vec![
                ("OPT_TODO".into(), "Todo".into()),
                ("OPT_DONE".into(), "Done".into()),
            ],
            fields: vec![ProjectFieldMetadata {
                id: "FIELD_STATUS".into(),
                name: "Status".into(),
                kind: ProjectFieldKind::SingleSelect,
                options: vec![
                    ("OPT_TODO".into(), "Todo".into()),
                    ("OPT_DONE".into(), "Done".into()),
                ],
            }],
        };

        assert_eq!(
            status_option_id(&metadata, "Todo", "Status").unwrap(),
            "OPT_TODO"
        );
        assert!(status_option_id(&metadata, "Agent Review", "Status").is_err());
    }

    #[test]
    fn parses_added_project_item_id() {
        let response = serde_json::json!({
            "data": {
                "addProjectV2ItemById": {
                    "item": {
                        "id": "PVTI_35"
                    }
                }
            }
        });

        assert_eq!(
            project_item_id_from_add_response(&response).unwrap(),
            "PVTI_35"
        );
        assert!(project_item_id_from_add_response(&serde_json::json!({"data": {}})).is_err());
    }

    #[test]
    fn claim_decision_identifies_claimable_active_and_external_states() {
        let config = github_config(
            r#"---
tracker:
  kind: github_project_v2
  owner: Alive24
  repo: jade-symphony
  project_owner: Alive24
  project_number: 9
---
Prompt
"#,
        );

        assert_eq!(
            claim_decision(&issue("Todo"), &config),
            ClaimDecision::Claimable
        );
        assert_eq!(
            claim_decision(&issue("Rework"), &config),
            ClaimDecision::Claimable
        );
        assert_eq!(
            claim_decision(&issue("In Progress"), &config),
            ClaimDecision::AlreadyInProgress
        );
        assert_eq!(
            claim_decision(&issue("Agent Review"), &config),
            ClaimDecision::StopAndReplan {
                current_state: "Agent Review".into()
            }
        );
    }

    #[test]
    fn status_update_required_skips_same_mapped_state() {
        assert!(!status_update_required(
            &issue("In Progress"),
            "in_progress"
        ));
        assert!(!status_update_required(
            &issue("Agent Review"),
            "Agent Review"
        ));
        assert!(status_update_required(&issue("Todo"), "In Progress"));
    }

    #[test]
    fn parses_linear_issue_payload_into_normalized_tracker_issue() {
        let response = serde_json::json!({
            "data": {
                "issues": {
                    "nodes": [
                        {
                            "id": "LIN_1",
                            "identifier": "JAD-10",
                            "title": "Implement Linear adapter",
                            "description": "body",
                            "priority": 2,
                            "state": {"name": "Todo"},
                            "branchName": "feature/jad-10",
                            "url": "https://linear.app/acme/issue/JAD-10/implement-linear-adapter",
                            "assignee": {"id": "USR_1"},
                            "labels": {"nodes": [{"name": "Dogfood"}]},
                            "inverseRelations": {
                                "nodes": [
                                    {
                                        "type": "blocks",
                                        "issue": {
                                            "id": "LIN_BLOCKER",
                                            "identifier": "JAD-9",
                                            "state": {"name": "In Progress"}
                                        }
                                    },
                                    {
                                        "type": "related",
                                        "issue": {
                                            "id": "LIN_RELATED",
                                            "identifier": "JAD-8",
                                            "state": {"name": "Done"}
                                        }
                                    }
                                ]
                            },
                            "createdAt": "2026-05-10T00:00:00Z",
                            "updatedAt": "2026-05-10T01:00:00Z"
                        }
                    ],
                    "pageInfo": {
                        "hasNextPage": false,
                        "endCursor": null
                    }
                }
            }
        });

        let (issues, next_cursor, has_next) = linear_issues_from_response(&response).unwrap();

        assert!(!has_next);
        assert_eq!(next_cursor, None);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].tracker_kind, "linear");
        assert_eq!(issues[0].id, "LIN_1");
        assert_eq!(issues[0].identifier, "JAD-10");
        assert_eq!(issues[0].state, "Todo");
        assert_eq!(issues[0].assignees, vec!["USR_1"]);
        assert_eq!(issues[0].labels, vec!["dogfood"]);
        assert_eq!(issues[0].priority, Some(2));
        assert_eq!(issues[0].branch_name.as_deref(), Some("feature/jad-10"));
        assert_eq!(issues[0].blocked_by.len(), 1);
        assert_eq!(issues[0].blocked_by[0].identifier.as_deref(), Some("JAD-9"));
    }

    #[test]
    fn maps_normalized_state_to_linear_state_name() {
        let config = github_config(
            r#"---
tracker:
  kind: linear
  project_slug: jade-symphony
  fixture_path: issues.json
  state_map:
    in_progress: Started
    agent_review: Agent Review
---
Prompt
"#,
        );

        assert_eq!(
            linear_state_option_name(&config, "in_progress").unwrap(),
            "Started"
        );
        assert_eq!(
            linear_state_option_name(&config, "agent review").unwrap(),
            "Agent Review"
        );
        assert!(linear_state_option_name(&config, "unknown").is_err());
    }

    #[test]
    fn detects_linear_graphql_error_payloads() {
        let response = serde_json::json!({
            "errors": [
                {"message": "Linear API rejected the query"}
            ]
        });

        assert_eq!(
            linear_graphql_error_message(&response).as_deref(),
            Some("Linear GraphQL returned errors: Linear API rejected the query")
        );
        assert!(linear_graphql_error_message(&serde_json::json!({"data": {}})).is_none());
    }

    #[test]
    fn prepends_workpad_marker_once() {
        let marker = "<!-- jade-symphony-workpad -->";
        let body = ensure_workpad_marker("## Workpad", marker);
        assert!(body.starts_with("<!-- jade-symphony-workpad -->"));
        let body = ensure_workpad_marker(&body, marker);
        let body = ensure_workpad_marker(&body, marker);
        assert_eq!(body.matches(marker).count(), 1);
    }

    #[test]
    fn merge_workpad_body_appends_without_losing_existing_sections() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing =
            format!("{marker}\n## Jade Symphony Workpad\n\n### Plan\n- [ ] inspect issue");
        let incoming = "## Agent Review\n\n### Manual Review Evidence\n````md\npass\n````";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert!(body.contains("### Plan"));
        assert!(body.contains("## Agent Review"));
        assert!(body.contains("pass"));
    }

    #[test]
    fn merge_workpad_body_appends_distinct_review_attempts() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Agent Review\n\n- Reviewer backend: gemini-cli\n\n### Review Attempt gemini-old\n- Review pass evidence: `recorded`"
        );
        let incoming = "## Agent Review\n\n- Reviewer backend: gemini-cli\n\n### Review Attempt gemini-new\n- [Confirmed] Bug: needs rework";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Agent Review").count(), 2);
        assert!(body.contains("### Review Attempt gemini-old"));
        assert!(body.contains("### Review Attempt gemini-new"));
        assert!(body.contains("Review pass evidence: `recorded`"));
        assert!(body.contains("[Confirmed] Bug"));
    }

    #[test]
    fn merge_workpad_body_replaces_matching_jade_symphony_workpad_entry() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Context\n- old context\n\n### Plan\n- [ ] old plan\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming =
            "## Jade Symphony Workpad\n\n### Context\n- updated context\n\n### Plan\n- [x] updated plan";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(body.contains("- updated context"));
        assert!(body.contains("- [x] updated plan"));
        assert!(!body.contains("- old context"));
        assert!(body.contains("## Agent Review"));
        assert!(body.contains("Manual Review Evidence"));
    }

    #[test]
    fn merge_workpad_body_collapses_duplicate_matching_entries() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Context\n- first\n\n---\n\n## Jade Symphony Workpad\n\n### Context\n- duplicate\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming = "## Jade Symphony Workpad\n\n### Context\n- final";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(body.contains("- final"));
        assert!(!body.contains("- first"));
        assert!(!body.contains("- duplicate"));
        assert!(body.contains("## Agent Review"));
    }

    #[test]
    fn merge_workpad_body_replaces_workspace_adoption_block() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Context\n- keep\n\n<!-- jade-symphony-workspace-adoption -->\n### Workspace Adoption\n- Path: `/tmp/old`\n<!-- /jade-symphony-workspace-adoption -->"
        );
        let incoming =
            "<!-- jade-symphony-workspace-adoption -->\n### Workspace Adoption\n- Path: `/tmp/new`\n<!-- /jade-symphony-workspace-adoption -->";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- jade-symphony-workspace-adoption -->")
                .count(),
            1
        );
        assert!(body.contains("/tmp/new"));
        assert!(!body.contains("/tmp/old"));
        assert!(body.contains("- keep"));
    }

    #[test]
    fn merge_workpad_body_replaces_runtime_ownership_marker() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n<!-- jade-symphony-runtime-ownership -->\n### Runtime Ownership\n- Branch: `old`\n<!-- /jade-symphony-runtime-ownership -->\n\n### Plan\n- [ ] inspect issue"
        );
        let incoming = "<!-- jade-symphony-runtime-ownership -->\n### Runtime Ownership\n- Branch: `new`\n<!-- /jade-symphony-runtime-ownership -->\n\n### Runtime Ownership Note\nupdated";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- jade-symphony-runtime-ownership -->")
                .count(),
            1
        );
        assert!(body.contains("- Branch: `new`"));
        assert!(!body.contains("- Branch: `old`"));
        assert!(body.contains("### Plan"));
        assert!(body.contains("### Runtime Ownership Note"));
    }

    #[test]
    fn duplicate_workpad_body_removes_marker_text() {
        let marker = "<!-- jade-symphony-workpad -->";
        let body = duplicate_workpad_body(marker);

        assert!(!body.contains(marker));
    }

    #[test]
    fn follow_up_body_preserves_related_context() {
        let body = follow_up_issue_body(&FollowUpIssueInput {
            title: "Follow-up".into(),
            body: "Main body".into(),
            assignees: Vec::new(),
            project_id: Some("PVT_1".into()),
            related_issue_ref: Some("#3".into()),
            blocked_by_issue_ref: Some("#2".into()),
        });

        assert!(body.contains("Main body"));
        assert!(body.contains("Related issue: #3"));
        assert!(body.contains("Blocked by: #2"));
        assert!(body.contains("Project id: PVT_1"));
    }

    #[test]
    fn detects_graphql_error_payloads() {
        let response = serde_json::json!({
            "errors": [
                {"message": "Could not resolve to a ProjectV2"}
            ]
        });

        assert_eq!(
            graphql_error_message(&response).as_deref(),
            Some("GitHub GraphQL returned errors: Could not resolve to a ProjectV2")
        );
        assert!(graphql_error_message(&serde_json::json!({"data": {}})).is_none());
    }

    #[test]
    fn classifies_project_state_failures() {
        assert_eq!(
            classify_project_state_failure_message("API rate limit exceeded"),
            ProjectStateFailureKind::RateLimit
        );
        assert_eq!(
            classify_project_state_failure_message("GraphQL resource limit exceeded"),
            ProjectStateFailureKind::ResourceLimit
        );
        assert_eq!(
            classify_project_state_failure_message("could not resolve host api.github.com"),
            ProjectStateFailureKind::Network
        );
        assert_eq!(
            classify_project_state_failure_message(
                "GitHub GraphQL returned errors: Field 'foo' doesn't exist on type ProjectV2"
            ),
            ProjectStateFailureKind::Schema
        );
        assert_eq!(
            classify_project_state_error(&TrackerError::Payload(
                "partial ProjectV2 response missing status field \"Status\" for issue #7".into()
            )),
            ProjectStateFailureKind::PartialResponse
        );
        assert_eq!(
            classify_project_state_error(&TrackerError::NotImplemented(
                "missing CLI capability for raw issue content reads".into()
            )),
            ProjectStateFailureKind::MissingCapability
        );
    }

    #[test]
    fn project_issue_missing_status_is_partial_response_error() {
        let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
        let response = serde_json::json!({
            "data": {
                "organization": {
                    "projectV2": {
                        "items": {
                            "nodes": [
                                {
                                    "id": "PVTI_1",
                                    "content": {
                                        "__typename": "Issue",
                                        "id": "I_1",
                                        "number": 7,
                                        "title": "Project read hardening"
                                    },
                                    "fieldValues": {
                                        "nodes": []
                                    }
                                }
                            ],
                            "pageInfo": {
                                "hasNextPage": false,
                                "endCursor": null
                            }
                        }
                    }
                }
            }
        });

        let error = issues_from_project_response(&response, &config).unwrap_err();

        assert_eq!(
            classify_project_state_error(&error),
            ProjectStateFailureKind::PartialResponse
        );
        assert!(error.to_string().contains("missing status field"));
    }
}
