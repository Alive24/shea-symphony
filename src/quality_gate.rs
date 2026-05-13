use std::path::{Path, PathBuf};

use crate::model::{GateDecision, GateDecisionKind, TrackerIssue};

const REQUIRED_SECTIONS: &[(&str, &str)] = &[
    ("Issue Goal", "goal"),
    ("Why Now", "why now"),
    ("Issue Context", "context"),
    ("Scope", "scope"),
    ("Canonical References", "canonical references"),
    ("Target Repository / Package", "target repo or package"),
    ("Non-Negotiable Guardrails", "guardrails and constraints"),
    ("Verification", "validation requirements"),
    ("Completion Criteria", "acceptance criteria"),
];

pub fn evaluate_issue(issue: &TrackerIssue) -> GateDecision {
    let Some(description) = issue.description.as_deref() else {
        return GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec!["description with executable issue contract".into()],
            assumptions: Vec::new(),
            notes: vec!["Issue body is empty.".into()],
        };
    };

    let mut missing = Vec::new();
    for (heading, label) in REQUIRED_SECTIONS {
        if !contains_heading(description, heading) {
            missing.push((*label).to_string());
        }
    }

    if has_explicit_blocked_decision(description) && missing.is_empty() {
        return GateDecision {
            kind: GateDecisionKind::Blocked,
            missing: vec!["blocked dependency must be resolved before dispatch".into()],
            assumptions: Vec::new(),
            notes: Vec::new(),
        };
    }

    if !missing.is_empty() {
        return GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: Vec::new(),
            notes: vec!["Issue contract does not yet match the Jade quality template.".into()],
        };
    }

    let assumptions = extract_assumptions(description);
    if assumptions.is_empty() {
        GateDecision::ready()
    } else {
        GateDecision {
            kind: GateDecisionKind::ReadyWithAssumptions,
            missing: Vec::new(),
            assumptions,
            notes: Vec::new(),
        }
    }
}

pub fn evaluate_issue_with_source_alignment(
    issue: &TrackerIssue,
    repo_root: &Path,
    expected_target_repo: Option<&str>,
) -> GateDecision {
    let mut decision = evaluate_issue(issue);
    if !decision.is_dispatchable() {
        return decision;
    }

    let description = issue.description.as_deref().unwrap_or_default();
    let mut missing = Vec::new();
    let mut notes = Vec::new();

    if let Some(expected) = expected_target_repo {
        match first_bullet_in_section(description, "Target Repository / Package") {
            Some(actual) if normalize_target_repo(&actual) == normalize_target_repo(expected) => {}
            Some(actual) => missing.push(format!(
                "target repository mismatch: expected `{expected}`, found `{actual}`"
            )),
            None => missing.push("target repository bullet".into()),
        }
    }

    for path in referenced_paths(description) {
        if !repo_root.join(&path).exists() {
            missing.push(format!("referenced path missing: `{}`", path.display()));
        }
    }

    let commands = verification_commands(description);
    if commands.is_empty() {
        missing.push("verification command".into());
    } else {
        for command in commands {
            if !is_supported_verification_command(&command) {
                missing.push(format!("unsupported verification command: `{command}`"));
            }
        }
    }

    if missing.is_empty() {
        notes.push("Source alignment preflight passed.".into());
        decision.notes.extend(notes);
        decision
    } else {
        GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing,
            assumptions: decision.assumptions,
            notes: vec![
                "Source alignment preflight found missing or unsupported repository context."
                    .into(),
            ],
        }
    }
}

fn has_explicit_blocked_decision(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let normalized = line.trim().trim_start_matches('-').trim().to_lowercase();

        matches!(
            normalized.as_str(),
            "gate decision: blocked" | "classification: blocked" | "status: blocked"
        )
    })
}

fn contains_heading(markdown: &str, heading: &str) -> bool {
    markdown.lines().any(|line| {
        let line = line.trim_start_matches('#').trim();
        line.eq_ignore_ascii_case(heading)
    })
}

