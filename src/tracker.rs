use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{AssigneeFilter, RuntimeConfig};
use crate::model::{normalize_state, BlockerRef, LinkedPullRequest, TrackerIssue};

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
        || normalized.contains("failed to connect")
        || normalized.contains("connection timed out")
        || normalized.contains("timed out after")
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
        enrich_native_subissue_project_statuses_from_project_read(issues);

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
    const MAX_ATTEMPTS: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(10);

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
        let output = run_gh_command(&args, operation)?;

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
        let output = run_gh_command(args, api.operation_label())?;

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

fn run_gh_command(args: &[String], operation: &str) -> Result<Output, TrackerError> {
    run_command_with_timeout("gh", args, operation, GithubCliAccess::TIMEOUT)
}

fn run_command_with_timeout(
    program: &str,
    args: &[String],
    operation: &str,
    timeout: Duration,
) -> Result<Output, TrackerError> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base = format!("jade-symphony-command-{}-{suffix}", std::process::id());
    let stdout_path = std::env::temp_dir().join(format!("{base}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{base}.stderr"));
    let stdout_file = fs::File::create(&stdout_path).map_err(|error| {
        TrackerError::IntegrationUnavailable(format!("{operation} stdout capture failed: {error}"))
    })?;
    let stderr_file = fs::File::create(&stderr_path).map_err(|error| {
        let _ = fs::remove_file(&stdout_path);
        TrackerError::IntegrationUnavailable(format!("{operation} stderr capture failed: {error}"))
    })?;

    let mut child = Command::new(program)
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .map_err(|error| {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            TrackerError::IntegrationUnavailable(error.to_string())
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = fs::read(&stdout_path).map_err(|error| {
                    TrackerError::IntegrationUnavailable(format!(
                        "{operation} stdout read failed: {error}"
                    ))
                })?;
                let stderr = fs::read(&stderr_path).map_err(|error| {
                    TrackerError::IntegrationUnavailable(format!(
                        "{operation} stderr read failed: {error}"
                    ))
                })?;
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "{operation} timed out after {}ms",
                    timeout.as_millis()
                )));
            }
            Ok(None) => {
                let elapsed = started.elapsed();
                if elapsed >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&stdout_path);
                    let _ = fs::remove_file(&stderr_path);
                    return Err(TrackerError::IntegrationUnavailable(format!(
                        "{operation} timed out after {}ms",
                        timeout.as_millis()
                    )));
                }
                thread::sleep((timeout - elapsed).min(Duration::from_millis(100)));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(TrackerError::IntegrationUnavailable(format!(
                    "{operation} wait failed: {error}"
                )));
            }
        }
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
        ProjectStateFailureKind::Network
            | ProjectStateFailureKind::TransientBackend
            | ProjectStateFailureKind::RateLimit
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
const GITHUB_PROJECT_SUBISSUE_PAGE_SIZE: usize = 50;
const GITHUB_PROJECT_LINKED_PR_PAGE_SIZE: usize = 10;
const GITHUB_PROJECT_COMMENT_PAGE_SIZE: usize = 100;
const GITHUB_PROJECT_METADATA_FIELD_PAGE_SIZE: usize = 50;
const GITHUB_WORKPAD_COMMENT_PAGE_SIZE: usize = 50;
const GITHUB_ISSUE_PROJECT_ITEM_PAGE_SIZE: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubProjectReadMode {
    QueueScan,
    RichEvidence,
}

fn github_project_query(owner_field: &str, mode: GithubProjectReadMode) -> String {
    let rich_issue_fields = match mode {
        GithubProjectReadMode::QueueScan => String::new(),
        GithubProjectReadMode::RichEvidence => rich_issue_evidence_fields(),
    };
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
              parent {{
                id
                number
                title
                state
                url
              }}
              subIssues(first: {GITHUB_PROJECT_SUBISSUE_PAGE_SIZE}) {{
                nodes {{
                  id
                  number
                  title
                  state
                  url
                }}
              }}
{rich_issue_fields}
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

fn rich_issue_evidence_fields() -> String {
    format!(
        r#"
              body
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
              }}"#
    )
}

