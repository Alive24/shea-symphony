use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::RuntimeConfig;
use crate::model::{normalize_state, LinkedPullRequest, TrackerIssue};

mod project;
mod read;

use super::cli::{gh_available, run_gh_api_json, GithubCliAccess};
use super::evidence::github_issue_number;
use super::project_v2::{
    apply_rest_project_item_overlay_fallback, apply_rest_project_item_overlays,
    issues_from_project_response, project_item_id_from_add_response, ProjectFieldKind,
    ProjectFieldUpdateValue, ProjectMetadataCache, ProjectV2OwnerType,
};
use super::queries::{
    github_issue_comments_query, GITHUB_ADD_COMMENT_MUTATION, GITHUB_ADD_PROJECT_ITEM_MUTATION,
    GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION, GITHUB_CLOSE_ISSUE_MUTATION,
    GITHUB_CREATE_ISSUE_MUTATION, GITHUB_REPOSITORY_ID_QUERY, GITHUB_UPDATE_ISSUE_COMMENT_MUTATION,
};
use super::GithubProjectReadMode;
use crate::tracker::follow_up::follow_up_issue_body;
use crate::tracker::state::status_update_required;
use crate::tracker::workpad::{duplicate_workpad_body, ensure_workpad_marker, merge_workpad_body};
use crate::tracker::{
    relationship_readback_from_issue, resolve_configured_tracker_state, FollowUpIssueInput,
    IssueRelationshipReadback, ProjectFieldAssignment, TrackerError,
};

#[derive(Debug, Clone)]
pub(in crate::tracker) struct GithubProjectV2GhClient {
    config: RuntimeConfig,
    metadata_cache: ProjectMetadataCache,
}