fn extract_assumptions(markdown: &str) -> Vec<String> {
    let mut in_assumptions = false;
    let mut assumptions = Vec::new();

    for line in markdown.lines() {
        let trimmed = line.trim();
        let heading = trimmed.trim_start_matches('#').trim();

        if heading.eq_ignore_ascii_case("Assumptions") {
            in_assumptions = true;
            continue;
        }

        if in_assumptions && trimmed.starts_with('#') {
            break;
        }

        if in_assumptions && trimmed.starts_with('-') {
            assumptions.push(trimmed.trim_start_matches('-').trim().to_string());
        }
    }

    assumptions
}

fn first_bullet_in_section(markdown: &str, heading: &str) -> Option<String> {
    section_lines(markdown, heading)
        .into_iter()
        .find_map(|line| {
            line.trim()
                .strip_prefix('-')
                .map(clean_markdown_value)
                .filter(|value| !value.is_empty())
        })
}

fn referenced_paths(markdown: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for heading in ["Relevant Knowledge Sources", "Relevant Code Paths"] {
        for line in section_lines(markdown, heading) {
            let Some(raw) = line.trim().strip_prefix('-') else {
                continue;
            };
            let value = clean_markdown_value(raw);
            if is_local_reference(&value) {
                paths.push(PathBuf::from(value));
            }
        }
    }
    paths
}

fn verification_commands(markdown: &str) -> Vec<String> {
    section_lines(markdown, "Functional Verification")
        .into_iter()
        .filter_map(|line| {
            line.trim()
                .strip_prefix('-')
                .map(clean_markdown_value)
                .filter(|value| looks_like_command(value))
        })
        .collect()
}

fn section_lines(markdown: &str, heading: &str) -> Vec<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        let normalized_heading = trimmed.trim_start_matches('#').trim();
        if normalized_heading.eq_ignore_ascii_case(heading) {
            in_section = true;
            continue;
        }
        if in_section && trimmed.starts_with('#') {
            break;
        }
        if in_section {
            lines.push(line.to_string());
        }
    }
    lines
}

fn clean_markdown_value(raw: &str) -> String {
    raw.trim()
        .trim_matches('`')
        .trim()
        .trim_end_matches('.')
        .to_string()
}

fn normalize_target_repo(value: &str) -> String {
    clean_markdown_value(value)
        .trim_start_matches("https://github.com/")
        .trim_start_matches("github.com/")
        .trim_matches('`')
        .to_ascii_lowercase()
}

fn is_local_reference(value: &str) -> bool {
    !value.starts_with("http://")
        && !value.starts_with("https://")
        && !value.starts_with('$')
        && (value.contains('/')
            || value.starts_with("Cargo.")
            || value == "README.md"
            || value.ends_with(".rs")
            || value.ends_with(".md"))
}

fn looks_like_command(value: &str) -> bool {
    matches!(
        value.split_whitespace().next(),
        Some("cargo" | "gh" | "git" | "pnpm" | "npm" | "node")
    )
}

