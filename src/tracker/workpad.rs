pub(super) fn ensure_workpad_marker(markdown: &str, marker: &str) -> String {
    if markdown.contains(marker) {
        markdown.to_string()
    } else {
        format!("{marker}\n{markdown}")
    }
}

pub(super) fn merge_workpad_body(existing: &str, incoming: &str, marker: &str) -> String {
    let existing = ensure_workpad_marker(existing, marker);
    let incoming = ensure_workpad_marker(incoming, marker);
    let (mut merged, incoming_remainder) = replace_singleton_workpad_blocks(&existing, &incoming);
    let incoming_content = strip_workpad_marker(&incoming_remainder, marker);

    for entry in split_workpad_entries(incoming_content) {
        merged = merge_workpad_entry(&merged, &entry, marker);
    }

    merged
}

fn merge_workpad_entry(existing: &str, incoming_entry: &str, marker: &str) -> String {
    let incoming_entry = incoming_entry.trim();
    if incoming_entry.is_empty() || existing.contains(incoming_entry) {
        return existing.to_string();
    }

    if let Some(key) = workpad_entry_key(incoming_entry) {
        return replace_or_append_workpad_entry(existing, incoming_entry, marker, &key);
    }

    append_workpad_entry(existing, incoming_entry)
}

fn replace_or_append_workpad_entry(
    existing: &str,
    incoming_entry: &str,
    marker: &str,
    incoming_key: &str,
) -> String {
    let content = strip_workpad_marker(existing, marker);
    let mut replaced = false;
    let mut entries = Vec::new();
    let incoming_is_canonical_workpad = is_canonical_workpad_entry(incoming_entry);

    for entry in split_workpad_entries(content) {
        let should_replace = if incoming_is_canonical_workpad {
            is_canonical_workpad_entry(&entry)
        } else {
            workpad_entry_key(&entry).as_deref() == Some(incoming_key)
        };

        if should_replace {
            if !replaced {
                entries.push(incoming_entry.to_string());
                replaced = true;
            }
        } else {
            entries.push(entry);
        }
    }

    if !replaced {
        entries.push(incoming_entry.to_string());
    }

    render_workpad_entries(marker, &entries)
}

fn append_workpad_entry(existing: &str, incoming_entry: &str) -> String {
    let mut merged = existing.trim_end().to_string();
    merged.push_str("\n\n---\n\n");
    merged.push_str(incoming_entry);
    merged
}

fn split_workpad_entries(markdown: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current = Vec::new();

    for line in markdown.lines() {
        if line.trim() == "---" {
            let entry = current.join("\n").trim().to_string();
            if !entry.is_empty() {
                entries.push(entry);
            }
            current.clear();
        } else {
            current.push(line);
        }
    }

    let entry = current.join("\n").trim().to_string();
    if !entry.is_empty() {
        entries.push(entry);
    }

    entries
}

fn render_workpad_entries(marker: &str, entries: &[String]) -> String {
    if entries.is_empty() {
        marker.to_string()
    } else {
        format!("{marker}\n{}", entries.join("\n\n---\n\n"))
    }
}

fn workpad_entry_key(entry: &str) -> Option<String> {
    let mut h2 = None;
    let mut h3 = None;

    for line in entry.lines() {
        let line = line.trim();
        if h2.is_none() && line.starts_with("## ") && !line.starts_with("### ") {
            h2 = Some(line.to_string());
            continue;
        }
        if h2.is_some() && line.starts_with("### ") {
            h3 = Some(line.to_string());
            break;
        }
    }

    h2.map(|h2| match h3 {
        Some(h3) => format!("{h2}\n{h3}"),
        None => h2,
    })
}

fn is_canonical_workpad_entry(entry: &str) -> bool {
    workpad_h2(entry).is_some_and(|h2| matches!(h2, "## Jade Symphony Workpad" | "## Workpad"))
}

fn workpad_h2(entry: &str) -> Option<&str> {
    entry
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("## ") && !line.starts_with("### "))
}

fn strip_workpad_marker<'a>(markdown: &'a str, marker: &str) -> &'a str {
    markdown
        .strip_prefix(marker)
        .map(str::trim_start)
        .unwrap_or(markdown)
}

fn replace_singleton_workpad_blocks(existing: &str, incoming: &str) -> (String, String) {
    const RUNTIME_OWNERSHIP: (&str, &str, bool) = (
        "<!-- jade-symphony-runtime-ownership -->",
        "<!-- /jade-symphony-runtime-ownership -->",
        true,
    );
    const WORKSPACE_ADOPTION: (&str, &str, bool) = (
        "<!-- jade-symphony-workspace-adoption -->",
        "<!-- /jade-symphony-workspace-adoption -->",
        false,
    );

    let mut merged = existing.to_string();
    let mut remainder = incoming.to_string();
    for (start, end, strip_when_missing) in [RUNTIME_OWNERSHIP, WORKSPACE_ADOPTION] {
        let Some(incoming_block) = marked_block(&remainder, start, end).map(str::to_string) else {
            continue;
        };
        if let Some(existing_block) = marked_block(&merged, start, end).map(str::to_string) {
            merged = merged.replacen(&existing_block, &incoming_block, 1);
            remainder = remainder.replacen(&incoming_block, "", 1);
        } else if strip_when_missing {
            remainder = remainder.replacen(&incoming_block, "", 1);
        }
    }

    (merged, remainder)
}

