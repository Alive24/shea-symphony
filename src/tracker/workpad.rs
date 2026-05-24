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
