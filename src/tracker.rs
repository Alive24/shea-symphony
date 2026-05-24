use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{AssigneeFilter, RuntimeConfig};
use crate::model::{normalize_state, BlockerRef, LinkedPullRequest, TrackerIssue};

mod github;
mod linear;
mod workpad;

pub use github::GithubProjectReadMode;
pub use linear::LinearAdapter;

use github::{
    gh_available, github_auth_gap, github_auth_mode, github_graphql_auth_smoke,
    github_issue_comments_query, github_issue_evidence_query, github_issue_project_item_query,
    github_project_metadata_query, github_project_query, run_gh_api_json, run_gh_graphql,
    GithubCliAccess, GITHUB_ADD_COMMENT_MUTATION, GITHUB_ADD_PROJECT_ITEM_MUTATION,
    GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION, GITHUB_CLOSE_ISSUE_MUTATION,
    GITHUB_CREATE_ISSUE_MUTATION, GITHUB_REPOSITORY_ID_QUERY, GITHUB_UPDATE_ISSUE_COMMENT_MUTATION,
    GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION, GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
};
#[cfg(test)]
use github::{project_state_error_is_retryable, run_command_with_timeout, GithubAuthMode};
#[cfg(test)]
use linear::{linear_graphql_error_message, linear_issues_from_response, linear_state_option_name};
use workpad::{ensure_workpad_marker, merge_workpad_body};

pub trait TrackerAdapter {
    fn kind(&self) -> &'static str;
    fn list_queue_scan_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.list_dispatchable_issues()
    }
    fn list_project_summary_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.list_queue_scan_issues()
    }
    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError>;
    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError>;
    fn hydrate_issue_evidence(
        &self,
        issue: TrackerIssue,
        _project_context: &[TrackerIssue],
    ) -> Result<TrackerIssue, TrackerError> {
        Ok(self.get_issue(&issue.identifier)?.unwrap_or(issue))
    }
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
    TransientBackend,
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
            Self::TransientBackend => "transient_backend",
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
    } else if normalized.contains("http 500")
        || normalized.contains("http 502")
        || normalized.contains("http 503")
        || normalized.contains("http 504")
        || normalized.contains("bad gateway")
        || normalized.contains("service unavailable")
        || normalized.contains("gateway timeout")
        || normalized.contains("internal server error")
    {
        ProjectStateFailureKind::TransientBackend
    } else if normalized.contains("could not resolve host")
        || normalized.contains("error connecting to")
        || normalized.contains("failed to connect")
        || normalized.contains("could not connect")
        || normalized.contains("connection timed out")
        || normalized.contains("timed out after")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
        || normalized.contains("connection closed")
        || normalized.contains("temporary failure in name resolution")
        || normalized.contains("no route to host")
        || normalized.contains("i/o timeout")
        || normalized.contains("context deadline exceeded")
        || is_transport_eof_message(&normalized)
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

fn is_transport_eof_message(normalized: &str) -> bool {
    let trimmed = normalized.trim();
    let eof_suffix = trimmed == "eof" || trimmed.ends_with(": eof") || trimmed.ends_with(" eof");
    if !eof_suffix {
        return false;
    }

    let looks_like_json_parse_error = normalized.contains("invalid gh")
        || normalized.contains("invalid github")
        || normalized.contains("while parsing");
    if looks_like_json_parse_error {
        return false;
    }

    normalized.contains("api.github.com")
        || normalized.contains("graphql")
        || normalized.contains("rest")
        || normalized.contains("http://")
        || normalized.contains("https://")
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

    fn load_issues(&self, mode: GithubProjectReadMode) -> Result<Vec<TrackerIssue>, TrackerError> {
        let mut issues = apply_github_read_filters(self.load_mapped_issues(mode)?, &self.config);
        self.enrich_native_subissue_project_statuses(&mut issues)?;
        self.enrich_native_issue_blockers_for_claimable_issues(&mut issues)?;
        Ok(issues)
    }

    fn load_mapped_issues(
        &self,
        mode: GithubProjectReadMode,
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
        let issues =
            if !self.fixture_issues.is_empty() || self.config.tracker.fixture_path.is_some() {
                self.fixture_issues.clone()
            } else {
                GithubProjectV2GhClient::new(&self.config).fetch_project_issues(mode)?
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

    fn enrich_native_subissue_project_statuses(
        &self,
        issues: &mut [TrackerIssue],
    ) -> Result<(), TrackerError> {
        if !self.fixture_issues.is_empty() || self.config.tracker.fixture_path.is_some() {
            enrich_native_subissue_project_statuses_from_project_read(issues);
            return Ok(());
        }

        enrich_native_subissue_project_statuses_from_project_read(issues);
        let client = GithubProjectV2GhClient::new(&self.config);
        for issue in issues
            .iter_mut()
            .filter(|issue| github_issue_needs_native_subissue_prefetch(issue, &self.config))
        {
            client.enrich_native_subissues(std::slice::from_mut(issue))?;
        }
        let mut project_states = project_state_map(issues);
        hydrate_missing_native_subissue_project_statuses(
            issues,
            &mut project_states,
            |issue_ref| client.fetch_project_issue(issue_ref),
        )?;

        Ok(())
    }

    fn enrich_missing_native_subissue_project_statuses(
        &self,
        client: &GithubProjectV2GhClient,
        issue: &mut TrackerIssue,
        project_states: &mut BTreeMap<String, String>,
    ) -> Result<(), TrackerError> {
        enrich_native_subissue_project_statuses_for_issue(issue, project_states);
        let missing_refs = native_subissue_refs_missing_project_state(issue);
        if missing_refs.is_empty() {
            return Ok(());
        }

        project_states.extend(client.fetch_project_states_for_issue_refs(&missing_refs)?);
        enrich_native_subissue_project_statuses_for_issue(issue, project_states);
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

fn github_issue_needs_native_subissue_prefetch(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
) -> bool {
    if has_native_subissue_fields(issue) {
        return false;
    }
    let state = tracker_state_key(&issue.state);
    let main_lane_state = state == tracker_state_key(&config.tracker.state_map.todo)
        || state == tracker_state_key(&config.tracker.state_map.rework)
        || state == tracker_state_key(&config.tracker.state_map.in_progress);
    main_lane_state
        && issue
            .description
            .as_deref()
            .map(|description| description.to_ascii_lowercase().contains("subissue"))
            .unwrap_or(false)
}

fn has_native_subissue_fields(issue: &TrackerIssue) -> bool {
    issue.project_fields.contains_key("GitHub Native Subissues")
        || issue.project_fields.contains_key("Native Subissues")
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

    fn list_queue_scan_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.load_issues(GithubProjectReadMode::QueueScan)
    }

    fn list_project_summary_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.list_queue_scan_issues()
    }

    fn list_dispatchable_issues(&self) -> Result<Vec<TrackerIssue>, TrackerError> {
        self.list_queue_scan_issues()
    }

    fn get_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
        if self.fixture_issues.is_empty()
            && self.config.tracker.fixture_path.is_none()
            && github_issue_number(issue_ref).is_some()
        {
            let client = GithubProjectV2GhClient::new(&self.config);
            let Some(mut issue) = client.fetch_project_issue(issue_ref)? else {
                return Ok(None);
            };
            if !status_is_mapped(&issue.state, &self.config) {
                return Ok(None);
            }
            self.enrich_native_subissue_project_statuses(std::slice::from_mut(&mut issue))?;
            let mut project_states = BTreeMap::new();
            self.enrich_missing_native_subissue_project_statuses(
                &client,
                &mut issue,
                &mut project_states,
            )?;
            client.enrich_native_issue_blockers(std::slice::from_mut(&mut issue))?;
            return Ok(Some(issue));
        }

        let mut issues = self.load_mapped_issues(GithubProjectReadMode::QueueScan)?;
        enrich_native_subissue_project_statuses_from_project_read(&mut issues);
        let project_context = issues.clone();
        let issue = issues
            .into_iter()
            .find(|issue| issue.id == issue_ref || issue.identifier == issue_ref)
            .map(|issue| self.hydrate_issue_evidence(issue, &project_context))
            .transpose()?;

        Ok(issue)
    }

    fn hydrate_issue_evidence(
        &self,
        mut issue: TrackerIssue,
        project_context: &[TrackerIssue],
    ) -> Result<TrackerIssue, TrackerError> {
        if self.fixture_issues.is_empty() && self.config.tracker.fixture_path.is_none() {
            let mut project_states = project_state_map(project_context);
            let client = GithubProjectV2GhClient::new(&self.config);
            client.enrich_issue_evidence(&mut issue, &project_states)?;
            self.enrich_missing_native_subissue_project_statuses(
                &client,
                &mut issue,
                &mut project_states,
            )?;
            client.enrich_native_issue_blockers(std::slice::from_mut(&mut issue))?;
        }
        Ok(issue)
    }

    fn fetch_issues_by_states(&self, states: &[String]) -> Result<Vec<TrackerIssue>, TrackerError> {
        let mut mapped_issues = self.load_mapped_issues(GithubProjectReadMode::QueueScan)?;
        self.enrich_native_subissue_project_statuses(&mut mapped_issues)?;
        let mut issues = MemoryTracker::new(mapped_issues).fetch_issues_by_states(states)?;
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
    metadata_cache: ProjectMetadataCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectV2OwnerType {
    Organization,
    User,
}

impl ProjectV2OwnerType {
    fn parse(value: &str) -> Result<Self, TrackerError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "organization" => Ok(Self::Organization),
            "user" => Ok(Self::User),
            other => Err(TrackerError::IntegrationUnavailable(format!(
                "tracker.project_owner_type must be user or organization; got {other}"
            ))),
        }
    }

    fn query_field(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::User => "user",
        }
    }

    fn as_str(self) -> &'static str {
        self.query_field()
    }
}