fn marked_block<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let start_index = text.find(start)?;
    let after_start = &text[start_index + start.len()..];
    let end_offset = after_start.find(end)?;
    let end_index = start_index + start.len() + end_offset + end.len();
    Some(&text[start_index..end_index])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepends_workpad_marker_once() {
        let marker = "<!-- jade-symphony-workpad -->";
        let body = ensure_workpad_marker("## Workpad", marker);
        assert!(body.starts_with("<!-- jade-symphony-workpad -->"));
        let body = ensure_workpad_marker(&body, marker);
        let body = ensure_workpad_marker(&body, marker);
        assert_eq!(body.matches(marker).count(), 1);
    }

    #[test]
    fn merge_workpad_body_appends_without_losing_existing_sections() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing =
            format!("{marker}\n## Jade Symphony Workpad\n\n### Plan\n- [ ] inspect issue");
        let incoming = "## Agent Review\n\n### Manual Review Evidence\n````md\npass\n````";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert!(body.contains("### Plan"));
        assert!(body.contains("## Agent Review"));
        assert!(body.contains("pass"));
    }

    #[test]
    fn merge_workpad_body_appends_agent_review_handoff_without_replacing_main_workpad() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Plan\n- [x] inspect issue\n\n### Work Log\n- implemented main work"
        );
        let incoming =
            "## Agent Review Handoff\n\n### Agent Review Handoff Invariant\n- Status: `Ready`";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(body.contains("### Plan"));
        assert!(body.contains("### Work Log"));
        assert!(body.contains("- implemented main work"));
        assert!(body.contains("## Agent Review Handoff"));
        assert!(body.contains("### Agent Review Handoff Invariant"));
    }

    #[test]
    fn merge_workpad_body_appends_distinct_review_attempts() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Agent Review\n\n- Reviewer backend: gemini-cli\n\n### Review Attempt gemini-old\n- Review pass evidence: `recorded`"
        );
        let incoming = "## Agent Review\n\n- Reviewer backend: gemini-cli\n\n### Review Attempt gemini-new\n- [Confirmed] Bug: needs rework";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Agent Review").count(), 2);
        assert!(body.contains("### Review Attempt gemini-old"));
        assert!(body.contains("### Review Attempt gemini-new"));
        assert!(body.contains("Review pass evidence: `recorded`"));
        assert!(body.contains("[Confirmed] Bug"));
    }

    #[test]
    fn merge_workpad_body_replaces_matching_jade_symphony_workpad_entry() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Context\n- old context\n\n### Plan\n- [ ] old plan\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming =
            "## Jade Symphony Workpad\n\n### Context\n- updated context\n\n### Plan\n- [x] updated plan";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(body.contains("- updated context"));
        assert!(body.contains("- [x] updated plan"));
        assert!(!body.contains("- old context"));
        assert!(body.contains("## Agent Review"));
        assert!(body.contains("Manual Review Evidence"));
    }

    #[test]
    fn merge_workpad_body_collapses_duplicate_matching_entries() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Context\n- first\n\n---\n\n## Jade Symphony Workpad\n\n### Context\n- duplicate\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming = "## Jade Symphony Workpad\n\n### Context\n- final";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(body.contains("- final"));
        assert!(!body.contains("- first"));
        assert!(!body.contains("- duplicate"));
        assert!(body.contains("## Agent Review"));
    }

    #[test]
    fn merge_workpad_body_replaces_legacy_workpad_and_stale_pr_evidence() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Workpad\n\n### Workspace Evidence\n- Workspace path: `/tmp/old`\n\n---\n\n## Jade Symphony Workpad\n\n### Planned Handoff\n- Live PR: `not-created`\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming =
            "## Jade Symphony Workpad\n\n### Planned Handoff\n- Live PR: `https://github.com/Alive24/jade-symphony/pull/337`";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Jade Symphony Workpad").count(), 1);
        assert!(!body.contains("## Workpad"));
        assert!(!body.contains("not-created"));
        assert!(body.contains("https://github.com/Alive24/jade-symphony/pull/337"));
        assert!(body.contains("## Agent Review"));
    }

    #[test]
    fn merge_workpad_body_replaces_workspace_adoption_block() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n### Context\n- keep\n\n<!-- jade-symphony-workspace-adoption -->\n### Workspace Adoption\n- Path: `/tmp/old`\n<!-- /jade-symphony-workspace-adoption -->"
        );
        let incoming =
            "<!-- jade-symphony-workspace-adoption -->\n### Workspace Adoption\n- Path: `/tmp/new`\n<!-- /jade-symphony-workspace-adoption -->";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- jade-symphony-workspace-adoption -->")
                .count(),
            1
        );
        assert!(body.contains("/tmp/new"));
        assert!(!body.contains("/tmp/old"));
        assert!(body.contains("- keep"));
    }

    #[test]
    fn merge_workpad_body_replaces_runtime_ownership_marker() {
        let marker = "<!-- jade-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Jade Symphony Workpad\n\n<!-- jade-symphony-runtime-ownership -->\n### Runtime Ownership\n- Branch: `old`\n<!-- /jade-symphony-runtime-ownership -->\n\n### Plan\n- [ ] inspect issue"
        );
        let incoming = "<!-- jade-symphony-runtime-ownership -->\n### Runtime Ownership\n- Branch: `new`\n<!-- /jade-symphony-runtime-ownership -->\n\n### Runtime Ownership Note\nupdated";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- jade-symphony-runtime-ownership -->")
                .count(),
            1
        );
        assert!(body.contains("- Branch: `new`"));
        assert!(!body.contains("- Branch: `old`"));
        assert!(body.contains("### Plan"));
        assert!(body.contains("### Runtime Ownership Note"));
    }
}