fn is_supported_verification_command(command: &str) -> bool {
    let parts: Vec<_> = command.split_whitespace().collect();
    matches!(
        parts.as_slice(),
        ["cargo", "test"]
            | ["cargo", "fmt", "--check"]
            | ["cargo", "clippy", ..]
            | ["cargo", "run", ..]
            | ["gh", ..]
            | ["git", ..]
            | ["pnpm", ..]
            | ["npm", ..]
            | ["node", ..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrackerIssue;

    fn issue(description: Option<String>) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "1".into(),
            item_id: None,
            identifier: "#1".into(),
            title: "Implement".into(),
            description,
            url: None,
            state: "Todo".into(),
            labels: vec![],
            assignees: vec![],
            priority: None,
            branch_name: None,
            linked_pull_requests: vec![],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn missing_contract_needs_clarification() {
        let decision = evaluate_issue(&issue(Some("thin body".into())));
        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision.missing.iter().any(|item| item == "goal"));
    }

    #[test]
    fn template_shaped_issue_is_ready() {
        let body = [
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "It blocks the next slice.",
            "## Issue Context",
            "Context.",
            "## Non-Negotiable Guardrails",
            "- Keep tracker abstraction.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n");

        assert!(evaluate_issue(&issue(Some(body))).is_dispatchable());
    }

    #[test]
    fn incidental_blocked_word_does_not_block_ready_issue() {
        let body = [
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "It is needed before blocked downstream work can proceed.",
            "## Issue Context",
            "Context.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
        ]
        .join("\n");

        assert!(evaluate_issue(&issue(Some(body))).is_dispatchable());
    }

    #[test]
    fn explicit_blocked_decision_blocks_issue() {
        let body = [
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Tests pass.",
            "- Gate Decision: Blocked",
        ]
        .join("\n");

        let decision = evaluate_issue(&issue(Some(body)));
        assert_eq!(decision.kind, GateDecisionKind::Blocked);
    }

    #[test]
    fn source_alignment_accepts_existing_paths_and_supported_commands() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src")).unwrap();
        std::fs::write(temp.path().join("src/main.rs"), "").unwrap();
        std::fs::create_dir_all(temp.path().join("docs")).unwrap();
        std::fs::write(temp.path().join("docs/dogfood-readiness.md"), "").unwrap();
        let body = aligned_body(
            "Alive24/jade-symphony",
            &["docs/dogfood-readiness.md"],
            &["src/main.rs"],
            &["cargo test", "cargo fmt --check"],
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert!(decision.is_dispatchable());
        assert!(decision
            .notes
            .contains(&"Source alignment preflight passed.".to_string()));
    }

    #[test]
    fn source_alignment_reports_missing_paths() {
        let temp = tempfile::tempdir().unwrap();
        let body = aligned_body(
            "Alive24/jade-symphony",
            &["docs/missing.md"],
            &["src/main.rs"],
            &["cargo test"],
        );

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .iter()
            .any(|item| item.contains("referenced path missing")));
    }

    #[test]
    fn source_alignment_reports_target_repo_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let body = aligned_body("Other/repo", &[], &[], &["cargo test"]);

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .iter()
            .any(|item| item.contains("target repository mismatch")));
    }

    #[test]
    fn source_alignment_reports_weak_verification() {
        let temp = tempfile::tempdir().unwrap();
        let body = aligned_body("Alive24/jade-symphony", &[], &[], &["manually inspect"]);

        let decision = evaluate_issue_with_source_alignment(
            &issue(Some(body)),
            temp.path(),
            Some("Alive24/jade-symphony"),
        );

        assert_eq!(decision.kind, GateDecisionKind::NeedToClarify);
        assert!(decision
            .missing
            .iter()
            .any(|item| item == "verification command"));
    }

    fn aligned_body(target_repo: &str, docs: &[&str], paths: &[&str], commands: &[&str]) -> String {
        let docs = docs
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect::<Vec<_>>()
            .join("\n");
        let paths = paths
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect::<Vec<_>>()
            .join("\n");
        let commands = commands
            .iter()
            .map(|command| format!("- `{command}`"))
            .collect::<Vec<_>>()
            .join("\n");
        [
            "## Issue Goal",
            "Ship a thing.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Decisions / Assumptions",
            "### Assumptions",
            "- Deterministic source checks are enough.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "### In Scope",
            "- Code.",
            "## Canonical References",
            "### Target Repository / Package",
            &format!("- `{target_repo}`"),
            "### Relevant Knowledge Sources",
            &docs,
            "### Relevant Code Paths",
            &paths,
            "## Verification",
            "### Completion Criteria",
            "- Pass.",
            "### Functional Verification",
            &commands,
        ]
        .join("\n")
    }
}