impl GithubProjectV2GhClient {
    fn new(config: &RuntimeConfig) -> Self {
        Self {
            config: config.clone(),
            metadata_cache: ProjectMetadataCache::default(),
        }
    }

    fn project_owner_query_order(&self) -> Result<Vec<ProjectV2OwnerType>, TrackerError> {
        project_owner_query_order(&self.config)
    }

    fn query_project_owner<F, T>(
        &self,
        operation: &str,
        mut query: F,
    ) -> Result<(ProjectV2OwnerType, T), TrackerError>
    where
        F: FnMut(ProjectV2OwnerType) -> Result<T, TrackerError>,
    {
        let mut attempts = Vec::new();

        for owner_type in self.project_owner_query_order()? {
            match query(owner_type) {
                Ok(response) => return Ok((owner_type, response)),
                Err(error) => attempts.push((owner_type, error)),
            }
        }

        Err(project_owner_query_error(operation, attempts))
    }

    fn fetch_project_issues(
        &self,
        mode: GithubProjectReadMode,
    ) -> Result<Vec<TrackerIssue>, TrackerError> {
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
        let (rest_item_overlays, rest_item_overlay_fallback) =
            match self.rest_project_item_overlays(&metadata) {
                Ok(overlays) => (overlays, None),
                Err(rest_error) => (
                    BTreeMap::new(),
                    Some(format!(
                        "REST Projects v2 item overlay fallback reason: {rest_error}; using GraphQL Project item fields where available"
                    )),
                ),
            };

        let mut issues = Vec::new();
        let mut cursor = None;

        loop {
            let response =
                self.graphql_project_page(metadata.owner_type, cursor.as_deref(), mode)?;

            let (mut page_issues, next_cursor, has_next_page) =
                issues_from_project_response(&response, &self.config)?;
            apply_rest_project_item_overlays(&mut page_issues, &rest_item_overlays);
            apply_rest_project_item_overlay_fallback(
                &mut page_issues,
                rest_item_overlay_fallback.as_deref(),
            );
            issues.append(&mut page_issues);

            if has_next_page {
                cursor = next_cursor;
            } else {
                break;
            }
        }

        Ok(issues)
    }

    fn enrich_issue_evidence(
        &self,
        issue: &mut TrackerIssue,
        project_states: &BTreeMap<String, String>,
    ) -> Result<(), TrackerError> {
        let Some(number) = github_issue_number(&issue.identifier) else {
            return Ok(());
        };
        let content = self.fetch_issue_evidence(number)?;
        merge_github_issue_evidence(issue, &content, &self.config)?;
        enrich_native_subissue_project_statuses_for_issue(issue, project_states);
        Ok(())
    }

    fn fetch_issue_evidence(&self, issue_number: u64) -> Result<serde_json::Value, TrackerError> {
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
        let response = self.graphql_magic(
            &github_issue_evidence_query(),
            &[
                ("owner", owner.to_string()),
                ("repo", repo.to_string()),
                ("number", issue_number.to_string()),
            ],
            &["number"],
        )?;
        response
            .pointer("/data/repository/issue")
            .cloned()
            .ok_or_else(|| {
                TrackerError::Payload(format!(
                    "GitHub issue evidence response missing issue #{issue_number}"
                ))
            })
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

    fn enrich_native_subissues(&self, issues: &mut [TrackerIssue]) -> Result<(), TrackerError> {
        for issue in issues {
            let Some(number) = github_issue_number(&issue.identifier) else {
                continue;
            };
            let native_subissues = self.fetch_native_subissues(number)?;
            if !native_subissues.is_empty() {
                insert_native_subissue_status_fields(
                    &mut issue.project_fields,
                    native_subissues,
                    &BTreeMap::new(),
                );
            }
        }

        Ok(())
    }

    fn fetch_project_issue(&self, issue_ref: &str) -> Result<Option<TrackerIssue>, TrackerError> {
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
        let issue_number = github_issue_number(issue_ref).ok_or_else(|| {
            TrackerError::Payload(format!(
                "issue ref {issue_ref:?} is not a GitHub issue number"
            ))
        })?;

        let response = self.graphql_magic(
            &github_issue_project_item_query(),
            &[
                ("owner", owner.to_string()),
                ("repo", repo.to_string()),
                ("number", issue_number.to_string()),
            ],
            &["number"],
        )?;

        issue_from_repository_issue_response(&response, &self.config)
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

    fn fetch_native_subissues(
        &self,
        issue_number: u64,
    ) -> Result<Vec<NativeSubissueRef>, TrackerError> {
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
            format!("repos/{owner}/{repo}/issues/{issue_number}/sub_issues"),
        ])?;

        native_subissue_refs_from_rest_response(&response)
    }

