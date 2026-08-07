use std::collections::BTreeMap;
use std::process::Command;

use crate::config::RuntimeConfig;
use crate::model::{normalize_state, BlockerRef, LinkedPullRequest, TrackerIssue};

use super::follow_up::follow_up_issue_body;
use super::workpad::ensure_workpad_marker;
use super::{
    issue_matches_assignee_filter, json_number_to_i64, load_fixture,
    resolve_configured_tracker_state, string_nodes, FollowUpIssueInput, MemoryTracker,
    TrackerAdapter, TrackerError,
};

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
            &format!("Shea Symphony linked pull request: {pr_ref}"),
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
        .filter(|issue| {
            !issue.assignees.is_empty()
                && (config
                    .tracker
                    .assignee_filter
                    .additional_assignees
                    .is_empty()
                    || issue_matches_assignee_filter(issue, &config.tracker.assignee_filter, None))
        })
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
query SheaSymphonyLinearIssues($projectSlug: String!, $stateNames: [String!]!, $first: Int!, $relationFirst: Int!, $after: String) {
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
query SheaSymphonyLinearIssue($issueId: String!, $relationFirst: Int!) {
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
query SheaSymphonyLinearResolveStateId($issueId: String!, $stateName: String!) {
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
query SheaSymphonyLinearIssueComments($issueId: String!) {
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
query SheaSymphonyLinearProject($projectSlug: String!) {
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
mutation SheaSymphonyLinearCreateComment($issueId: String!, $body: String!) {
  commentCreate(input: {issueId: $issueId, body: $body}) {
    success
  }
}
"#;

const LINEAR_UPDATE_COMMENT_MUTATION: &str = r#"
mutation SheaSymphonyLinearUpdateComment($commentId: String!, $body: String!) {
  commentUpdate(id: $commentId, input: {body: $body}) {
    success
  }
}
"#;

const LINEAR_UPDATE_ISSUE_STATE_MUTATION: &str = r#"
mutation SheaSymphonyLinearUpdateIssueState($issueId: String!, $stateId: String!) {
  issueUpdate(id: $issueId, input: {stateId: $stateId}) {
    success
  }
}
"#;

const LINEAR_ADD_ISSUE_TO_PROJECT_MUTATION: &str = r#"
mutation SheaSymphonyLinearAddIssueToProject($issueId: String!, $projectId: String!) {
  issueUpdate(id: $issueId, input: {projectId: $projectId}) {
    success
  }
}
"#;

const LINEAR_CREATE_ISSUE_MUTATION: &str = r#"
mutation SheaSymphonyLinearCreateIssue($teamId: String!, $projectId: String!, $title: String!, $description: String!) {
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

pub(super) fn linear_issues_from_response(
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

pub(super) fn linear_state_option_name(
    config: &RuntimeConfig,
    state_input: &str,
) -> Result<String, TrackerError> {
    Ok(
        resolve_configured_tracker_state(&config.tracker.state_map, state_input)?
            .display_value()
            .to_string(),
    )
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

pub(super) fn linear_graphql_error_message(response: &serde_json::Value) -> Option<String> {
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
