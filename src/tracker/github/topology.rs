use std::collections::{BTreeMap, BTreeSet};

use crate::model::{normalize_state, TrackerIssue};

use super::super::TrackerError;

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
pub(in crate::tracker) struct NativeSubissueRef {
    id: Option<String>,
    identifier: String,
    title: Option<String>,
    github_state: Option<String>,
    url: Option<String>,
    project_state: Option<String>,
}

pub(in crate::tracker) fn native_subissue_refs_from_rest_response(
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

pub(in crate::tracker) fn insert_native_subissue_fields(
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

pub(in crate::tracker) fn enrich_native_subissue_project_statuses_from_project_read(
    issues: &mut [TrackerIssue],
) {
    let project_states = project_state_map(issues);

    for issue in issues {
        enrich_native_subissue_project_statuses_for_issue(issue, &project_states);
    }
}

pub(in crate::tracker) fn hydrate_missing_native_subissue_project_statuses<F>(
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

pub(in crate::tracker) fn project_state_map(issues: &[TrackerIssue]) -> BTreeMap<String, String> {
    issues
        .iter()
        .map(|issue| (issue.identifier.clone(), issue.state.clone()))
        .collect()
}

pub(in crate::tracker) fn enrich_native_subissue_project_statuses_for_issue(
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

pub(in crate::tracker) fn native_subissue_refs_missing_project_state(
    issue: &TrackerIssue,
) -> Vec<String> {
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

pub(in crate::tracker) fn insert_native_subissue_status_fields(
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
