use std::collections::BTreeMap;
use std::{fs, process::Command as ProcessCommand};

#[cfg(test)]
use crate::config::AssigneeFilter;
use crate::config::RuntimeConfig;
use crate::model::{
    native_parent_identifier, native_subissue_statuses, BlockerRef, LinkedPullRequest, TrackerIssue,
};

mod error;
mod follow_up;
mod github;
mod linear;
mod memory;
mod project_field;
mod state;
mod workpad;

pub use error::{
    classify_project_state_error, classify_project_state_failure_message, ProjectStateFailureKind,
    TrackerError,
};
pub use follow_up::FollowUpIssueInput;
pub use github::GithubProjectReadMode;
pub use linear::LinearAdapter;
pub use memory::MemoryTracker;
pub use project_field::ProjectFieldAssignment;
pub use state::{claim_decision, ClaimDecision};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IssueRelationshipRef {
    pub id: Option<String>,
    pub identifier: String,
    pub title: Option<String>,
    pub github_state: Option<String>,
    pub url: Option<String>,
    pub project_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IssueRelationshipReadback {
    pub issue_identifier: String,
    pub native_parent: Option<String>,
    pub blocked_by: Vec<BlockerRef>,
    pub native_subissues: Vec<IssueRelationshipRef>,
}

impl IssueRelationshipReadback {
    pub fn has_blocker(&self, blocker_ref: &str) -> bool {
        self.blocked_by.iter().any(|blocker| {
            blocker
                .identifier
                .as_deref()
                .is_some_and(|identifier| issue_refs_match(identifier, blocker_ref))
                || blocker.id.as_deref().is_some_and(|id| id == blocker_ref)
        })
    }

    pub fn has_native_subissue(&self, subissue_ref: &str) -> bool {
        self.native_subissues
            .iter()
            .any(|subissue| issue_refs_match(&subissue.identifier, subissue_ref))
    }
}

#[cfg(test)]
use follow_up::follow_up_issue_body;
#[cfg(test)]
use github::{
    apply_rest_project_item_overlays, blocker_refs_from_project_fields,
    github_issue_description_with_workpad, github_native_blocker_refs_from_response,
    github_project_query, issue_from_repository_issue_response, issues_from_project_response,
    linked_pull_request_from_url, linked_pull_requests_from_workpads, merge_blocker_refs,
    merge_github_issue_evidence, merge_linked_pull_requests,
    project_field_from_metadata_with_refresh, project_item_id_from_add_response,
    project_metadata_from_response, project_owner_query_error, project_owner_query_order,
    project_state_error_is_retryable, rest_project_item_field_update_body,
    rest_project_item_overlays_from_response, rest_project_metadata_from_response,
    run_command_with_timeout, status_option_id, GithubAuthMode, ProjectFieldKind,
    ProjectFieldMetadata, ProjectFieldUpdateValue, ProjectMetadata, ProjectMetadataCache,
    ProjectV2OwnerType,
};
use github::{
    enrich_native_subissue_project_statuses_for_issue,
    enrich_native_subissue_project_statuses_from_project_read, gh_available, github_auth_gap,
    github_auth_mode, github_graphql_auth_smoke, github_issue_number,
    hydrate_missing_native_subissue_project_statuses, json_number_to_i64,
    native_subissue_refs_missing_project_state, project_state_map, string_nodes,
    GithubProjectV2GhClient,
};
#[cfg(test)]
use linear::{linear_graphql_error_message, linear_issues_from_response, linear_state_option_name};
#[cfg(test)]
use state::status_update_required;
use state::{issue_matches_assignee_filter, status_is_mapped, tracker_state_key};
#[cfg(test)]
use workpad::duplicate_workpad_body;

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
    fn relationship_readback(
        &self,
        issue_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support issue relationship readback for {issue_ref}",
            self.kind()
        )))
    }
    fn add_blocked_by_relationship(
        &self,
        issue_ref: &str,
        blocker_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support adding blocked-by relationship {issue_ref} <- {blocker_ref}",
            self.kind()
        )))
    }
    fn add_subissue_relationship(
        &self,
        parent_ref: &str,
        subissue_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        Err(TrackerError::NotImplemented(format!(
            "{} tracker does not support adding native subissue relationship {parent_ref} -> {subissue_ref}",
            self.kind()
        )))
    }
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

pub fn adapter_from_config(config: &RuntimeConfig) -> Box<dyn TrackerAdapter> {
    match config.tracker.kind.as_str() {
        "memory" => Box::new(MemoryTracker::from_config(config)),
        "linear" => Box::new(LinearAdapter::new(config.clone())),
        _ => Box::new(GithubProjectV2Adapter::new(config.clone())),
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
        let current_login =
            if self.fixture_issues.is_empty() && self.config.tracker.fixture_path.is_none() {
                current_gh_login()
            } else {
                None
            };
        let mut issues = apply_github_read_filters(
            self.load_mapped_issues(mode)?,
            &self.config,
            current_login.as_deref(),
        );
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
    current_login: Option<&str>,
) -> Vec<TrackerIssue> {
    apply_github_status_filters(issues, config)
        .into_iter()
        .filter(|issue| {
            issue_matches_assignee_filter(issue, &config.tracker.assignee_filter, current_login)
        })
        .collect()
}

fn current_gh_login() -> Option<String> {
    let output = ProcessCommand::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!login.is_empty()).then_some(login)
}

pub fn relationship_readback_from_issue(issue: &TrackerIssue) -> IssueRelationshipReadback {
    let native_subissues = native_subissue_statuses(issue)
        .into_iter()
        .map(|subissue| IssueRelationshipRef {
            id: None,
            identifier: subissue.identifier,
            title: None,
            github_state: subissue.github_state,
            url: None,
            project_state: subissue.project_state,
        })
        .collect();

    IssueRelationshipReadback {
        issue_identifier: issue.identifier.clone(),
        native_parent: native_parent_identifier(issue),
        blocked_by: issue.blocked_by.clone(),
        native_subissues,
    }
}

fn issue_refs_match(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    left.eq_ignore_ascii_case(right)
        || (github_issue_number(left).is_some()
            && github_issue_number(left) == github_issue_number(right))
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

    fn relationship_readback(
        &self,
        issue_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return MemoryTracker::new(self.fixture_issues.clone())
                .relationship_readback(issue_ref);
        }

        let issue = self
            .get_issue(issue_ref)?
            .ok_or_else(|| TrackerError::Payload(format!("issue not found: {issue_ref}")))?;
        Ok(relationship_readback_from_issue(&issue))
    }

    fn add_blocked_by_relationship(
        &self,
        issue_ref: &str,
        blocker_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot mutate live issue relationships".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config)
            .add_blocked_by_relationship(issue_ref, blocker_ref)
    }

    fn add_subissue_relationship(
        &self,
        parent_ref: &str,
        subissue_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        if self.config.tracker.fixture_path.is_some() {
            return Err(TrackerError::IntegrationUnavailable(
                "GitHub Project v2 fixture mode cannot mutate live issue relationships".into(),
            ));
        }

        GithubProjectV2GhClient::new(&self.config)
            .add_subissue_relationship(parent_ref, subissue_ref)
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
