use std::collections::{BTreeMap, BTreeSet};

use crate::config::RuntimeConfig;
use crate::model::{
    normalize_state, BlockerRef, LinkedPullRequest, LinkedPullRequestSource, TrackerIssue,
};

use super::super::TrackerError;
use super::topology::insert_native_subissue_fields;

pub(in crate::tracker) fn github_issue_description_with_workpad(
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
    let timeline_comments = shea_symphony_timeline_comment_bodies(&comment_bodies, marker);

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

pub(in crate::tracker) fn merge_github_issue_evidence(
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

pub(in crate::tracker) fn github_issue_comment_bodies(content: &serde_json::Value) -> Vec<&str> {
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
        .find(|body| body.contains(marker) && !body.contains("Superseded Shea Symphony workpad"))
        .map(|body| (*body).to_string())
}

fn shea_symphony_timeline_comment_bodies(comment_bodies: &[&str], marker: &str) -> Vec<String> {
    comment_bodies
        .iter()
        .filter(|body| !body.contains(marker))
        .filter(|body| is_shea_symphony_timeline_comment(body))
        .map(|body| (*body).to_string())
        .collect()
}

fn is_shea_symphony_timeline_comment(body: &str) -> bool {
    [
        "## Shea Symphony Agent Review Run",
        "## Shea Symphony Rework Run",
        "## Shea Symphony Merge Run",
        "## Shea Symphony Human Review Decision",
        "## Shea Symphony Doctor Triage",
        "## Manual Agent Review Evidence",
    ]
    .iter()
    .any(|heading| body.contains(heading))
}

pub(in crate::tracker) fn blocker_refs_from_project_fields(
    project_fields: &BTreeMap<String, serde_json::Value>,
) -> Vec<BlockerRef> {
    project_fields
        .iter()
        .filter(|(name, _)| is_blocker_field(name))
        .flat_map(|(_, value)| blocker_refs_from_value(value))
        .collect()
}

pub(in crate::tracker) fn github_native_blocker_refs_from_response(
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

pub(in crate::tracker) fn merge_blocker_refs(
    existing: &mut Vec<BlockerRef>,
    incoming: Vec<BlockerRef>,
) {
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

pub(in crate::tracker) fn github_issue_number(identifier: &str) -> Option<u64> {
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

pub(in crate::tracker) fn json_number_to_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_f64().map(|number| number as i64))
}

pub(in crate::tracker) fn project_status(
    item: &serde_json::Value,
    status_field: &str,
) -> Option<String> {
    field_values(item).find_map(|value| {
        let field_name = value.pointer("/field/name")?.as_str()?;
        if field_name == status_field {
            value.get("name")?.as_str().map(ToOwned::to_owned)
        } else {
            None
        }
    })
}

pub(in crate::tracker) fn project_fields(
    item: &serde_json::Value,
) -> BTreeMap<String, serde_json::Value> {
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

pub(in crate::tracker) fn string_nodes(
    nodes: Option<&serde_json::Value>,
    field: &str,
) -> Vec<String> {
    nodes
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|node| node.get(field).and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

pub(in crate::tracker) fn pull_requests_from_issue(
    issue: &serde_json::Value,
) -> Vec<LinkedPullRequest> {
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
            source: LinkedPullRequestSource::GithubNative,
            ..Default::default()
        })
        .collect()
}

pub(in crate::tracker) fn linked_pull_requests_from_workpads(
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

pub(in crate::tracker) fn merge_linked_pull_requests(
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

pub(in crate::tracker) fn linked_pull_request_from_url(url: &str) -> LinkedPullRequest {
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
        source: LinkedPullRequestSource::FallbackDiagnostic,
    }
}

fn linked_pull_request_comment_refs(
    text: &str,
    owner: Option<&str>,
    repo: Option<&str>,
) -> Vec<LinkedPullRequest> {
    text.lines()
        .filter_map(|line| {
            let (_, raw_ref) = line.split_once("Shea Symphony linked pull request:")?;
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
                source: LinkedPullRequestSource::FallbackDiagnostic,
            })
        })
        .collect()
}