    fn fetch_project_states_for_issue_refs(
        &self,
        issue_refs: &[String],
    ) -> Result<BTreeMap<String, String>, TrackerError> {
        let mut states = BTreeMap::new();
        for issue_ref in issue_refs {
            if let Some(issue) = self.fetch_project_issue(issue_ref)? {
                states.insert(issue.identifier, issue.state);
            }
        }
        Ok(states)
    }

    fn set_state(&self, issue_ref: &str, normalized_state: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let option_name = self.state_option_name(normalized_state)?;
        if !status_update_required(&issue, &option_name) {
            return Ok(());
        }

        let item_id = issue.item_id.clone().ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "issue {issue_ref} has no ProjectV2 item id"
            ))
        })?;
        let (metadata, option_id) = self.status_option_id_with_refresh(&option_name)?;
        if let Ok(()) = self.update_project_item_field_rest(
            &issue,
            &metadata,
            &metadata.status_field(),
            ProjectFieldUpdateValue::String(option_id.clone()),
        ) {
            return Ok(());
        }

        self.graphql_update_project_single_select_field(
            metadata.project_id,
            item_id,
            metadata.status_field_id,
            option_id,
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
        let option_name = self.state_option_name(normalized_state)?;
        let (metadata, option_id) = self.status_option_id_with_refresh(&option_name)?;
        let response = self.graphql(
            GITHUB_ADD_PROJECT_ITEM_MUTATION,
            &[
                ("projectId", metadata.project_id.clone()),
                ("contentId", issue_id.to_string()),
            ],
        )?;
        let item_id = project_item_id_from_add_response(&response)?;
        self.graphql_update_project_single_select_field(
            metadata.project_id,
            item_id,
            metadata.status_field_id,
            option_id,
        )?;
        Ok(())
    }

    fn set_project_field(
        &self,
        issue_ref: &str,
        assignment: &ProjectFieldAssignment,
    ) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let item_id = issue.item_id.clone().ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "issue {issue_ref} is not a ProjectV2 item; add it to the project before setting fields"
            ))
        })?;
        let (metadata, field) = self.project_field_with_refresh(&assignment.name)?;
        let project_id = metadata.project_id.clone();
        let field_id = field.id.clone();
        match field.kind {
            ProjectFieldKind::SingleSelect => {
                let option_id =
                    self.project_field_option_id_with_refresh(&assignment.name, &assignment.value)?;
                if let Ok(()) = self.update_project_item_field_rest(
                    &issue,
                    &metadata,
                    &field,
                    ProjectFieldUpdateValue::String(option_id.clone()),
                ) {
                    return Ok(());
                }
                self.graphql_update_project_single_select_field(
                    project_id, item_id, field_id, option_id,
                )?;
            }
            ProjectFieldKind::Text => {
                if let Ok(()) = self.update_project_item_field_rest(
                    &issue,
                    &metadata,
                    &field,
                    ProjectFieldUpdateValue::String(assignment.value.clone()),
                ) {
                    return Ok(());
                }
                self.graphql_update_project_text_field(
                    project_id,
                    item_id,
                    field_id,
                    assignment.value.clone(),
                )?;
            }
            ProjectFieldKind::Number => {
                let number = assignment.value.parse::<f64>().map_err(|error| {
                    TrackerError::Payload(format!(
                        "ProjectV2 number field {:?} value {:?} is invalid: {error}",
                        assignment.name, assignment.value
                    ))
                })?;
                self.update_project_item_field_rest(
                    &issue,
                    &metadata,
                    &field,
                    ProjectFieldUpdateValue::Number(number),
                )?;
            }
            ProjectFieldKind::Date => {
                self.update_project_item_field_rest(
                    &issue,
                    &metadata,
                    &field,
                    ProjectFieldUpdateValue::String(assignment.value.clone()),
                )?;
            }
            _ => {
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "ProjectV2 field {:?} is {:?}; REST-first assignment supports single-select, text, number, and date; GraphQL fallback is unavailable for this field kind",
                    assignment.name, field.kind
                )));
            }
        }
        Ok(())
    }

    fn clear_project_field(&self, issue_ref: &str, field_name: &str) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let item_id = issue.item_id.clone().ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "issue {issue_ref} is not a ProjectV2 item; add it to the project before clearing fields"
            ))
        })?;
        let (metadata, field) = self.project_field_with_refresh(field_name)?;
        let project_id = metadata.project_id.clone();
        let field_id = field.id.clone();
        if let Ok(()) = self.update_project_item_field_rest(
            &issue,
            &metadata,
            &field,
            ProjectFieldUpdateValue::Null,
        ) {
            return Ok(());
        }

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
        self.fetch_project_issues(GithubProjectReadMode::QueueScan)?
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
        if github_issue_number(issue_ref).is_some() {
            return Ok(self
                .fetch_project_issue(issue_ref)?
                .map(|issue| issue.linked_pull_requests)
                .unwrap_or_default());
        }

        let mut issue = self.resolve_issue(issue_ref)?;
        let project_states = BTreeMap::new();
        self.enrich_issue_evidence(&mut issue, &project_states)?;
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
        self.metadata_cache
            .get_or_try_init(|| self.load_project_metadata())
    }

    fn refresh_project_metadata(&self) -> Result<ProjectMetadata, TrackerError> {
        self.metadata_cache.refresh(|| self.load_project_metadata())
    }

    fn load_project_metadata(&self) -> Result<ProjectMetadata, TrackerError> {
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

        match self.query_project_owner("ProjectV2 REST metadata", |owner_type| {
            self.rest_project_metadata(owner_type, owner, number)
        }) {
            Ok((_owner_type, metadata)) => Ok(metadata),
            Err(rest_error) => {
                let (_owner_type, response) = self
                    .query_project_owner("ProjectV2 metadata", |owner_type| {
                        self.graphql_project_metadata(owner_type, owner, number)
                    })
                    .map_err(|graphql_error| {
                        TrackerError::IntegrationUnavailable(format!(
                            "REST ProjectV2 metadata fallback reason: {rest_error}; GraphQL fallback failed: {graphql_error}"
                        ))
                    })?;

                project_metadata_from_response(&response, &self.config.tracker.status_field)
            }
        }
    }

    fn project_field_with_refresh(
        &self,
        field_name: &str,
    ) -> Result<(ProjectMetadata, ProjectFieldMetadata), TrackerError> {
        project_field_from_metadata_with_refresh(&self.metadata_cache, field_name, || {
            self.load_project_metadata()
        })
    }

    fn status_option_id_with_refresh(
        &self,
        option_name: &str,
    ) -> Result<(ProjectMetadata, String), TrackerError> {
        let (metadata, field) =
            self.project_field_with_refresh(&self.config.tracker.status_field)?;
        let Some(option_id) = field.option_id(option_name) else {
            let refreshed = self.refresh_project_metadata()?;
            let refreshed_field = refreshed
                .field(&self.config.tracker.status_field)
                .ok_or_else(|| {
                    TrackerError::IntegrationUnavailable(format!(
                        "ProjectV2 field {:?} was not found after metadata refresh",
                        self.config.tracker.status_field
                    ))
                })?;
            let option_id = refreshed_field.option_id(option_name).ok_or_else(|| {
                TrackerError::IntegrationUnavailable(format!(
                    "status option {option_name:?} was not found in ProjectV2 field {:?} after metadata refresh",
                    self.config.tracker.status_field
                ))
            })?;
            return Ok((refreshed, option_id));
        };
        Ok((metadata, option_id))
    }

    fn project_field_option_id_with_refresh(
        &self,
        field_name: &str,
        option_name: &str,
    ) -> Result<String, TrackerError> {
        let (_metadata, field) = self.project_field_with_refresh(field_name)?;
        if let Some(option_id) = field.option_id(option_name) {
            return Ok(option_id);
        }

        let refreshed = self.refresh_project_metadata()?;
        let refreshed_field = refreshed.field(field_name).ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "ProjectV2 field {field_name:?} was not found after metadata refresh"
            ))
        })?;
        refreshed_field.option_id(option_name).ok_or_else(|| {
            TrackerError::IntegrationUnavailable(format!(
                "option {option_name:?} was not found in ProjectV2 field {field_name:?} after metadata refresh"
            ))
        })
    }

    fn rest_project_metadata(
        &self,
        owner_type: ProjectV2OwnerType,
        owner: &str,
        number: u64,
    ) -> Result<ProjectMetadata, TrackerError> {
        let base_path = github_rest_project_path(owner_type, owner, number);
        let project = run_gh_api_json(vec!["api".into(), base_path.clone()])?;
        let fields = run_gh_api_json(vec![
            "api".into(),
            format!("{base_path}/fields?per_page=100"),
            "--paginate".into(),
            "--slurp".into(),
        ])?;

        rest_project_metadata_from_response(
            &project,
            &fields,
            &self.config.tracker.status_field,
            owner_type,
        )
    }

    fn rest_project_item_overlays(
        &self,
        metadata: &ProjectMetadata,
    ) -> Result<BTreeMap<String, RestProjectItemOverlay>, TrackerError> {
        let field_ids = metadata
            .supported_rest_field_ids()
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
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
        let base_path = github_rest_project_path(metadata.owner_type, owner, number);
        let endpoint = if field_ids.is_empty() {
            format!("{base_path}/items?per_page=100")
        } else {
            format!("{base_path}/items?per_page=100&fields={field_ids}")
        };
        let response = run_gh_api_json(vec![
            "api".into(),
            endpoint,
            "--paginate".into(),
            "--slurp".into(),
        ])?;
        rest_project_item_overlays_from_response(&response)
    }

    fn update_project_item_field_rest(
        &self,
        issue: &TrackerIssue,
        metadata: &ProjectMetadata,
        field: &ProjectFieldMetadata,
        value: ProjectFieldUpdateValue,
    ) -> Result<(), TrackerError> {
        let item_rest_id = project_rest_item_id(issue).ok_or_else(|| {
            TrackerError::NotImplemented(
                "REST Projects v2 item update fallback reason: current Project read lacks REST item id; using GraphQL where available"
                    .into(),
            )
        })?;
        self.update_project_item_field_rest_by_id(metadata, item_rest_id, field, value)
    }

    fn update_project_item_field_rest_by_id(
        &self,
        metadata: &ProjectMetadata,
        item_rest_id: u64,
        field: &ProjectFieldMetadata,
        value: ProjectFieldUpdateValue,
    ) -> Result<(), TrackerError> {
        let field_rest_id = field.rest_id.ok_or_else(|| {
            TrackerError::NotImplemented(format!(
                "REST Projects v2 item update fallback reason: ProjectV2 field {:?} lacks a REST field id; using GraphQL where available",
                field.name
            ))
        })?;
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
        let base_path = github_rest_project_path(metadata.owner_type, owner, number);
        let body = rest_project_item_field_update_body(field_rest_id, value)?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let body_path =
            std::env::temp_dir().join(format!("jade-symphony-project-item-field-{nonce}.json"));
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|error| TrackerError::Payload(format!("invalid REST update body: {error}")))?;
        fs::write(&body_path, body_bytes)
            .map_err(|error| TrackerError::IntegrationUnavailable(error.to_string()))?;
        let result = run_gh_api_json(vec![
            "api".into(),
            "-X".into(),
            "PATCH".into(),
            format!("{base_path}/items/{item_rest_id}"),
            "--input".into(),
            body_path.to_string_lossy().to_string(),
        ]);
        let _ = fs::remove_file(&body_path);
        result.map(|_| ())
    }

    fn graphql_update_project_single_select_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        option_id: String,
    ) -> Result<(), TrackerError> {
        self.graphql(
            GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
            &[
                ("projectId", project_id),
                ("itemId", item_id),
                ("fieldId", field_id),
                ("optionId", option_id),
            ],
        )?;
        Ok(())
    }

    fn graphql_update_project_text_field(
        &self,
        project_id: String,
        item_id: String,
        field_id: String,
        text: String,
    ) -> Result<(), TrackerError> {
        self.graphql(
            GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
            &[
                ("projectId", project_id),
                ("itemId", item_id),
                ("fieldId", field_id),
                ("text", text),
            ],
        )?;
        Ok(())
    }

    fn graphql_project_metadata(
        &self,
        owner_type: ProjectV2OwnerType,
        owner: &str,
        number: u64,
    ) -> Result<serde_json::Value, TrackerError> {
        let query = github_project_metadata_query(owner_type.query_field());

        self.graphql_magic(
            &query,
            &[("owner", owner.to_string()), ("number", number.to_string())],
            &["number"],
        )
    }

    fn graphql_project_page(
        &self,
        owner_type: ProjectV2OwnerType,
        cursor: Option<&str>,
        mode: GithubProjectReadMode,
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
                github_project_query(owner_type.query_field(), mode)
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

fn project_owner_query_order(
    config: &RuntimeConfig,
) -> Result<Vec<ProjectV2OwnerType>, TrackerError> {
    if let Some(owner_type) = config.tracker.project_owner_type.as_deref() {
        return Ok(vec![ProjectV2OwnerType::parse(owner_type)?]);
    }

    Ok(vec![
        ProjectV2OwnerType::Organization,
        ProjectV2OwnerType::User,
    ])
}

fn project_owner_query_error(
    operation: &str,
    attempts: Vec<(ProjectV2OwnerType, TrackerError)>,
) -> TrackerError {
    let Some((last_type, last_error)) = attempts.last() else {
        return TrackerError::IntegrationUnavailable(format!("{operation} failed"));
    };

    let prior_attempts_are_owner_misses = attempts
        .iter()
        .take(attempts.len().saturating_sub(1))
        .all(|(_, error)| project_owner_type_miss(error));
    if prior_attempts_are_owner_misses && !project_owner_type_miss(last_error) {
        return TrackerError::IntegrationUnavailable(format!(
            "{operation} failed as {} owner: {last_error}",
            last_type.as_str()
        ));
    }

    let details = attempts
        .iter()
        .map(|(owner_type, error)| format!("{}={error}", owner_type.as_str()))
        .collect::<Vec<_>>()
        .join("; ");
    TrackerError::IntegrationUnavailable(format!(
        "{operation} failed for ProjectV2 owner attempts: {details}"
    ))
}

fn project_owner_type_miss(error: &TrackerError) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("projectv2 organization owner lookup missed")
        || message.contains("projectv2 user owner lookup missed")
        || message.contains("could not resolve to an organization")
        || message.contains("could not resolve to a organization")
        || message.contains("could not resolve to organization")
        || message.contains("could not resolve to a user")
        || message.contains("could not resolve to an user")
        || message.contains("could not resolve to user")
        || message.contains("not an organization account")
        || message.contains("not a user account")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectMetadata {
    owner_type: ProjectV2OwnerType,
    project_id: String,
    status_field_id: String,
    status_options: Vec<(String, String)>,
    fields: Vec<ProjectFieldMetadata>,
}

impl ProjectMetadata {
    fn field(&self, name: &str) -> Option<&ProjectFieldMetadata> {
        self.fields.iter().find(|field| field.name == name)
    }

    fn status_field(&self) -> ProjectFieldMetadata {
        if let Some(field) = self
            .fields
            .iter()
            .find(|field| field.id == self.status_field_id)
        {
            return field.clone();
        }

        ProjectFieldMetadata {
            id: self.status_field_id.clone(),
            name: "Status".into(),
            kind: ProjectFieldKind::SingleSelect,
            options: self.status_options.clone(),
            rest_id: self
                .fields
                .iter()
                .find(|field| field.id == self.status_field_id)
                .and_then(|field| field.rest_id),
        }
    }

    fn supported_rest_field_ids(&self) -> Vec<u64> {
        self.fields
            .iter()
            .filter(|field| field.kind.supports_rest_update())
            .filter_map(|field| field.rest_id)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectFieldMetadata {
    id: String,
    name: String,
    kind: ProjectFieldKind,
    options: Vec<(String, String)>,
    rest_id: Option<u64>,
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

impl ProjectFieldKind {
    fn supports_rest_update(self) -> bool {
        matches!(
            self,
            Self::SingleSelect | Self::Text | Self::Number | Self::Date
        )
    }
}

#[derive(Debug, Clone, Default)]
struct ProjectMetadataCache {
    value: RefCell<Option<ProjectMetadata>>,
}

impl ProjectMetadataCache {
    fn get_or_try_init<F>(&self, fetch: F) -> Result<ProjectMetadata, TrackerError>
    where
        F: FnOnce() -> Result<ProjectMetadata, TrackerError>,
    {
        if let Some(metadata) = self.value.borrow().clone() {
            return Ok(metadata);
        }

        let metadata = fetch()?;
        *self.value.borrow_mut() = Some(metadata.clone());
        Ok(metadata)
    }

    fn refresh<F>(&self, fetch: F) -> Result<ProjectMetadata, TrackerError>
    where
        F: FnOnce() -> Result<ProjectMetadata, TrackerError>,
    {
        let metadata = fetch()?;
        *self.value.borrow_mut() = Some(metadata.clone());
        Ok(metadata)
    }
}

fn project_field_from_metadata_with_refresh<F>(
    cache: &ProjectMetadataCache,
    field_name: &str,
    fetch: F,
) -> Result<(ProjectMetadata, ProjectFieldMetadata), TrackerError>
where
    F: Fn() -> Result<ProjectMetadata, TrackerError>,
{
    let metadata = cache.get_or_try_init(&fetch)?;
    if let Some(field) = metadata.field(field_name) {
        return Ok((metadata.clone(), field.clone()));
    }

    let refreshed = cache.refresh(fetch)?;
    let field = refreshed.field(field_name).cloned().ok_or_else(|| {
        TrackerError::IntegrationUnavailable(format!(
            "ProjectV2 field {field_name:?} was not found after metadata refresh"
        ))
    })?;
    Ok((refreshed, field))
}

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
    let (owner_type, project) = response
        .pointer("/data/organization/projectV2")
        .map(|project| (ProjectV2OwnerType::Organization, project))
        .or_else(|| {
            response
                .pointer("/data/user/projectV2")
                .map(|project| (ProjectV2OwnerType::User, project))
        })
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
            owner_type,
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
        rest_id: None,
    })
}

fn rest_project_metadata_from_response(
    project: &serde_json::Value,
    fields_response: &serde_json::Value,
    status_field: &str,
    owner_type: ProjectV2OwnerType,
) -> Result<ProjectMetadata, TrackerError> {
    let project_id = project
        .get("node_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| TrackerError::Payload("REST ProjectV2 metadata missing node_id".into()))?;
    let fields =
        rest_paginated_array_items(fields_response, "REST ProjectV2 metadata fields response")?;
    let fields = fields
        .into_iter()
        .filter_map(rest_project_field_metadata)
        .collect::<Vec<_>>();
    let Some(status_field_metadata) = fields.iter().find(|field| field.name == status_field) else {
        return Err(TrackerError::Payload(format!(
            "REST ProjectV2 status field {status_field:?} was not found"
        )));
    };

    Ok(ProjectMetadata {
        owner_type,
        project_id: project_id.to_string(),
        status_field_id: status_field_metadata.id.clone(),
        status_options: status_field_metadata.options.clone(),
        fields,
    })
}

fn rest_project_field_metadata(field: &serde_json::Value) -> Option<ProjectFieldMetadata> {
    let id = field.get("node_id")?.as_str()?.to_string();
    let rest_id = field.get("id").and_then(serde_json::Value::as_u64);
    let name = field.get("name")?.as_str()?.to_string();
    let kind = match field
        .get("data_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
    {
        "single_select" => ProjectFieldKind::SingleSelect,
        "text" => ProjectFieldKind::Text,
        "number" => ProjectFieldKind::Number,
        "date" => ProjectFieldKind::Date,
        "iteration" => ProjectFieldKind::Iteration,
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
                rich_text_or_string(option.get("name")?)?,
            ))
        })
        .collect();

    Some(ProjectFieldMetadata {
        id,
        name,
        kind,
        options,
        rest_id,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestProjectItemOverlay {
    item_node_id: String,
    content_node_id: String,
    project_fields: BTreeMap<String, serde_json::Value>,
}

fn rest_project_item_overlays_from_response(
    response: &serde_json::Value,
) -> Result<BTreeMap<String, RestProjectItemOverlay>, TrackerError> {
    let items = rest_paginated_array_items(response, "REST ProjectV2 items response")?;
    let mut overlays = BTreeMap::new();
    for item in items {
        if let Some(overlay) = rest_project_item_overlay(item)? {
            overlays.insert(overlay.content_node_id.clone(), overlay);
        }
    }
    Ok(overlays)
}

fn rest_project_item_overlay(
    item: &serde_json::Value,
) -> Result<Option<RestProjectItemOverlay>, TrackerError> {
    let content = match item.get("content") {
        Some(content) if content.get("node_id").is_some() => content,
        _ => return Ok(None),
    };
    let item_rest_id = item
        .get("id")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| TrackerError::Payload("REST ProjectV2 item missing id".into()))?;
    let item_node_id = item
        .get("node_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| TrackerError::Payload("REST ProjectV2 item missing node_id".into()))?;
    let content_node_id = content
        .get("node_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            TrackerError::Payload("REST ProjectV2 item content missing node_id".into())
        })?;
    let mut project_fields = BTreeMap::new();
    project_fields.insert(
        "GitHub Project Item REST ID".into(),
        serde_json::Value::Number(item_rest_id.into()),
    );
    project_fields.insert(
        "GitHub Project Item Node ID".into(),
        serde_json::Value::String(item_node_id.to_string()),
    );
    for field in item
        .get("fields")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = field.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let value = rest_project_field_value(field);
        project_fields.insert(name.to_string(), value);
    }

    Ok(Some(RestProjectItemOverlay {
        item_node_id: item_node_id.to_string(),
        content_node_id: content_node_id.to_string(),
        project_fields,
    }))
}

fn rest_project_field_value(field: &serde_json::Value) -> serde_json::Value {
    let Some(value) = field.get("value") else {
        return serde_json::Value::Null;
    };
    match field
        .get("data_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
    {
        "single_select" => value
            .get("name")
            .and_then(rich_text_or_string)
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
        "text" | "date" => value
            .as_str()
            .map(|text| serde_json::Value::String(text.to_string()))
            .unwrap_or(serde_json::Value::Null),
        "number" => value.clone(),
        _ => value.clone(),
    }
}

fn apply_rest_project_item_overlays(
    issues: &mut [TrackerIssue],
    overlays: &BTreeMap<String, RestProjectItemOverlay>,
) {
    for issue in issues {
        if let Some(overlay) = overlays.get(&issue.id) {
            for (name, value) in &overlay.project_fields {
                issue.project_fields.insert(name.clone(), value.clone());
            }
            issue
                .item_id
                .get_or_insert_with(|| overlay.item_node_id.clone());
        }
    }
}

fn apply_rest_project_item_overlay_fallback(issues: &mut [TrackerIssue], reason: Option<&str>) {
    let Some(reason) = reason else {
        return;
    };
    for issue in issues {
        issue.project_fields.insert(
            "GitHub REST Project Item Fallback Reason".into(),
            serde_json::Value::String(reason.to_string()),
        );
    }
}

fn rest_paginated_array_items<'a>(
    response: &'a serde_json::Value,
    label: &str,
) -> Result<Vec<&'a serde_json::Value>, TrackerError> {
    let values = response
        .as_array()
        .ok_or_else(|| TrackerError::Payload(format!("{label} was not an array")))?;
    if values.is_empty() {
        return Ok(Vec::new());
    }
    if values.iter().all(serde_json::Value::is_array) {
        return Ok(values
            .iter()
            .flat_map(|page| page.as_array().into_iter().flatten())
            .collect());
    }
    if values.iter().all(serde_json::Value::is_object) {
        return Ok(values.iter().collect());
    }
    Err(TrackerError::Payload(format!(
        "{label} mixed paginated pages and item objects"
    )))
}

