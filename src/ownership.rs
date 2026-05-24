use serde::{Deserialize, Serialize};

const OWNERSHIP_MARKER_START: &str = "<!-- shea-symphony-runtime-ownership -->";
const OWNERSHIP_MARKER_END: &str = "<!-- /shea-symphony-runtime-ownership -->";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOwnershipMarker {
    pub issue_ref: String,
    pub actor_role: String,
    pub actor_label: String,
    pub profile_id: Option<String>,
    pub instance_name: Option<String>,
    pub workspace_key: String,
    pub branch_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeOwnershipDecision {
    Missing,
    Matches,
    Mismatched {
        reason: String,
        existing: RuntimeOwnershipMarker,
    },
}

pub fn render_runtime_ownership_marker(marker: &RuntimeOwnershipMarker) -> String {
    [
        OWNERSHIP_MARKER_START.to_string(),
        "### Runtime Ownership".to_string(),
        format!("- Issue: `{}`", marker.issue_ref),
        format!("- Actor role: `{}`", marker.actor_role),
        format!("- Actor label: `{}`", marker.actor_label),
        format!(
            "- Profile: `{}`",
            marker.profile_id.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Instance: `{}`",
            marker.instance_name.as_deref().unwrap_or("n/a")
        ),
        format!("- Workspace key: `{}`", marker.workspace_key),
        format!("- Branch: `{}`", marker.branch_name),
        OWNERSHIP_MARKER_END.to_string(),
    ]
    .join("\n")
}

pub fn runtime_ownership_from_text(text: &str) -> Option<RuntimeOwnershipMarker> {
    let block = ownership_block(text)?;
    Some(RuntimeOwnershipMarker {
        issue_ref: ownership_value(block, "Issue")?,
        actor_role: ownership_value(block, "Actor role")?,
        actor_label: ownership_value(block, "Actor label")?,
        profile_id: optional_ownership_value(block, "Profile"),
        instance_name: optional_ownership_value(block, "Instance"),
        workspace_key: ownership_value(block, "Workspace key")?,
        branch_name: ownership_value(block, "Branch")?,
    })
}

pub fn runtime_ownership_decision(
    text: Option<&str>,
    expected: &RuntimeOwnershipMarker,
) -> RuntimeOwnershipDecision {
    let Some(existing) = text.and_then(runtime_ownership_from_text) else {
        return RuntimeOwnershipDecision::Missing;
    };

    if existing.issue_ref != expected.issue_ref {
        return RuntimeOwnershipDecision::Mismatched {
            reason: format!(
                "ownership marker is for {}, expected {}",
                existing.issue_ref, expected.issue_ref
            ),
            existing,
        };
    }

    if let Some(reason) = ownership_mismatch_reason(&existing, expected) {
        return RuntimeOwnershipDecision::Mismatched { reason, existing };
    }

    RuntimeOwnershipDecision::Matches
}

fn ownership_mismatch_reason(
    existing: &RuntimeOwnershipMarker,
    expected: &RuntimeOwnershipMarker,
) -> Option<String> {
    for (label, actual, expected_value) in [
        (
            "profile",
            existing.profile_id.as_deref(),
            expected.profile_id.as_deref(),
        ),
        (
            "instance",
            existing.instance_name.as_deref(),
            expected.instance_name.as_deref(),
        ),
        (
            "workspace_key",
            Some(existing.workspace_key.as_str()),
            Some(expected.workspace_key.as_str()),
        ),
        (
            "branch",
            Some(existing.branch_name.as_str()),
            Some(expected.branch_name.as_str()),
        ),
    ] {
        if actual != expected_value {
            return Some(format!(
                "{label} differs: existing `{}` expected `{}`",
                actual.unwrap_or("n/a"),
                expected_value.unwrap_or("n/a")
            ));
        }
    }
    None
}

fn ownership_block(text: &str) -> Option<&str> {
    let start = text.find(OWNERSHIP_MARKER_START)?;
    let after_start = &text[start + OWNERSHIP_MARKER_START.len()..];
    let end = after_start.find(OWNERSHIP_MARKER_END)?;
    Some(&after_start[..end])
}

fn ownership_value(block: &str, key: &str) -> Option<String> {
    block.lines().find_map(|line| {
        let line = line.trim();
        let raw = line.strip_prefix("- ")?;
        let (label, value) = raw.split_once(':')?;
        if label.trim() != key {
            return None;
        }
        Some(clean_value(value))
    })
}

fn optional_ownership_value(block: &str, key: &str) -> Option<String> {
    ownership_value(block, key).filter(|value| value != "n/a")
}

fn clean_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('`')
        .trim()
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(profile: Option<&str>) -> RuntimeOwnershipMarker {
        RuntimeOwnershipMarker {
            issue_ref: "#99".into(),
            actor_role: "implementation_agent".into(),
            actor_label: "Shea Symphony Agent".into(),
            profile_id: profile.map(str::to_string),
            instance_name: profile.map(|value| format!("{value} instance")),
            workspace_key: "codex-alpha-_99".into(),
            branch_name: "feature/issue-99-runtime-ownership".into(),
        }
    }

    #[test]
    fn renders_and_parses_runtime_ownership_marker() {
        let marker = marker(Some("codex-alpha"));
        let body = render_runtime_ownership_marker(&marker);

        assert_eq!(runtime_ownership_from_text(&body), Some(marker));
    }

    #[test]
    fn ownership_decision_allows_missing_or_matching_marker() {
        let marker = marker(Some("codex-alpha"));
        let body = render_runtime_ownership_marker(&marker);

        assert_eq!(
            runtime_ownership_decision(None, &marker),
            RuntimeOwnershipDecision::Missing
        );
        assert_eq!(
            runtime_ownership_decision(Some(&body), &marker),
            RuntimeOwnershipDecision::Matches
        );
    }

    #[test]
    fn ownership_decision_detects_mismatched_profile() {
        let existing = marker(Some("codex-alpha"));
        let expected = marker(Some("codex-beta"));
        let body = render_runtime_ownership_marker(&existing);

        let decision = runtime_ownership_decision(Some(&body), &expected);

        assert!(matches!(
            decision,
            RuntimeOwnershipDecision::Mismatched { .. }
        ));
    }
}
