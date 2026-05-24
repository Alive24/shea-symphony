use std::collections::BTreeMap;

use crate::model::TrackerIssue;
use crate::tracker::TrackerError;

use super::metadata::{
    ProjectFieldKind, ProjectFieldMetadata, ProjectMetadata, ProjectV2OwnerType,
};

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