fn project_rest_item_id(issue: &TrackerIssue) -> Option<u64> {
    issue
        .project_fields
        .get("GitHub Project Item REST ID")
        .and_then(serde_json::Value::as_u64)
}

#[derive(Debug, Clone, PartialEq)]
enum ProjectFieldUpdateValue {
    String(String),
    Number(f64),
    Null,
}

fn rest_project_item_field_update_body(
    field_rest_id: u64,
    value: ProjectFieldUpdateValue,
) -> Result<serde_json::Value, TrackerError> {
    let value = match value {
        ProjectFieldUpdateValue::String(value) => serde_json::Value::String(value),
        ProjectFieldUpdateValue::Number(value) => {
            let number = serde_json::Number::from_f64(value).ok_or_else(|| {
                TrackerError::Payload(format!(
                    "ProjectV2 REST number value {value:?} is not finite"
                ))
            })?;
            serde_json::Value::Number(number)
        }
        ProjectFieldUpdateValue::Null => serde_json::Value::Null,
    };
    Ok(serde_json::json!({
        "fields": [
            {
                "id": field_rest_id,
                "value": value
            }
        ]
    }))
}

fn github_rest_project_path(kind: ProjectV2OwnerType, owner: &str, number: u64) -> String {
    match kind {
        ProjectV2OwnerType::Organization => format!("orgs/{owner}/projectsV2/{number}"),
        ProjectV2OwnerType::User => format!("users/{owner}/projectsV2/{number}"),
    }
}