impl GithubProjectV2GhClient {
    pub(in crate::tracker) fn new(config: &RuntimeConfig) -> Self {
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

    pub(in crate::tracker) fn fetch_project_issues(
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

    pub(in crate::tracker) fn set_state(
        &self,
        issue_ref: &str,
        normalized_state: &str,
    ) -> Result<(), TrackerError> {
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

    pub(in crate::tracker) fn upsert_workpad(
        &self,
        issue_ref: &str,
        markdown: &str,
    ) -> Result<(), TrackerError> {
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

    pub(in crate::tracker) fn add_issue_comment(
        &self,
        issue_ref: &str,
        markdown: &str,
    ) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        self.graphql(
            GITHUB_ADD_COMMENT_MUTATION,
            &[("subjectId", issue.id), ("body", markdown.to_string())],
        )?;
        Ok(())
    }

    pub(in crate::tracker) fn create_follow_up_issue(
        &self,
        input: FollowUpIssueInput,
    ) -> Result<String, TrackerError> {
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

    pub(in crate::tracker) fn update_issue_content(
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
            std::env::temp_dir().join(format!("shea-symphony-issue-body-{number}-{nonce}.md"));
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

    pub(in crate::tracker) fn add_issue_to_project(
        &self,
        issue_id: &str,
    ) -> Result<(), TrackerError> {
        self.add_issue_to_project_with_state(issue_id, "todo")
    }

    pub(in crate::tracker) fn add_issue_to_project_with_state(
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

    pub(in crate::tracker) fn set_project_field(
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

    pub(in crate::tracker) fn clear_project_field(
        &self,
        issue_ref: &str,
        field_name: &str,
    ) -> Result<(), TrackerError> {
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

    fn state_option_name(&self, state_input: &str) -> Result<String, TrackerError> {
        Ok(
            resolve_configured_tracker_state(&self.config.tracker.state_map, state_input)?
                .display_value()
                .to_string(),
        )
    }

    pub(in crate::tracker) fn link_pull_request(
        &self,
        issue_ref: &str,
        pr_ref: &str,
    ) -> Result<(), TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        self.graphql(
            GITHUB_ADD_COMMENT_MUTATION,
            &[
                ("subjectId", issue.id),
                (
                    "body",
                    format!("Shea Symphony linked pull request: {pr_ref}"),
                ),
            ],
        )?;
        Ok(())
    }

    pub(in crate::tracker) fn list_linked_pull_requests(
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

    pub(in crate::tracker) fn add_blocked_by_relationship(
        &self,
        issue_ref: &str,
        blocker_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        let issue = self.resolve_github_rest_issue_identity(issue_ref)?;
        let blocker = self.resolve_github_rest_issue_identity(blocker_ref)?;
        let readback = relationship_readback_from_issue(&self.resolve_issue(&issue.identifier)?);
        if readback.has_blocker(&blocker.identifier) {
            return Ok(readback);
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
        GithubCliAccess::run_status(
            github_add_blocked_by_args(owner, repo, issue.number, blocker.rest_id),
            "github issue dependency add",
        )?;

        let verified = relationship_readback_from_issue(&self.resolve_issue(&issue.identifier)?);
        if verified.has_blocker(&blocker.identifier) {
            Ok(verified)
        } else {
            Err(TrackerError::IntegrationUnavailable(format!(
                "blocked-by relationship readback missing: issue={} blocked_by={}",
                issue.identifier, blocker.identifier
            )))
        }
    }

    pub(in crate::tracker) fn add_subissue_relationship(
        &self,
        parent_ref: &str,
        subissue_ref: &str,
    ) -> Result<IssueRelationshipReadback, TrackerError> {
        let parent = self.resolve_github_rest_issue_identity(parent_ref)?;
        let subissue = self.resolve_github_rest_issue_identity(subissue_ref)?;
        let readback = relationship_readback_from_issue(&self.resolve_issue(&parent.identifier)?);
        if readback.has_native_subissue(&subissue.identifier) {
            return Ok(readback);
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
        GithubCliAccess::run_status(
            github_add_subissue_args(owner, repo, parent.number, subissue.rest_id),
            "github native subissue add",
        )?;

        let verified = relationship_readback_from_issue(&self.resolve_issue(&parent.identifier)?);
        if verified.has_native_subissue(&subissue.identifier) {
            Ok(verified)
        } else {
            Err(TrackerError::IntegrationUnavailable(format!(
                "native subissue relationship readback missing: parent={} subissue={}",
                parent.identifier, subissue.identifier
            )))
        }
    }

    fn resolve_github_rest_issue_identity(
        &self,
        issue_ref: &str,
    ) -> Result<GithubRestIssueIdentity, TrackerError> {
        let issue = self.resolve_issue(issue_ref)?;
        let number = github_issue_number(&issue.identifier).ok_or_else(|| {
            TrackerError::Payload(format!(
                "issue ref {issue_ref:?} did not resolve to a GitHub issue number"
            ))
        })?;
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
            format!("repos/{owner}/{repo}/issues/{number}"),
        ])?;
        let rest_id = response
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                TrackerError::Payload(format!(
                    "GitHub REST issue response missing numeric id for {}",
                    issue.identifier
                ))
            })?;

        Ok(GithubRestIssueIdentity {
            number,
            rest_id,
            identifier: issue.identifier,
        })
    }

    pub(in crate::tracker) fn close_issue(&self, issue_ref: &str) -> Result<(), TrackerError> {
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
}

pub(in crate::tracker) fn project_owner_query_order(
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

pub(in crate::tracker) fn project_owner_query_error(
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
struct GithubRestIssueIdentity {
    number: u64,
    rest_id: u64,
    identifier: String,
}

fn github_add_blocked_by_args(
    owner: &str,
    repo: &str,
    issue_number: u64,
    blocker_rest_id: u64,
) -> Vec<String> {
    vec![
        "api".into(),
        "-X".into(),
        "POST".into(),
        format!("repos/{owner}/{repo}/issues/{issue_number}/dependencies/blocked_by"),
        "-F".into(),
        format!("issue_id={blocker_rest_id}"),
    ]
}

fn github_add_subissue_args(
    owner: &str,
    repo: &str,
    parent_number: u64,
    subissue_rest_id: u64,
) -> Vec<String> {
    vec![
        "api".into(),
        "-X".into(),
        "POST".into(),
        format!("repos/{owner}/{repo}/issues/{parent_number}/sub_issues"),
        "-F".into(),
        format!("sub_issue_id={subissue_rest_id}"),
    ]
}
