use std::collections::BTreeMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::RuntimeConfig;
use crate::model::{normalize_state, BlockerRef, LinkedPullRequest, TrackerIssue};

use super::cli::{gh_available, run_gh_api_json, run_gh_graphql, GithubCliAccess};
use super::evidence::{
    github_issue_number, github_native_blocker_refs_from_response, merge_blocker_refs,
    merge_github_issue_evidence,
};
use super::project_v2::{
    apply_rest_project_item_overlay_fallback, apply_rest_project_item_overlays,
    github_rest_project_path, issue_from_repository_issue_response, issues_from_project_response,
    project_field_from_metadata_with_refresh, project_item_id_from_add_response,
    project_metadata_from_response, project_rest_item_id, rest_project_item_field_update_body,
    rest_project_item_overlays_from_response, rest_project_metadata_from_response,
    ProjectFieldKind, ProjectFieldMetadata, ProjectFieldUpdateValue, ProjectMetadata,
    ProjectMetadataCache, ProjectV2OwnerType, RestProjectItemOverlay,
};
use super::queries::{
    github_issue_comments_query, github_issue_evidence_query, github_issue_project_item_query,
    github_project_metadata_query, github_project_query, GITHUB_ADD_COMMENT_MUTATION,
    GITHUB_ADD_PROJECT_ITEM_MUTATION, GITHUB_CLEAR_PROJECT_ITEM_FIELD_MUTATION,
    GITHUB_CLOSE_ISSUE_MUTATION, GITHUB_CREATE_ISSUE_MUTATION, GITHUB_REPOSITORY_ID_QUERY,
    GITHUB_UPDATE_ISSUE_COMMENT_MUTATION, GITHUB_UPDATE_PROJECT_ITEM_FIELD_MUTATION,
    GITHUB_UPDATE_PROJECT_ITEM_TEXT_FIELD_MUTATION,
};
use super::topology::{
    enrich_native_subissue_project_statuses_for_issue, insert_native_subissue_status_fields,
    native_subissue_refs_from_rest_response, NativeSubissueRef,
};
use super::GithubProjectReadMode;
use crate::tracker::follow_up::follow_up_issue_body;
use crate::tracker::state::status_update_required;
use crate::tracker::workpad::{duplicate_workpad_body, ensure_workpad_marker, merge_workpad_body};
use crate::tracker::{FollowUpIssueInput, ProjectFieldAssignment, TrackerError};

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

    pub(in crate::tracker) fn enrich_issue_evidence(
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

    pub(in crate::tracker) fn enrich_native_issue_blockers(
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

    pub(in crate::tracker) fn enrich_native_subissues(
        &self,
        issues: &mut [TrackerIssue],
    ) -> Result<(), TrackerError> {
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

    pub(in crate::tracker) fn fetch_project_issue(
        &self,
        issue_ref: &str,
    ) -> Result<Option<TrackerIssue>, TrackerError> {
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

    pub(in crate::tracker) fn fetch_project_states_for_issue_refs(
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
                    format!("Jade Symphony linked pull request: {pr_ref}"),
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

    pub(in crate::tracker) fn update_project_item_field_rest(
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