fn rich_text_or_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("raw")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            value
                .get("html")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
}

#[cfg(test)]
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

fn issue_from_repository_issue_response(
    response: &serde_json::Value,
    config: &RuntimeConfig,
) -> Result<Option<TrackerIssue>, TrackerError> {
    let issue = response
        .pointer("/data/repository/issue")
        .ok_or_else(|| TrackerError::Payload("missing GitHub issue payload".into()))?;
    if issue.is_null() {
        return Ok(None);
    }
    let project_number = config
        .tracker
        .project_number
        .ok_or_else(|| TrackerError::IntegrationUnavailable("missing project_number".into()))?;
    let project_items = issue
        .pointer("/projectItems/nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            TrackerError::Payload("partial GitHub issue response missing projectItems".into())
        })?;
    let Some(project_item) = project_items.iter().find(|item| {
        item.pointer("/project/number")
            .and_then(serde_json::Value::as_u64)
            == Some(project_number)
    }) else {
        return Ok(None);
    };

    let item = serde_json::json!({
        "id": project_item.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "fieldValues": project_item.get("fieldValues").cloned().unwrap_or(serde_json::Value::Null),
        "content": issue,
    });
    issue_from_project_item(&item, config)
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
    insert_native_subissue_fields(&mut project_fields, content);
    let blocked_by = blocker_refs_from_project_fields(&project_fields);
    let comment_bodies = github_issue_comment_bodies(content)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let linked_pull_requests = merge_linked_pull_requests(
        pull_requests_from_issue(content),
        linked_pull_requests_from_workpads(
            &comment_bodies,
            config.tracker.owner.as_deref(),
            config.tracker.repo.as_deref(),
        ),
    );

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
        linked_pull_requests,
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

