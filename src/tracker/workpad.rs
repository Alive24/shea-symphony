pub(in crate::tracker) fn ensure_workpad_marker(markdown: &str, marker: &str) -> String {
    if markdown.contains(marker) {
        markdown.to_string()
    } else {
        format!("{marker}\n{markdown}")
    }
}

pub(in crate::tracker) fn merge_workpad_body(
    existing: &str,
    incoming: &str,
    marker: &str,
) -> String {
    let existing = ensure_workpad_marker(existing, marker);
    let incoming = ensure_workpad_marker(incoming, marker);
    let (mut merged, incoming_remainder) = replace_singleton_workpad_blocks(&existing, &incoming);
    let incoming_content = strip_workpad_marker(&incoming_remainder, marker);

    for entry in split_workpad_entries(incoming_content) {
        merged = merge_workpad_entry(&merged, &entry, marker);
    }

    merged
}

pub(in crate::tracker) fn duplicate_workpad_body(_marker: &str) -> String {
    "Superseded Shea Symphony workpad comment. The canonical marker was removed from this duplicate."
        .to_string()
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
    let mut entries = Vec::new();
    let incoming_is_canonical_workpad = is_canonical_workpad_entry(incoming_entry);

    if incoming_is_canonical_workpad {
        let mut canonical = None;
        for entry in split_workpad_entries(content) {
            if is_canonical_workpad_entry(&entry) {
                canonical = Some(merge_canonical_workpad_sections(
                    canonical.as_deref(),
                    &entry,
                ));
            } else {
                entries.push(entry);
            }
        }
        canonical = Some(merge_canonical_workpad_sections(
            canonical.as_deref(),
            incoming_entry,
        ));
        entries.push(canonical.expect("incoming canonical workpad always renders"));
        return render_workpad_entries(marker, &entries);
    }

    let mut replaced = false;

    for entry in split_workpad_entries(content) {
        let should_replace = workpad_entry_key(&entry).as_deref() == Some(incoming_key);

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

fn merge_canonical_workpad_sections(existing: Option<&str>, incoming: &str) -> String {
    let mut sections = existing.map(canonical_workpad_sections).unwrap_or_default();
    for incoming_section in canonical_workpad_sections(incoming) {
        if let Some(index) = sections.iter().position(|section| {
            canonical_section_key(section) == canonical_section_key(&incoming_section)
        }) {
            sections[index] = incoming_section;
        } else {
            sections.push(incoming_section);
        }
    }

    let mut rendered = "## Shea Symphony Workpad".to_string();
    if !sections.is_empty() {
        rendered.push_str("\n\n");
        rendered.push_str(&sections.join("\n\n"));
    }
    rendered
}

fn canonical_workpad_sections(entry: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = Vec::new();
    for line in entry
        .lines()
        .skip_while(|line| !line.trim().starts_with("## "))
        .skip(1)
    {
        if line.trim().starts_with("### ") && !current.is_empty() {
            let section = current.join("\n").trim().to_string();
            if !section.is_empty() {
                sections.push(section);
            }
            current.clear();
        }
        current.push(line);
    }
    let section = current.join("\n").trim().to_string();
    if !section.is_empty() {
        sections.push(section);
    }
    sections
}

fn canonical_section_key(section: &str) -> &str {
    section
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or("")
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
    workpad_h2(entry).is_some_and(|h2| matches!(h2, "## Shea Symphony Workpad" | "## Workpad"))
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
        "<!-- shea-symphony-runtime-ownership -->",
        "<!-- /shea-symphony-runtime-ownership -->",
        true,
    );
    const WORKSPACE_ADOPTION: (&str, &str, bool) = (
        "<!-- shea-symphony-workspace-adoption -->",
        "<!-- /shea-symphony-workspace-adoption -->",
        false,
    );

    let mut merged = existing.to_string();
    let mut remainder = incoming.to_string();
    let incoming_updates_canonical_workpad = split_workpad_entries(incoming)
        .iter()
        .any(|entry| is_canonical_workpad_entry(entry));
    for (start, end, strip_when_missing) in [RUNTIME_OWNERSHIP, WORKSPACE_ADOPTION] {
        if start == RUNTIME_OWNERSHIP.0 && incoming_updates_canonical_workpad {
            // The section-aware canonical merge below replaces Run Identity as a
            // unit, including its ownership marker. Pre-stripping that block
            // would discard the new marker during the section replacement.
            continue;
        }
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
        let marker = "<!-- shea-symphony-workpad -->";
        let body = ensure_workpad_marker("## Workpad", marker);
        assert!(body.starts_with("<!-- shea-symphony-workpad -->"));
        let body = ensure_workpad_marker(&body, marker);
        let body = ensure_workpad_marker(&body, marker);
        assert_eq!(body.matches(marker).count(), 1);
    }

    #[test]
    fn merge_workpad_body_appends_without_losing_existing_sections() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing =
            format!("{marker}\n## Shea Symphony Workpad\n\n### Plan\n- [ ] inspect issue");
        let incoming = "## Agent Review\n\n### Manual Review Evidence\n````md\npass\n````";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert!(body.contains("### Plan"));
        assert!(body.contains("## Agent Review"));
        assert!(body.contains("pass"));
    }

    #[test]
    fn merge_workpad_body_appends_agent_review_handoff_without_replacing_main_workpad() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n### Plan\n- [x] inspect issue\n\n### Work Log\n- implemented main work"
        );
        let incoming =
            "## Agent Review Handoff\n\n### Agent Review Handoff Invariant\n- Status: `Ready`";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert_eq!(body.matches("## Shea Symphony Workpad").count(), 1);
        assert!(body.contains("### Plan"));
        assert!(body.contains("### Work Log"));
        assert!(body.contains("- implemented main work"));
        assert!(body.contains("## Agent Review Handoff"));
        assert!(body.contains("### Agent Review Handoff Invariant"));
    }

    #[test]
    fn merge_workpad_body_appends_distinct_review_attempts() {
        let marker = "<!-- shea-symphony-workpad -->";
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
    fn merge_workpad_body_replaces_matching_shea_symphony_workpad_entry() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n### Context\n- old context\n\n### Plan\n- [ ] old plan\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming =
            "## Shea Symphony Workpad\n\n### Context\n- updated context\n\n### Plan\n- [x] updated plan";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert_eq!(body.matches("## Shea Symphony Workpad").count(), 1);
        assert!(body.contains("- updated context"));
        assert!(body.contains("- [x] updated plan"));
        assert!(!body.contains("- old context"));
        assert!(body.contains("## Agent Review"));
        assert!(body.contains("Manual Review Evidence"));
    }

    #[test]
    fn automatic_and_manual_main_updates_preserve_one_canonical_shape() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n### Plan\n- [x] inspect\n\n### Work Log\n- implemented\n\n### Verification\n- cargo test: pass\n\n### PR / Linkage\n- pending\n\n### Recovery / Rework\n- prior recovery evidence\n\n### Handoff\n- pending"
        );
        let session_update = "## Shea Symphony Workpad\n\n### Run Identity\n- Run: `run-2`\n- Workspace: `/tmp/issue`";
        let handoff_update = "## Shea Symphony Workpad\n\n### PR / Linkage\n- PR: `#99`\n- Source: `github_native`\n\n### Handoff\n- Ready for Agent Review";

        let after_session = merge_workpad_body(&existing, session_update, marker);
        let body = merge_workpad_body(&after_session, handoff_update, marker);

        assert_eq!(body.matches(marker).count(), 1);
        assert_eq!(body.matches("## Shea Symphony Workpad").count(), 1);
        assert_eq!(body.matches("### Plan").count(), 1);
        assert!(body.contains("- [x] inspect"));
        assert!(body.contains("- implemented"));
        assert!(body.contains("- cargo test: pass"));
        assert!(body.contains("- prior recovery evidence"));
        assert!(body.contains("- Run: `run-2`"));
        assert!(body.contains("- PR: `#99`"));
        assert!(body.contains("- Ready for Agent Review"));
        assert!(!body.contains("- pending"));
    }

    #[test]
    fn merge_workpad_body_collapses_duplicate_matching_entries() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n### Context\n- first\n\n---\n\n## Shea Symphony Workpad\n\n### Context\n- duplicate\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming = "## Shea Symphony Workpad\n\n### Context\n- final";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Shea Symphony Workpad").count(), 1);
        assert!(body.contains("- final"));
        assert!(!body.contains("- first"));
        assert!(!body.contains("- duplicate"));
        assert!(body.contains("## Agent Review"));
    }

    #[test]
    fn merge_workpad_body_replaces_legacy_workpad_and_stale_pr_evidence() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Workpad\n\n### Workspace Evidence\n- Workspace path: `/tmp/old`\n\n---\n\n## Shea Symphony Workpad\n\n### Planned Handoff\n- Live PR: `not-created`\n\n---\n\n## Agent Review\n\n### Manual Review Evidence\npass"
        );
        let incoming =
            "## Shea Symphony Workpad\n\n### Planned Handoff\n- Live PR: `https://github.com/Alive24/shea-symphony/pull/337`";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(body.matches("## Shea Symphony Workpad").count(), 1);
        assert!(!body.contains("## Workpad"));
        assert!(!body.contains("not-created"));
        assert!(body.contains("https://github.com/Alive24/shea-symphony/pull/337"));
        assert!(body.contains("## Agent Review"));
    }

    #[test]
    fn merge_workpad_body_replaces_workspace_adoption_block() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n### Context\n- keep\n\n<!-- shea-symphony-workspace-adoption -->\n### Workspace Adoption\n- Path: `/tmp/old`\n<!-- /shea-symphony-workspace-adoption -->"
        );
        let incoming =
            "<!-- shea-symphony-workspace-adoption -->\n### Workspace Adoption\n- Path: `/tmp/new`\n<!-- /shea-symphony-workspace-adoption -->";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- shea-symphony-workspace-adoption -->")
                .count(),
            1
        );
        assert!(body.contains("/tmp/new"));
        assert!(!body.contains("/tmp/old"));
        assert!(body.contains("- keep"));
    }

    #[test]
    fn merge_workpad_body_replaces_runtime_ownership_marker() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n<!-- shea-symphony-runtime-ownership -->\n### Runtime Ownership\n- Branch: `old`\n<!-- /shea-symphony-runtime-ownership -->\n\n### Plan\n- [ ] inspect issue"
        );
        let incoming = "<!-- shea-symphony-runtime-ownership -->\n### Runtime Ownership\n- Branch: `new`\n<!-- /shea-symphony-runtime-ownership -->\n\n### Runtime Ownership Note\nupdated";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- shea-symphony-runtime-ownership -->")
                .count(),
            1
        );
        assert!(body.contains("- Branch: `new`"));
        assert!(!body.contains("- Branch: `old`"));
        assert!(body.contains("### Plan"));
        assert!(body.contains("### Runtime Ownership Note"));
    }

    #[test]
    fn canonical_run_identity_update_keeps_one_runtime_ownership_marker() {
        let marker = "<!-- shea-symphony-workpad -->";
        let existing = format!(
            "{marker}\n## Shea Symphony Workpad\n\n### Run Identity\n- Run: `old`\n\n<!-- shea-symphony-runtime-ownership -->\n- Branch: `old`\n<!-- /shea-symphony-runtime-ownership -->\n\n### Plan\n- [x] keep"
        );
        let incoming = "## Shea Symphony Workpad\n\n### Run Identity\n- Run: `new`\n\n<!-- shea-symphony-runtime-ownership -->\n- Branch: `new`\n<!-- /shea-symphony-runtime-ownership -->";

        let body = merge_workpad_body(&existing, incoming, marker);

        assert_eq!(
            body.matches("<!-- shea-symphony-runtime-ownership -->")
                .count(),
            1
        );
        assert!(body.contains("- Run: `new`"));
        assert!(body.contains("- Branch: `new`"));
        assert!(!body.contains("- Branch: `old`"));
        assert!(body.contains("- [x] keep"));
    }

    #[test]
    fn repeated_main_handoffs_replace_bounded_documentation_evidence_in_one_workpad() {
        let marker = "<!-- shea-symphony-workpad -->";
        let first = "## Shea Symphony Workpad\n\n### Documentation Impact\n- Actual documentation changes: `docs/first.md`\n- Unresolved reconciliation: pending\n\n### Plan\n- [x] keep";
        let second = "## Shea Symphony Workpad\n\n### Documentation Impact\n- Actual documentation changes: `docs/final.md`\n- Unresolved reconciliation: Human Review comparison required";

        let after_first = merge_workpad_body("", first, marker);
        let body = merge_workpad_body(&after_first, second, marker);

        assert_eq!(body.matches("## Shea Symphony Workpad").count(), 1);
        assert_eq!(body.matches("### Documentation Impact").count(), 1);
        assert!(!body.contains("docs/first.md"));
        assert!(body.contains("docs/final.md"));
        assert!(body.contains("- [x] keep"));
    }
}
