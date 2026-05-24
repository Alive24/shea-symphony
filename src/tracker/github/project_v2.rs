use crate::config::RuntimeConfig;
use crate::model::TrackerIssue;

mod metadata;
mod rest;

use super::super::TrackerError;
use super::evidence::{
    blocker_refs_from_project_fields, github_issue_comment_bodies,
    github_issue_description_with_workpad, json_number_to_i64, linked_pull_requests_from_workpads,
    merge_linked_pull_requests, project_fields, project_status, pull_requests_from_issue,
    string_nodes,
};
use super::topology::insert_native_subissue_fields;
#[cfg(test)]
pub(in crate::tracker) use metadata::status_option_id;
pub(in crate::tracker) use metadata::{
    project_field_from_metadata_with_refresh, project_metadata_from_response, ProjectFieldKind,
    ProjectFieldMetadata, ProjectMetadata, ProjectMetadataCache, ProjectV2OwnerType,
};
pub(in crate::tracker) use rest::{
    apply_rest_project_item_overlay_fallback, apply_rest_project_item_overlays,
    github_rest_project_path, project_rest_item_id, rest_project_item_field_update_body,
    rest_project_item_overlays_from_response, rest_project_metadata_from_response,
    ProjectFieldUpdateValue, RestProjectItemOverlay,
};

pub(in crate::tracker) fn issues_from_project_response(
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

pub(in crate::tracker) fn issue_from_repository_issue_response(
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

pub(in crate::tracker) fn project_item_id_from_add_response(
    response: &serde_json::Value,
) -> Result<String, TrackerError> {
    response
        .pointer("/data/addProjectV2ItemById/item/id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            TrackerError::Payload("addProjectV2ItemById response missing item id".into())
        })
}