fn merge_github_issue_evidence(
    issue: &mut TrackerIssue,
    content: &serde_json::Value,
    config: &RuntimeConfig,
) -> Result<(), TrackerError> {
    let number = content.get("number").and_then(serde_json::Value::as_u64);
    if let Some(number) = number {
        issue.identifier = format!("#{number}");
    }
    if let Some(id) = content.get("id").and_then(serde_json::Value::as_str) {
        issue.id = id.to_string();
    }
    if let Some(title) = content.get("title").and_then(serde_json::Value::as_str) {
        issue.title = title.to_string();
    }
    if let Some(url) = content.get("url").and_then(serde_json::Value::as_str) {
        issue.url = Some(url.to_string());
    }
    if let Some(issue_state) = content.get("state").and_then(serde_json::Value::as_str) {
        issue.project_fields.insert(
            "GitHub Issue State".into(),
            serde_json::Value::String(issue_state.to_string()),
        );
    }

    insert_native_subissue_fields(&mut issue.project_fields, content);
    issue.blocked_by = blocker_refs_from_project_fields(&issue.project_fields);
    let comment_bodies = github_issue_comment_bodies(content)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    issue.linked_pull_requests = merge_linked_pull_requests(
        pull_requests_from_issue(content),
        linked_pull_requests_from_workpads(
            &comment_bodies,
            config.tracker.owner.as_deref(),
            config.tracker.repo.as_deref(),
        ),
    );
    issue.description =
        github_issue_description_with_workpad(content, &config.tracker.workpad.marker);
    issue.labels = string_nodes(content.pointer("/labels/nodes"), "name")
        .into_iter()
        .map(|label| label.to_lowercase())
        .collect();
    issue.assignees = string_nodes(content.pointer("/assignees/nodes"), "login");
    issue.created_at = content
        .get("createdAt")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    issue.updated_at = content
        .get("updatedAt")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);

    if number.is_none() {
        return Err(TrackerError::Payload(format!(
            "GitHub issue evidence for {} missing number",
            issue.identifier
        )));
    }

    Ok(())
}

