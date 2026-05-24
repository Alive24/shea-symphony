use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::config::RuntimeConfig;
use crate::model::TrackerIssue;

use super::super::TrackerError;
use super::evidence::{
    blocker_refs_from_project_fields, github_issue_comment_bodies,
    github_issue_description_with_workpad, json_number_to_i64, linked_pull_requests_from_workpads,
    merge_linked_pull_requests, project_fields, project_status, pull_requests_from_issue,
    string_nodes,
};
use super::topology::insert_native_subissue_fields;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tracker) enum ProjectV2OwnerType {
    Organization,
    User,
}

impl ProjectV2OwnerType {
    pub(in crate::tracker) fn parse(value: &str) -> Result<Self, TrackerError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "organization" => Ok(Self::Organization),
            "user" => Ok(Self::User),
            other => Err(TrackerError::IntegrationUnavailable(format!(
                "tracker.project_owner_type must be user or organization; got {other}"
            ))),
        }
    }

    pub(in crate::tracker) fn query_field(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::User => "user",
        }
    }

    pub(in crate::tracker) fn as_str(self) -> &'static str {
        self.query_field()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tracker) struct ProjectMetadata {
    pub(in crate::tracker) owner_type: ProjectV2OwnerType,
    pub(in crate::tracker) project_id: String,
    pub(in crate::tracker) status_field_id: String,
    pub(in crate::tracker) status_options: Vec<(String, String)>,
    pub(in crate::tracker) fields: Vec<ProjectFieldMetadata>,
}

impl ProjectMetadata {
    pub(in crate::tracker) fn field(&self, name: &str) -> Option<&ProjectFieldMetadata> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub(in crate::tracker) fn status_field(&self) -> ProjectFieldMetadata {
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

    pub(in crate::tracker) fn supported_rest_field_ids(&self) -> Vec<u64> {
        self.fields
            .iter()
            .filter(|field| field.kind.supports_rest_update())
            .filter_map(|field| field.rest_id)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::tracker) struct ProjectFieldMetadata {
    pub(in crate::tracker) id: String,
    pub(in crate::tracker) name: String,
    pub(in crate::tracker) kind: ProjectFieldKind,
    pub(in crate::tracker) options: Vec<(String, String)>,
    pub(in crate::tracker) rest_id: Option<u64>,
}

impl ProjectFieldMetadata {
    pub(in crate::tracker) fn option_id(&self, option_name: &str) -> Option<String> {
        self.options
            .iter()
            .find_map(|(id, name)| (name == option_name).then_some(id.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::tracker) enum ProjectFieldKind {
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
pub(in crate::tracker) struct ProjectMetadataCache {
    value: RefCell<Option<ProjectMetadata>>,
}

impl ProjectMetadataCache {
    pub(in crate::tracker) fn get_or_try_init<F>(
        &self,
        fetch: F,
    ) -> Result<ProjectMetadata, TrackerError>
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

    pub(in crate::tracker) fn refresh<F>(&self, fetch: F) -> Result<ProjectMetadata, TrackerError>
    where
        F: FnOnce() -> Result<ProjectMetadata, TrackerError>,
    {
        let metadata = fetch()?;
        *self.value.borrow_mut() = Some(metadata.clone());
        Ok(metadata)
    }
}

pub(in crate::tracker) fn project_field_from_metadata_with_refresh<F>(
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

#[cfg(test)]
pub(in crate::tracker) fn status_option_id(
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

pub(in crate::tracker) fn project_metadata_from_response(
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

pub(in crate::tracker) fn rest_project_metadata_from_response(
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
pub(in crate::tracker) struct RestProjectItemOverlay {
    pub(in crate::tracker) item_node_id: String,
    pub(in crate::tracker) content_node_id: String,
    pub(in crate::tracker) project_fields: BTreeMap<String, serde_json::Value>,
}

pub(in crate::tracker) fn rest_project_item_overlays_from_response(
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

pub(in crate::tracker) fn apply_rest_project_item_overlays(
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

pub(in crate::tracker) fn apply_rest_project_item_overlay_fallback(
    issues: &mut [TrackerIssue],
    reason: Option<&str>,
) {
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

pub(in crate::tracker) fn project_rest_item_id(issue: &TrackerIssue) -> Option<u64> {
    issue
        .project_fields
        .get("GitHub Project Item REST ID")
        .and_then(serde_json::Value::as_u64)
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::tracker) enum ProjectFieldUpdateValue {
    String(String),
    Number(f64),
    Null,
}

pub(in crate::tracker) fn rest_project_item_field_update_body(
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

pub(in crate::tracker) fn github_rest_project_path(
    kind: ProjectV2OwnerType,
    owner: &str,
    number: u64,
) -> String {
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
