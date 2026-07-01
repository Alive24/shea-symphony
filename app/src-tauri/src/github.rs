use std::process::Command;

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::{target_context::TargetContext, workspace::WorkspaceManager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubUserSnapshot {
    available: bool,
    login: String,
    name: String,
    email: String,
    avatar_url: String,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueTimelineSnapshot {
    available: bool,
    status: String,
    repository: String,
    issue_ref: String,
    issue: Option<IssueSnapshot>,
    comments: Vec<IssueCommentSnapshot>,
    timeline_events: Vec<IssueTimelineEventSnapshot>,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueSnapshot {
    number: u64,
    title: String,
    state: String,
    url: String,
    created_at: String,
    updated_at: String,
    closed_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommentSnapshot {
    id: u64,
    url: String,
    author: String,
    created_at: String,
    updated_at: String,
    body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueTimelineEventSnapshot {
    id: u64,
    event: String,
    url: String,
    actor: String,
    created_at: String,
}

#[tauri::command]
pub async fn get_github_user() -> Result<GitHubUserSnapshot, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let output = Command::new("gh")
            .args(["api", "user"])
            .output()
            .map_err(|error| format!("failed to run gh api user: {error}"))?;
        if !output.status.success() {
            return Ok(GitHubUserSnapshot {
                available: false,
                login: String::new(),
                name: String::new(),
                email: String::new(),
                avatar_url: String::new(),
                error: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let user: Value = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("invalid gh user JSON: {error}"))?;
        Ok(GitHubUserSnapshot {
            available: true,
            login: user
                .get("login")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: user
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            email: user
                .get("email")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            avatar_url: user
                .get("avatar_url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            error: String::new(),
        })
    })
    .await
    .map_err(|error| format!("github user task failed: {error}"))?
}

#[tauri::command]
pub async fn get_issue_timeline(
    workspace: State<'_, WorkspaceManager>,
    issue_ref: String,
) -> Result<IssueTimelineSnapshot, String> {
    let workspace_profile = workspace.current();
    tauri::async_runtime::spawn_blocking(move || {
        let context = TargetContext::from_workspace(&workspace_profile);
        let Some(repository) = context.repository else {
            return Ok(unavailable_issue_timeline(
                "",
                &issue_ref,
                "Target GitHub repository is not configured.",
            ));
        };
        read_issue_timeline(&repository, &issue_ref)
    })
    .await
    .map_err(|error| format!("github issue timeline task failed: {error}"))?
}

fn read_issue_timeline(repository: &str, issue_ref: &str) -> Result<IssueTimelineSnapshot, String> {
    let Some(number) = issue_number(issue_ref) else {
        return Ok(unavailable_issue_timeline(
            repository,
            issue_ref,
            "Issue timeline reads require a numeric issue reference.",
        ));
    };
    let issue_endpoint = format!("repos/{repository}/issues/{number}");
    let issue_output = Command::new("gh")
        .args(["api", &issue_endpoint])
        .output()
        .map_err(|error| format!("failed to run gh api issue read: {error}"))?;
    if !issue_output.status.success() {
        return Ok(unavailable_issue_timeline(
            repository,
            issue_ref,
            &String::from_utf8_lossy(&issue_output.stderr),
        ));
    }
    let issue_json: Value = serde_json::from_slice(&issue_output.stdout)
        .map_err(|error| format!("invalid gh issue JSON: {error}"))?;

    let comments_endpoint = format!("repos/{repository}/issues/{number}/comments?per_page=100");
    let comments_output = Command::new("gh")
        .args(["api", "--paginate", "--slurp", &comments_endpoint])
        .output()
        .map_err(|error| format!("failed to run gh api issue comments read: {error}"))?;
    if !comments_output.status.success() {
        return Ok(unavailable_issue_timeline(
            repository,
            issue_ref,
            &String::from_utf8_lossy(&comments_output.stderr),
        ));
    }
    let comments_json: Value = serde_json::from_slice(&comments_output.stdout)
        .map_err(|error| format!("invalid gh issue comments JSON: {error}"))?;
    let timeline_endpoint = format!("repos/{repository}/issues/{number}/timeline?per_page=100");
    let timeline_output = Command::new("gh")
        .args(["api", "--paginate", "--slurp", &timeline_endpoint])
        .output()
        .map_err(|error| format!("failed to run gh api issue timeline read: {error}"))?;
    if !timeline_output.status.success() {
        return Ok(unavailable_issue_timeline(
            repository,
            issue_ref,
            &String::from_utf8_lossy(&timeline_output.stderr),
        ));
    }
    let timeline_json: Value = serde_json::from_slice(&timeline_output.stdout)
        .map_err(|error| format!("invalid gh issue timeline JSON: {error}"))?;

    Ok(IssueTimelineSnapshot {
        available: true,
        status: "available".into(),
        repository: repository.into(),
        issue_ref: format!("#{number}"),
        issue: Some(issue_snapshot(number, &issue_json)),
        comments: comment_values(&comments_json)
            .into_iter()
            .map(issue_comment_snapshot)
            .collect(),
        timeline_events: comment_values(&timeline_json)
            .into_iter()
            .map(issue_timeline_event_snapshot)
            .collect(),
        error: String::new(),
    })
}

fn unavailable_issue_timeline(
    repository: &str,
    issue_ref: &str,
    error: &str,
) -> IssueTimelineSnapshot {
    IssueTimelineSnapshot {
        available: false,
        status: "unavailable".into(),
        repository: repository.into(),
        issue_ref: issue_ref.into(),
        issue: None,
        comments: vec![],
        timeline_events: vec![],
        error: error.trim().into(),
    }
}

fn issue_snapshot(number: u64, issue: &Value) -> IssueSnapshot {
    IssueSnapshot {
        number,
        title: string_field(issue, "title"),
        state: string_field(issue, "state"),
        url: string_field(issue, "html_url"),
        created_at: string_field(issue, "created_at"),
        updated_at: string_field(issue, "updated_at"),
        closed_at: string_field(issue, "closed_at"),
    }
}

fn issue_comment_snapshot(comment: &Value) -> IssueCommentSnapshot {
    IssueCommentSnapshot {
        id: comment.get("id").and_then(Value::as_u64).unwrap_or(0),
        url: string_field(comment, "html_url"),
        author: comment
            .pointer("/user/login")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        created_at: string_field(comment, "created_at"),
        updated_at: string_field(comment, "updated_at"),
        body: string_field(comment, "body"),
    }
}

fn issue_timeline_event_snapshot(event: &Value) -> IssueTimelineEventSnapshot {
    IssueTimelineEventSnapshot {
        id: event.get("id").and_then(Value::as_u64).unwrap_or(0),
        event: string_field(event, "event"),
        url: string_field(event, "html_url"),
        actor: event
            .pointer("/actor/login")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        created_at: string_field(event, "created_at"),
    }
}

fn comment_values(value: &Value) -> Vec<&Value> {
    let Some(values) = value.as_array() else {
        return vec![];
    };
    if values.iter().all(Value::is_array) {
        values
            .iter()
            .flat_map(|page| page.as_array().into_iter().flatten())
            .collect()
    } else {
        values.iter().collect()
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .into()
}

fn issue_number(issue_ref: &str) -> Option<u64> {
    let trimmed = issue_ref.trim().trim_start_matches('#');
    let digits = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_issue_numbers() {
        assert_eq!(issue_number("#430"), Some(430));
        assert_eq!(issue_number("430"), Some(430));
        assert_eq!(issue_number("#430 extra"), Some(430));
        assert_eq!(issue_number("issue-430"), None);
    }

    #[test]
    fn flattens_paginated_comment_json() {
        let comments = json!([
            [{"id": 1, "body": "first"}],
            [{"id": 2, "body": "second"}]
        ]);
        let ids = comment_values(&comments)
            .into_iter()
            .map(|comment| comment.get("id").and_then(Value::as_u64))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec![Some(1), Some(2)]);
    }
}