fn native_issue_ref(issue: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    let issue = issue?;
    let number = issue.get("number").and_then(serde_json::Value::as_u64)?;
    Some(serde_json::json!({
        "id": issue.get("id").and_then(serde_json::Value::as_str),
        "identifier": format!("#{number}"),
        "state": issue.get("state").and_then(serde_json::Value::as_str),
    }))
}

fn native_issue_refs(nodes: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    nodes
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|node| native_issue_ref(Some(node)))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeSubissueRef {
    id: Option<String>,
    identifier: String,
    title: Option<String>,
    github_state: Option<String>,
    url: Option<String>,
    project_state: Option<String>,
}

fn native_subissue_refs_from_rest_response(
    response: &serde_json::Value,
) -> Result<Vec<NativeSubissueRef>, TrackerError> {
    let nodes = response.as_array().ok_or_else(|| {
        TrackerError::Payload("GitHub native subissues response was not an array".into())
    })?;

    Ok(nodes
        .iter()
        .filter_map(|node| {
            let number = node.get("number").and_then(serde_json::Value::as_u64)?;
            Some(NativeSubissueRef {
                id: node
                    .get("node_id")
                    .or_else(|| node.get("id"))
                    .and_then(|value| {
                        value
                            .as_str()
                            .map(str::to_string)
                            .or_else(|| value.as_u64().map(|number| number.to_string()))
                    }),
                identifier: format!("#{number}"),
                title: node
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                github_state: node
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                url: node
                    .get("html_url")
                    .or_else(|| node.get("url"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                project_state: None,
            })
        })
        .collect())
}

fn insert_native_subissue_fields(
    project_fields: &mut BTreeMap<String, serde_json::Value>,
    content: &serde_json::Value,
) {
    if let Some(parent) = native_issue_ref(content.get("parent")) {
        project_fields.insert("GitHub Native Parent".into(), parent);
    }
    let native_subissues = native_issue_refs(content.pointer("/subIssues/nodes"));
    if !native_subissues.is_empty() {
        let native_subissues = native_subissues
            .into_iter()
            .filter_map(|value| {
                let identifier = value
                    .get("identifier")
                    .and_then(serde_json::Value::as_str)?
                    .to_string();
                Some(NativeSubissueRef {
                    id: value
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    identifier,
                    title: None,
                    github_state: value
                        .get("state")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    url: None,
                    project_state: None,
                })
            })
            .collect();
        insert_native_subissue_status_fields(project_fields, native_subissues, &BTreeMap::new());
    }

    if let Some(parent) = content.get("parent").filter(|parent| !parent.is_null()) {
        if let Some(number) = parent.get("number").and_then(serde_json::Value::as_u64) {
            project_fields.insert(
                "Native Parent Issue".into(),
                serde_json::Value::String(format!("#{number}")),
            );
        }
        if let Some(title) = parent.get("title").and_then(serde_json::Value::as_str) {
            project_fields.insert(
                "Native Parent Title".into(),
                serde_json::Value::String(title.to_string()),
            );
        }
        if let Some(state) = parent.get("state").and_then(serde_json::Value::as_str) {
            project_fields.insert(
                "Native Parent State".into(),
                serde_json::Value::String(state.to_string()),
            );
        }
        if let Some(url) = parent.get("url").and_then(serde_json::Value::as_str) {
            project_fields.insert(
                "Native Parent URL".into(),
                serde_json::Value::String(url.to_string()),
            );
        }
    }

    let subissues = content
        .pointer("/subIssues/nodes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|subissue| {
            let number = subissue.get("number").and_then(serde_json::Value::as_u64)?;
            let state = subissue
                .get("state")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("UNKNOWN");
            Some((format!("#{number}"), state.to_string()))
        })
        .collect::<Vec<_>>();

    if !subissues.is_empty() && !project_fields.contains_key("Native Subissues") {
        project_fields.insert(
            "Native Subissue States".into(),
            serde_json::Value::String(
                subissues
                    .iter()
                    .map(|(issue, state)| format!("{issue}={state}"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        );
    }
}

fn enrich_native_subissue_project_statuses_from_project_read(issues: &mut [TrackerIssue]) {
    let project_states = project_state_map(issues);

    for issue in issues {
        enrich_native_subissue_project_statuses_for_issue(issue, &project_states);
    }
}

fn hydrate_missing_native_subissue_project_statuses<F>(
    issues: &mut [TrackerIssue],
    project_states: &mut BTreeMap<String, String>,
    mut fetch_issue: F,
) -> Result<(), TrackerError>
where
    F: FnMut(&str) -> Result<Option<TrackerIssue>, TrackerError>,
{
    let mut missing = BTreeSet::new();
    for issue in issues.iter() {
        for issue_ref in native_subissue_refs_missing_project_state(issue) {
            if !project_states.contains_key(&issue_ref) {
                missing.insert(issue_ref);
            }
        }
    }

    for issue_ref in missing {
        if let Some(issue) = fetch_issue(&issue_ref)? {
            project_states.insert(issue.identifier, issue.state);
        }
    }

    for issue in issues {
        enrich_native_subissue_project_statuses_for_issue(issue, project_states);
    }

    Ok(())
}

fn project_state_map(issues: &[TrackerIssue]) -> BTreeMap<String, String> {
    issues
        .iter()
        .map(|issue| (issue.identifier.clone(), issue.state.clone()))
        .collect()
}

fn enrich_native_subissue_project_statuses_for_issue(
    issue: &mut TrackerIssue,
    project_states: &BTreeMap<String, String>,
) {
    let mut native_subissues = native_subissues_from_project_fields(issue);
    if native_subissues.is_empty() {
        return;
    }
    for subissue in &mut native_subissues {
        if subissue_project_state_missing(subissue.project_state.as_deref()) {
            subissue.project_state = project_states.get(&subissue.identifier).cloned();
        }
    }
    insert_native_subissue_status_fields(
        &mut issue.project_fields,
        native_subissues,
        project_states,
    );
}

fn native_subissue_refs_missing_project_state(issue: &TrackerIssue) -> Vec<String> {
    native_subissues_from_project_fields(issue)
        .into_iter()
        .filter(|subissue| subissue_project_state_missing(subissue.project_state.as_deref()))
        .map(|subissue| subissue.identifier)
        .collect()
}

fn native_subissues_from_project_fields(issue: &TrackerIssue) -> Vec<NativeSubissueRef> {
    let mut subissues = Vec::new();
    if let Some(values) = issue
        .project_fields
        .get("GitHub Native Subissues")
        .and_then(serde_json::Value::as_array)
    {
        for value in values {
            if let Some(identifier) = issue_ref_from_value(value) {
                push_native_subissue_ref(
                    &mut subissues,
                    NativeSubissueRef {
                        id: value
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        identifier,
                        title: value
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        github_state: value
                            .get("state")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        url: value
                            .get("url")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        project_state: value
                            .get("project_state")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                    },
                );
            }
        }
    }
    if let Some(subissues_text) = issue
        .project_fields
        .get("Native Subissues")
        .and_then(serde_json::Value::as_str)
    {
        for identifier in subissues_text
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            push_native_subissue_ref(
                &mut subissues,
                NativeSubissueRef {
                    id: None,
                    identifier: identifier.to_string(),
                    title: None,
                    github_state: None,
                    url: None,
                    project_state: None,
                },
            );
        }
    }

    subissues
}

fn issue_ref_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("identifier")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| value.as_str().map(str::to_string))
        .or_else(|| {
            value
                .get("number")
                .and_then(serde_json::Value::as_u64)
                .map(|number| format!("#{number}"))
        })
}

fn insert_native_subissue_status_fields(
    project_fields: &mut BTreeMap<String, serde_json::Value>,
    native_subissues: Vec<NativeSubissueRef>,
    project_states: &BTreeMap<String, String>,
) {
    if native_subissues.is_empty() {
        return;
    }

    let mut normalized = Vec::new();
    for mut subissue in native_subissues {
        if subissue_project_state_missing(subissue.project_state.as_deref()) {
            subissue.project_state = project_states.get(&subissue.identifier).cloned();
        }
        push_native_subissue_ref(&mut normalized, subissue);
    }

    project_fields.insert(
        "GitHub Native Subissues".into(),
        serde_json::Value::Array(
            normalized
                .iter()
                .map(|subissue| {
                    serde_json::json!({
                        "id": subissue.id,
                        "identifier": subissue.identifier,
                        "title": subissue.title,
                        "state": subissue.github_state,
                        "url": subissue.url,
                        "project_state": subissue.project_state,
                    })
                })
                .collect(),
        ),
    );
    project_fields.insert(
        "Native Subissues".into(),
        serde_json::Value::String(
            normalized
                .iter()
                .map(|subissue| subissue.identifier.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    );
    project_fields.insert(
        "Native Subissue Project States".into(),
        serde_json::Value::String(
            normalized
                .iter()
                .map(|subissue| {
                    format!(
                        "{}={}",
                        subissue.identifier,
                        subissue.project_state.as_deref().unwrap_or("missing")
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
    );
}

fn push_native_subissue_ref(
    subissues: &mut Vec<NativeSubissueRef>,
    mut candidate: NativeSubissueRef,
) {
    if let Some(existing) = subissues
        .iter_mut()
        .find(|subissue| issue_refs_match(&subissue.identifier, &candidate.identifier))
    {
        if existing.id.is_none() {
            existing.id = candidate.id.take();
        }
        if existing.title.is_none() {
            existing.title = candidate.title.take();
        }
        if existing.github_state.is_none() {
            existing.github_state = candidate.github_state.take();
        }
        if existing.url.is_none() {
            existing.url = candidate.url.take();
        }
        if subissue_project_state_missing(existing.project_state.as_deref()) {
            existing.project_state = candidate.project_state.take();
        }
        return;
    }
    subissues.push(candidate);
}

fn subissue_project_state_missing(value: Option<&str>) -> bool {
    value
        .map(|state| normalize_state(state) == "missing")
        .unwrap_or(true)
}

fn issue_refs_match(left: &str, right: &str) -> bool {
    normalize_issue_ref(left) == normalize_issue_ref(right)
}

fn normalize_issue_ref(value: &str) -> String {
    value.trim().trim_start_matches('#').to_string()
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

fn linked_pull_requests_from_workpads(
    workpad_bodies: &[String],
    owner: Option<&str>,
    repo: Option<&str>,
) -> Vec<LinkedPullRequest> {
    let mut seen = BTreeSet::new();
    let mut linked = Vec::new();
    for body in workpad_bodies {
        for url in github_pull_request_urls(body) {
            if seen.insert(url.clone()) {
                linked.push(linked_pull_request_from_url(&url));
            }
        }
        for pr in linked_pull_request_comment_refs(body, owner, repo) {
            let key = pr
                .url
                .clone()
                .or_else(|| pr.number.map(|number| format!("#{number}")))
                .unwrap_or_default();
            if seen.insert(key) {
                linked.push(pr);
            }
        }
    }
    linked
}

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

fn github_pull_request_urls(text: &str) -> Vec<String> {
    text.split(|character: char| character.is_whitespace() || character == '<' || character == '>')
        .filter_map(clean_github_pull_request_url)
        .collect()
}

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

fn linked_pull_request_comment_refs(
    text: &str,
    owner: Option<&str>,
    repo: Option<&str>,
) -> Vec<LinkedPullRequest> {
    text.lines()
        .filter_map(|line| {
            let (_, raw_ref) = line.split_once("Jade Symphony linked pull request:")?;
            let raw_ref = raw_ref.trim();
            if raw_ref.starts_with("http://github.com/")
                || raw_ref.starts_with("https://github.com/")
            {
                return clean_github_pull_request_url(raw_ref)
                    .map(|url| linked_pull_request_from_url(&url));
            }
            let number = raw_ref
                .trim_start_matches('#')
                .split(|character: char| !character.is_ascii_digit())
                .next()
                .and_then(|number| number.parse::<u64>().ok())?;
            let url = owner
                .zip(repo)
                .map(|(owner, repo)| format!("https://github.com/{owner}/{repo}/pull/{number}"));
            Some(LinkedPullRequest {
                id: None,
                number: Some(number),
                url,
                state: None,
                is_draft: None,
                merge_state_status: None,
                review_decision: None,
                base_ref_name: None,
                head_ref_name: None,
            })
        })
        .collect()
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
mod tests;