fn github_issue_evidence_query() -> String {
    format!(
        r#"
query JadeSymphonyIssueEvidence($owner: String!, $repo: String!, $number: Int!) {{
  repository(owner: $owner, name: $repo) {{
    issue(number: $number) {{
      id
      number
      title
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
      parent {{
        id
        number
        title
        state
        url
      }}
      subIssues(first: {GITHUB_PROJECT_SUBISSUE_PAGE_SIZE}) {{
        nodes {{
          id
          number
          title
          state
          url
        }}
      }}
{}
    }}
  }}
}}
"#,
        rich_issue_evidence_fields()
    )
}

fn github_issue_project_item_query() -> String {
    format!(
        r#"
query JadeSymphonyIssueProjectItem($owner: String!, $repo: String!, $number: Int!) {{
  repository(owner: $owner, name: $repo) {{
    issue(number: $number) {{
      __typename
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
      parent {{
        id
        number
        title
        state
        url
      }}
      subIssues(first: {GITHUB_PROJECT_SUBISSUE_PAGE_SIZE}) {{
        nodes {{
          id
          number
          title
          state
          url
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
      projectItems(first: {GITHUB_ISSUE_PROJECT_ITEM_PAGE_SIZE}) {{
        nodes {{
          id
          project {{
            number
          }}
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
    let incoming_is_canonical_workpad = is_canonical_workpad_entry(incoming_entry);

    for entry in split_workpad_entries(content) {
        let should_replace = if incoming_is_canonical_workpad {
            is_canonical_workpad_entry(&entry)
        } else {
            workpad_entry_key(&entry).as_deref() == Some(incoming_key)
        };

        if should_replace {
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

fn is_canonical_workpad_entry(entry: &str) -> bool {
    workpad_h2(entry).is_some_and(|h2| matches!(h2, "## Jade Symphony Workpad" | "## Workpad"))
}

fn workpad_h2(entry: &str) -> Option<&str> {
    entry
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("## ") && !line.starts_with("### "))
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
        if subissue.project_state.is_none() {
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
    let mut missing: Vec<String> = Vec::new();
    for subissue in native_subissues_from_project_fields(issue) {
        if subissue.project_state.is_none()
            && !missing
                .iter()
                .any(|existing| issue_refs_match(existing.as_str(), &subissue.identifier))
        {
            missing.push(subissue.identifier);
        }
    }
    missing
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
        if subissue.project_state.is_none() {
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
        if existing.project_state.is_none() {
            existing.project_state = candidate.project_state.take();
        }
        return;
    }
    subissues.push(candidate);
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
    use std::cell::Cell;
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

    fn test_status_field() -> ProjectFieldMetadata {
        ProjectFieldMetadata {
            id: "FIELD_STATUS".into(),
            name: "Status".into(),
            kind: ProjectFieldKind::SingleSelect,
            options: vec![
                ("OPT_TODO".into(), "Todo".into()),
                ("OPT_DONE".into(), "Done".into()),
            ],
            rest_id: Some(345980099),
        }
    }

    fn test_text_field(name: &str, id: &str, rest_id: Option<u64>) -> ProjectFieldMetadata {
        ProjectFieldMetadata {
            id: id.into(),
            name: name.into(),
            kind: ProjectFieldKind::Text,
            options: Vec::new(),
            rest_id,
        }
    }

    fn test_metadata(fields: Vec<ProjectFieldMetadata>) -> ProjectMetadata {
        let status = fields
            .iter()
            .find(|field| field.name == "Status")
            .cloned()
            .unwrap_or_else(test_status_field);
        ProjectMetadata {
            owner_type: ProjectV2OwnerType::User,
            project_id: "PVT_1".into(),
            status_field_id: status.id.clone(),
            status_options: status.options,
            fields,
        }
    }

    #[test]
    fn memory_tracker_filters_by_state() {
        let tracker = MemoryTracker::new(vec![issue("Todo"), issue("Done")]);
        let found = tracker.fetch_issues_by_states(&["todo".into()]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].state, "Todo");
    }

    #[test]
    fn enriches_native_subissues_with_project_statuses_from_project_read() {
        let mut parent = issue("Todo");
        parent.identifier = "#243".into();
        parent.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::json!([
                {"identifier": "#272", "state": "closed"},
                {"identifier": "#273", "state": "open"}
            ]),
        );
        let mut done = issue("Done");
        done.identifier = "#272".into();
        let mut active = issue("Agent Review");
        active.identifier = "#273".into();
        let mut issues = vec![parent, done, active];

        enrich_native_subissue_project_statuses_from_project_read(&mut issues);

        assert_eq!(
            issues[0]
                .project_fields
                .get("Native Subissue Project States")
                .and_then(serde_json::Value::as_str),
            Some("#272=Done, #273=Agent Review")
        );
        let native = issues[0]
            .project_fields
            .get("GitHub Native Subissues")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        assert_eq!(native[0]["project_state"], "Done");
        assert_eq!(native[1]["project_state"], "Agent Review");
    }

    #[test]
    fn finds_native_subissue_refs_that_still_need_project_status() {
        let mut parent = issue("Todo");
        parent.identifier = "#347".into();
        parent.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::json!([
                {"identifier": "#348", "project_state": null},
                {"identifier": "#349", "project_state": "Done"}
            ]),
        );
        parent.project_fields.insert(
            "Native Subissues".into(),
            serde_json::Value::String("#348, #350".into()),
        );

        assert_eq!(
            native_subissue_refs_missing_project_state(&parent),
            vec!["#348".to_string(), "#350".to_string()]
        );
    }

    #[test]
    fn github_queue_scan_query_omits_rich_issue_evidence() {
        let query = github_project_query("organization", GithubProjectReadMode::QueueScan);

        assert!(query.contains("fieldValues"));
        assert!(query.contains("assignees"));
        assert!(query.contains("subIssues"));
        assert!(!query.contains("body"));
        assert!(!query.contains("closedByPullRequestsReferences"));
        assert!(!query.contains("comments(first"));
        assert!(!query.contains("recentComments"));

        let rich_query = github_project_query("organization", GithubProjectReadMode::RichEvidence);
        assert!(rich_query.contains("body"));
        assert!(rich_query.contains("closedByPullRequestsReferences"));
        assert!(rich_query.contains("comments(first"));
        assert!(rich_query.contains("recentComments"));
    }

    #[test]
    fn queue_scan_project_response_does_not_require_body_comments_or_prs() {
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
                                    "id": "PVTI_7",
                                    "content": {
                                        "__typename": "Issue",
                                        "id": "I_7",
                                        "number": 7,
                                        "title": "Queue scan",
                                        "url": "https://github.com/Alive24/jade-symphony/issues/7",
                                        "state": "OPEN",
                                        "createdAt": "2026-05-21T00:00:00Z",
                                        "updatedAt": "2026-05-21T00:00:00Z",
                                        "labels": {"nodes": [{"name": "Tracker"}]},
                                        "assignees": {"nodes": [{"login": "Alive24"}]},
                                        "parent": null,
                                        "subIssues": {"nodes": []}
                                    },
                                    "fieldValues": {
                                        "nodes": [
                                            {
                                                "name": "Todo",
                                                "field": {"name": "Status"}
                                            }
                                        ]
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

        let (issues, cursor, has_next_page) = issues_from_project_response(&response, &config)
            .expect("queue scan payload should parse without rich issue evidence");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].identifier, "#7");
        assert_eq!(issues[0].state, "Todo");
        assert_eq!(issues[0].description, None);
        assert!(issues[0].linked_pull_requests.is_empty());
        assert_eq!(issues[0].assignees, vec!["Alive24"]);
        assert_eq!(cursor, None);
        assert!(!has_next_page);
    }

    #[test]
    fn rich_issue_evidence_hydration_merges_body_comments_prs_and_topology() {
        let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
        let mut issue = issue("Todo");
        issue.identifier = "#350".into();
        issue
            .project_fields
            .insert("Status".into(), serde_json::Value::String("Todo".into()));
        let content = serde_json::json!({
            "id": "I_350",
            "number": 350,
            "title": "Rich targeted read",
            "url": "https://github.com/Alive24/jade-symphony/issues/350",
            "state": "OPEN",
            "createdAt": "2026-05-21T00:00:00Z",
            "updatedAt": "2026-05-21T00:10:00Z",
            "labels": {"nodes": [{"name": "Tracker"}]},
            "assignees": {"nodes": [{"login": "Alive24"}]},
            "parent": {
                "id": "I_347",
                "number": 347,
                "title": "Parent tracker hardening",
                "state": "OPEN",
                "url": "https://github.com/Alive24/jade-symphony/issues/347"
            },
            "subIssues": {
                "nodes": [
                    {
                        "id": "I_351",
                        "number": 351,
                        "title": "Sibling",
                        "state": "OPEN",
                        "url": "https://github.com/Alive24/jade-symphony/issues/351"
                    }
                ]
            },
            "body": "Issue body evidence.",
            "closedByPullRequestsReferences": {
                "nodes": [
                    {
                        "id": "PR_9",
                        "number": 9,
                        "url": "https://github.com/Alive24/jade-symphony/pull/9",
                        "state": "OPEN",
                        "isDraft": false,
                        "baseRefName": "integration/issue-347-github-projectv2-rest-first-tracker",
                        "headRefName": "feature/issue-350"
                    }
                ]
            },
            "comments": {
                "nodes": [
                    {"body": "<!-- jade-symphony-workpad -->\n## Jade Symphony Workpad\n\nWorkpad evidence."}
                ]
            },
            "recentComments": {
                "nodes": [
                    {"body": "## Jade Symphony Agent Review Run\n\nReview pass evidence: `recorded`"}
                ]
            }
        });

        merge_github_issue_evidence(&mut issue, &content, &config).unwrap();

        let description = issue.description.as_deref().unwrap();
        assert!(description.contains("Issue body evidence."));
        assert!(description.contains("## Jade Symphony Workpad"));
        assert!(description.contains("## Jade Symphony Agent Review Run"));
        assert_eq!(issue.linked_pull_requests.len(), 1);
        assert_eq!(issue.linked_pull_requests[0].number, Some(9));
        assert_eq!(
            issue
                .project_fields
                .get("Native Parent Issue")
                .and_then(serde_json::Value::as_str),
            Some("#347")
        );
        assert_eq!(
            crate::model::native_subissue_statuses(&issue)[0].identifier,
            "#351"
        );
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
    fn project_owner_query_order_uses_explicit_user_without_org_probe() {
        let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_owner_type: user\n  project_number: 1\n---\nPrompt",
        );

        let order = project_owner_query_order(&config).unwrap();

        assert_eq!(order, vec![ProjectV2OwnerType::User]);
        let query = github_project_query(order[0].query_field(), GithubProjectReadMode::QueueScan);
        assert!(query.contains("user(login: $owner)"));
        assert!(!query.contains("organization(login: $owner)"));
    }

    #[test]
    fn project_owner_query_order_supports_explicit_org_and_legacy_fallback() {
        let org_config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_owner_type: organization\n  project_number: 1\n---\nPrompt",
        );
        assert_eq!(
            project_owner_query_order(&org_config).unwrap(),
            vec![ProjectV2OwnerType::Organization]
        );

        let fallback_config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 1\n---\nPrompt",
        );
        assert_eq!(
            project_owner_query_order(&fallback_config).unwrap(),
            vec![ProjectV2OwnerType::Organization, ProjectV2OwnerType::User]
        );
    }

    #[test]
    fn project_owner_query_error_hides_expected_owner_miss_before_real_failure() {
        let error = project_owner_query_error(
            "ProjectV2 metadata",
            vec![
                (
                    ProjectV2OwnerType::Organization,
                    TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL returned errors: Could not resolve to an Organization with the login of 'Alive24'".into(),
                    ),
                ),
                (
                    ProjectV2OwnerType::User,
                    TrackerError::IntegrationUnavailable(
                        "GitHub GraphQL operation failed kind=transient_backend: HTTP 504 Gateway Timeout".into(),
                    ),
                ),
            ],
        );
        let rendered = error.to_string();

        assert!(rendered.contains("ProjectV2 metadata failed as user owner"));
        assert!(rendered.contains("HTTP 504"));
        assert!(!rendered.contains("Organization with the login"));
        assert_eq!(
            classify_project_state_error(&error),
            ProjectStateFailureKind::TransientBackend
        );
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
                                        "parent": {
                                            "number": 243,
                                            "title": "Complete parent/subissue orchestration umbrella gating",
                                            "state": "OPEN",
                                            "url": "https://github.com/Alive24/jade-symphony/issues/243"
                                        },
                                        "subIssues": {
                                            "nodes": [
                                                {
                                                    "number": 274,
                                                    "title": "Teach lane flows about parent integration branches",
                                                    "state": "OPEN",
                                                    "url": "https://github.com/Alive24/jade-symphony/issues/274"
                                                }
                                            ]
                                        },
                                        "closedByPullRequestsReferences": {
                                            "nodes": [
                                                {
                                                    "id": "PR_1",
                                                    "number": 7,
                                                    "url": "https://github.com/Alive24/jade-symphony/pull/7",
                                                    "state": "OPEN",
                                                    "baseRefName": "integration/issue-41-parent",
                                                    "headRefName": "feature/issue-42-implement-adapter"
                                                }
                                            ]
                                        },
                                        "comments": {
                                            "nodes": [
                                                {
                                                    "body": "Jade Symphony linked pull request: https://github.com/Alive24/jade-symphony/pull/289"
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
        assert_eq!(
            issues[0]
                .project_fields
                .get("Native Parent Issue")
                .and_then(serde_json::Value::as_str),
            Some("#243")
        );
        assert_eq!(
            issues[0]
                .project_fields
                .get("Native Subissues")
                .and_then(serde_json::Value::as_str),
            Some("#274")
        );
        assert_eq!(issues[0].linked_pull_requests[0].number, Some(7));
        assert_eq!(
            issues[0].linked_pull_requests[0].base_ref_name.as_deref(),
            Some("integration/issue-41-parent")
        );
        assert_eq!(
            issues[0]
                .project_fields
                .get("GitHub Native Parent")
                .and_then(|value| value.get("identifier"))
                .and_then(serde_json::Value::as_str),
            Some("#243")
        );
        assert_eq!(
            issues[0]
                .project_fields
                .get("GitHub Native Subissues")
                .and_then(serde_json::Value::as_array)
                .and_then(|values| values.first())
                .and_then(|value| value.get("identifier"))
                .and_then(serde_json::Value::as_str),
            Some("#274")
        );
        assert!(issues[0]
            .linked_pull_requests
            .iter()
            .any(|pr| pr.number == Some(289)));
    }

    #[test]
    fn discovers_pull_request_urls_from_workpad_text() {
        let bodies = vec![format!(
            "{}\n- Live PR: `https://github.com/Alive24/jade-symphony/pull/98` (created: `true`)\n- Also see https://github.com/Alive24/jade-symphony/pull/100.\nJade Symphony linked pull request: 101",
            "<!-- jade-symphony-workpad -->"
        )];

        let prs =
            linked_pull_requests_from_workpads(&bodies, Some("Alive24"), Some("jade-symphony"));

        assert_eq!(prs.len(), 3);
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
        assert_eq!(prs[2].number, Some(101));
        assert_eq!(
            prs[2].url.as_deref(),
            Some("https://github.com/Alive24/jade-symphony/pull/101")
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
        assert_eq!(metadata.owner_type, ProjectV2OwnerType::User);
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
    fn project_metadata_cache_reuses_loaded_metadata() {
        let cache = ProjectMetadataCache::default();
        let calls = Cell::new(0);
        let metadata = test_metadata(vec![test_status_field()]);

        let first = cache
            .get_or_try_init(|| {
                calls.set(calls.get() + 1);
                Ok(metadata.clone())
            })
            .unwrap();
        let second = cache
            .get_or_try_init(|| {
                calls.set(calls.get() + 1);
                Ok(test_metadata(vec![]))
            })
            .unwrap();

        assert_eq!(calls.get(), 1);
        assert_eq!(first.project_id, "PVT_1");
        assert_eq!(second.status_field_id, "FIELD_STATUS");
    }

    #[test]
    fn project_field_lookup_refreshes_stale_metadata() {
        let cache = ProjectMetadataCache::default();
        let calls = Cell::new(0);

        let (_metadata, field) =
            project_field_from_metadata_with_refresh(&cache, "Main Agent", || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Ok(test_metadata(vec![test_status_field()]))
                } else {
                    Ok(test_metadata(vec![
                        test_status_field(),
                        test_text_field("Main Agent", "FIELD_MAIN_AGENT", Some(347408996)),
                    ]))
                }
            })
            .unwrap();

        assert_eq!(calls.get(), 2);
        assert_eq!(field.name, "Main Agent");
        assert_eq!(field.rest_id, Some(347408996));
    }

    #[test]
    fn parses_rest_project_metadata_from_paginated_fields() {
        let project = serde_json::json!({"node_id": "PVT_1"});
        let fields = serde_json::json!([
            [
                {
                    "id": 345980099,
                    "node_id": "FIELD_STATUS",
                    "name": "Status",
                    "data_type": "single_select",
                    "options": [
                        {"id": "OPT_TODO", "name": {"raw": "Todo", "html": "Todo"}},
                        {"id": "OPT_DONE", "name": {"raw": "Done", "html": "Done"}}
                    ]
                }
            ],
            [
                {
                    "id": 347408996,
                    "node_id": "FIELD_MAIN_AGENT",
                    "name": "Main Agent",
                    "data_type": "text"
                },
                {
                    "id": 348315440,
                    "node_id": "FIELD_REVIEW_AGENT",
                    "name": "Review Agent",
                    "data_type": "text"
                }
            ]
        ]);

        let metadata = rest_project_metadata_from_response(
            &project,
            &fields,
            "Status",
            ProjectV2OwnerType::Organization,
        )
        .unwrap();

        assert_eq!(metadata.project_id, "PVT_1");
        assert_eq!(metadata.owner_type, ProjectV2OwnerType::Organization);
        assert_eq!(metadata.status_field_id, "FIELD_STATUS");
        assert_eq!(
            metadata.status_options[0],
            ("OPT_TODO".into(), "Todo".into())
        );
        assert_eq!(metadata.field("Status").unwrap().rest_id, Some(345980099));
        assert_eq!(
            metadata.field("Main Agent").unwrap().rest_id,
            Some(347408996)
        );
    }

    #[test]
    fn parses_rest_project_item_overlays_from_paginated_items() {
        let response = serde_json::json!([
            [
                {
                    "id": 190539790,
                    "node_id": "PVTI_1",
                    "content": {
                        "node_id": "I_1",
                        "number": 349
                    },
                    "fields": [
                        {
                            "id": 345980099,
                            "name": "Status",
                            "data_type": "single_select",
                            "value": {"id": "OPT_TODO", "name": {"raw": "Todo"}}
                        },
                        {
                            "id": 347408996,
                            "name": "Main Agent",
                            "data_type": "text",
                            "value": "v=1 lane=main"
                        }
                    ]
                }
            ],
            [
                {
                    "id": 190539791,
                    "node_id": "PVTI_2",
                    "content": {
                        "node_id": "I_2",
                        "number": 350
                    },
                    "fields": [
                        {
                            "id": 1,
                            "name": "Priority",
                            "data_type": "number",
                            "value": 3
                        },
                        {
                            "id": 2,
                            "name": "Target Date",
                            "data_type": "date",
                            "value": "2026-05-22"
                        }
                    ]
                }
            ]
        ]);

        let overlays = rest_project_item_overlays_from_response(&response).unwrap();

        let first = overlays.get("I_1").unwrap();
        assert_eq!(first.item_node_id, "PVTI_1");
        assert_eq!(
            first
                .project_fields
                .get("GitHub Project Item REST ID")
                .and_then(serde_json::Value::as_u64),
            Some(190539790)
        );
        assert_eq!(
            first
                .project_fields
                .get("Status")
                .and_then(serde_json::Value::as_str),
            Some("Todo")
        );
        assert_eq!(
            first
                .project_fields
                .get("Main Agent")
                .and_then(serde_json::Value::as_str),
            Some("v=1 lane=main")
        );

        let mut issue = issue("Todo");
        issue.id = "I_1".into();
        apply_rest_project_item_overlays(std::slice::from_mut(&mut issue), &overlays);
        assert_eq!(issue.item_id.as_deref(), Some("PVTI_1"));
        assert_eq!(
            issue
                .project_fields
                .get("GitHub Project Item REST ID")
                .and_then(serde_json::Value::as_u64),
            Some(190539790)
        );

        let second = overlays.get("I_2").unwrap();
        assert_eq!(
            second.project_fields.get("Priority"),
            Some(&serde_json::json!(3))
        );
        assert_eq!(
            second
                .project_fields
                .get("Target Date")
                .and_then(serde_json::Value::as_str),
            Some("2026-05-22")
        );
    }

    #[test]
    fn builds_rest_project_item_field_update_payloads() {
        assert_eq!(
            rest_project_item_field_update_body(
                345980099,
                ProjectFieldUpdateValue::String("f75ad846".into())
            )
            .unwrap(),
            serde_json::json!({
                "fields": [
                    {"id": 345980099, "value": "f75ad846"}
                ]
            })
        );
        assert_eq!(
            rest_project_item_field_update_body(
                347408996,
                ProjectFieldUpdateValue::String("v=1 lane=main".into())
            )
            .unwrap(),
            serde_json::json!({
                "fields": [
                    {"id": 347408996, "value": "v=1 lane=main"}
                ]
            })
        );
        assert_eq!(
            rest_project_item_field_update_body(42, ProjectFieldUpdateValue::Null).unwrap(),
            serde_json::json!({
                "fields": [
                    {"id": 42, "value": null}
                ]
            })
        );
    }

    #[test]
    fn rest_update_reports_graphql_fallback_reasons_for_missing_rest_ids() {
        let client = GithubProjectV2GhClient::new(&github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n---\nPrompt",
        ));
        let metadata = test_metadata(vec![
            test_status_field(),
            test_text_field("Main Agent", "FIELD_MAIN_AGENT", None),
        ]);
        let field_without_rest_id = metadata.field("Main Agent").unwrap().clone();
        let mut issue_with_rest_item = issue("Todo");
        issue_with_rest_item.project_fields.insert(
            "GitHub Project Item REST ID".into(),
            serde_json::json!(190539790),
        );

        let error = client
            .update_project_item_field_rest(
                &issue_with_rest_item,
                &metadata,
                &field_without_rest_id,
                ProjectFieldUpdateValue::String("v=1 lane=main".into()),
            )
            .unwrap_err();
        assert!(format!("{error}").contains("lacks a REST field id"));
        assert!(format!("{error}").contains("using GraphQL where available"));

        let field_with_rest_id = test_text_field("Main Agent", "FIELD_MAIN_AGENT", Some(347408996));
        let issue_without_rest_item = issue("Todo");
        let error = client
            .update_project_item_field_rest(
                &issue_without_rest_item,
                &metadata,
                &field_with_rest_id,
                ProjectFieldUpdateValue::String("v=1 lane=main".into()),
            )
            .unwrap_err();
        assert!(format!("{error}").contains("current Project read lacks REST item id"));
    }

    #[test]
    fn resolves_project_status_option_id() {
        let metadata = ProjectMetadata {
            owner_type: ProjectV2OwnerType::User,
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
                rest_id: None,
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
    fn merge_workpad_body_replaces_legacy_workpad_and_stale_pr_evidence() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Workpad\n\n### Workspace Evidence\n- Workspace path: `/tmp/old`\n\n---\n\n## Jade Symphony Workpad\n\n### Planned Handoff\n- Live PR: `not-created`\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming =
            "## Jade Symphony Workpad\n\n### Planned Handoff\n- Live PR: `https://github.com/Alive24/jade-symphony/pull/337`";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(!body.contains("## Workpad"));
        assert!(!body.contains("not-created"));
        assert!(body.contains("https://github.com/Alive24/jade-symphony/pull/337"));
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
    fn targeted_github_issue_response_parses_project_item_status() {
        let config = github_config(
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n---\nPrompt",
        );
        let response = serde_json::json!({
            "data": {
                "repository": {
                    "issue": {
                        "__typename": "Issue",
                        "id": "I_349",
                        "number": 349,
                        "title": "Add ProjectV2 metadata cache",
                        "body": "Issue body",
                        "url": "https://github.com/Alive24/jade-symphony/issues/349",
                        "state": "OPEN",
                        "createdAt": "2026-05-21T00:00:00Z",
                        "updatedAt": "2026-05-21T00:10:00Z",
                        "labels": { "nodes": [{ "name": "area:tracker" }] },
                        "assignees": { "nodes": [{ "login": "Alive24" }] },
                        "parent": null,
                        "subIssues": { "nodes": [] },
                        "closedByPullRequestsReferences": { "nodes": [] },
                        "comments": { "nodes": [] },
                        "recentComments": {
                            "nodes": [
                                {
                                    "body": "Jade Symphony linked pull request: https://github.com/Alive24/jade-symphony/pull/355"
                                }
                            ]
                        },
                        "projectItems": {
                            "nodes": [
                                {
                                    "id": "PVTI_OTHER",
                                    "project": { "number": 8 },
                                    "fieldValues": { "nodes": [] }
                                },
                                {
                                    "id": "PVTI_349",
                                    "project": { "number": 9 },
                                    "fieldValues": {
                                        "nodes": [
                                            {
                                                "name": "In Progress",
                                                "field": { "name": "Status" }
                                            },
                                            {
                                                "text": "v=1 issue=#349 lane=main",
                                                "field": { "name": "Main Agent" }
                                            }
                                        ]
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        });

        let issue = issue_from_repository_issue_response(&response, &config)
            .unwrap()
            .unwrap();

        assert_eq!(issue.identifier, "#349");
        assert_eq!(issue.item_id.as_deref(), Some("PVTI_349"));
        assert_eq!(issue.state, "In Progress");
        assert_eq!(issue.assignees, vec!["Alive24"]);
        assert_eq!(
            issue.project_fields.get("Main Agent"),
            Some(&serde_json::Value::String(
                "v=1 issue=#349 lane=main".into()
            ))
        );
        assert!(issue
            .linked_pull_requests
            .iter()
            .any(|pr| pr.number == Some(355)));
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
        for status in [
            "HTTP 502 Bad Gateway",
            "HTTP 503 Service Unavailable",
            "HTTP 504 Gateway Timeout",
        ] {
            assert_eq!(
                classify_project_state_failure_message(status),
                ProjectStateFailureKind::TransientBackend
            );
            assert!(project_state_error_is_retryable(
                &TrackerError::IntegrationUnavailable(status.into())
            ));
        }
        assert_eq!(
            classify_project_state_failure_message(
                "HTTP 403 Resource not accessible by integration"
            ),
            ProjectStateFailureKind::Auth
        );
        assert_eq!(
            classify_project_state_failure_message(
                "GitHub GraphQL returned errors: Could not resolve to a ProjectV2"
            ),
            ProjectStateFailureKind::Schema
        );
        assert_eq!(
            classify_project_state_failure_message(
                "GitHub GraphQL operation timed out after 30000ms"
            ),
            ProjectStateFailureKind::Network
        );
        assert_eq!(
            classify_project_state_failure_message(
                "GitHub GraphQL operation failed: HTTP 502 Bad Gateway"
            ),
            ProjectStateFailureKind::TransientBackend
        );
        assert_eq!(
            classify_project_state_failure_message(
                "GitHub GraphQL operation failed: HTTP 500 Internal Server Error"
            ),
            ProjectStateFailureKind::TransientBackend
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

    #[cfg(unix)]
    #[test]
    fn command_timeout_returns_transient_tracker_error() {
        let args = vec!["-c".into(), "sleep 0.05".into()];
        let error = run_command_with_timeout(
            "sh",
            &args,
            "GitHub GraphQL operation",
            Duration::from_millis(1),
        )
        .unwrap_err();

        assert_eq!(
            classify_project_state_error(&error),
            ProjectStateFailureKind::Network
        );
        assert!(error.to_string().contains("timed out after"));
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
