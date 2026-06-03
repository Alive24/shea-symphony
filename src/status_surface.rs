use crate::model::{GateDecision, LatestStatus, RuntimeSnapshot};

pub fn render_snapshot(snapshot: &RuntimeSnapshot) -> String {
    let mut lines = Vec::new();
    lines.push("SHEA SYMPHONY STATUS".to_string());

    if let Some(status) = &snapshot.latest_status {
        if latest_status_is_operator_visible(status) {
            lines.push(render_latest_status_bar(status));
        }
    }

    render_planned(snapshot, &mut lines);
    render_running(snapshot, &mut lines);
    render_sessions(snapshot, &mut lines);
    render_retrying(snapshot, &mut lines);
    render_skipped(snapshot, &mut lines);

    lines.join("\n")
}

fn latest_status_is_operator_visible(status: &LatestStatus) -> bool {
    status.issue_identifier.is_some() && status.category != "idle"
}

pub fn render_latest_status_bar(status: &LatestStatus) -> String {
    let issue = status.issue_identifier.as_deref().unwrap_or("no-issue");
    let title = status.issue_title.as_deref().unwrap_or("untitled");
    let mut parts = vec![
        format!("Latest: {}", status.lane),
        issue.to_string(),
        status.category.clone(),
        status.action.clone(),
    ];
    if issue != "no-issue" {
        parts.push(title.to_string());
    }
    if let Some(actor) = &status.actor_label {
        parts.push(format!("actor={actor}"));
    }
    if let Some(workspace) = &status.workspace {
        parts.push(format!("workspace={workspace}"));
    }
    if let Some(branch) = &status.branch {
        parts.push(format!("branch={branch}"));
    }
    if let Some(session_id) = &status.session_id {
        parts.push(format!("session={session_id}"));
    }
    if let Some(next) = &status.next {
        parts.push(format!("next={next}"));
    }
    parts.join(" | ")
}

fn render_planned(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.planned.is_empty() {
        return;
    }

    lines.push("planned issues:".into());
    for entry in &snapshot.planned {
        lines.push(format!(
            "- {} {} state={} backend={} profile={} workspace={} session={}",
            entry.issue_id,
            entry.identifier,
            entry.state,
            entry.backend,
            entry.profile_id.as_deref().unwrap_or("n/a"),
            entry.workspace_path.as_deref().unwrap_or("n/a"),
            entry.session_id.as_deref().unwrap_or("n/a"),
        ));
    }
}

fn render_running(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.running.is_empty() {
        return;
    }

    lines.push("running issues:".into());
    for entry in &snapshot.running {
        lines.push(format!(
            "- {} {} state={} backend={} profile={} workspace={} session={}",
            entry.issue_id,
            entry.identifier,
            entry.state,
            entry.backend,
            entry.profile_id.as_deref().unwrap_or("n/a"),
            entry.workspace_path.as_deref().unwrap_or("n/a"),
            entry.session_id.as_deref().unwrap_or("n/a"),
        ));
    }
}

fn render_sessions(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.sessions.is_empty() {
        return;
    }

    lines.push("runtime sessions:".into());
    for entry in &snapshot.sessions {
        lines.push(format!(
            "- {} lane={} backend={} issue={} status={} source={} evidence=\"{}\" attach={} log={}",
            entry.session_id,
            entry.lane,
            session_backend(entry),
            entry.issue_identifier.as_deref().unwrap_or("n/a"),
            entry.status,
            entry.evidence_source,
            entry.evidence,
            entry.attach_command.as_deref().unwrap_or("n/a"),
            entry.log_path.as_deref().unwrap_or("n/a"),
        ));
    }
}

fn session_backend(entry: &crate::model::SessionStatusSnapshot) -> &str {
    let backend = entry.backend.trim();
    if backend.is_empty() {
        "unknown"
    } else {
        backend
    }
}

fn render_retrying(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.retrying.is_empty() {
        return;
    }

    lines.push("retrying issues:".into());
    for entry in &snapshot.retrying {
        lines.push(format!(
            "- {} {} attempt={} due_in_ms={} error={}",
            entry.issue_id,
            entry.identifier,
            entry.attempt,
            entry.due_in_ms,
            entry.error.as_deref().unwrap_or("n/a"),
        ));
    }
}

fn render_skipped(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.skipped.is_empty() {
        return;
    }

    let sample_limit = 5;
    lines.push(format!("skipped issues: {}", snapshot.skipped.len()));
    for entry in snapshot.skipped.iter().take(sample_limit) {
        lines.push(format!(
            "- {} {} reason={}",
            entry.issue_id, entry.identifier, entry.reason
        ));
        if let Some(gate) = &entry.gate {
            render_gate(gate, lines);
        }
    }
    let remaining = snapshot.skipped.len().saturating_sub(sample_limit);
    if remaining > 0 {
        lines.push(format!("- ... {remaining} more skipped issue(s) omitted"));
    }
}

fn render_gate(gate: &GateDecision, lines: &mut Vec<String>) {
    lines.push(format!("  gate={:?}", gate.kind));
    if !gate.missing.is_empty() {
        lines.push(format!("  missing={}", gate.missing.join(", ")));
    }
    if !gate.assumptions.is_empty() {
        lines.push(format!("  assumptions={}", gate.assumptions.join("; ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        GateDecision, GateDecisionKind, LatestStatus, PollingSnapshot, RetrySnapshot,
        RunningSnapshot, RuntimeSnapshot, SessionStatusSnapshot, SkippedIssue, TokenTotals,
    };

    #[test]
    fn renders_operator_readable_snapshot() {
        let rendered = render_snapshot(&RuntimeSnapshot::default());
        assert!(rendered.contains("SHEA SYMPHONY STATUS"));
        assert!(!rendered.contains("activity:"));
        assert!(!rendered.contains("tokens:"));
        assert!(!rendered.contains("polling:"));
    }

    #[test]
    fn renders_runtime_categories_and_gaps() {
        let rendered = render_snapshot(&RuntimeSnapshot {
            planned: vec![RunningSnapshot {
                issue_id: "GHI_0".into(),
                identifier: "#0".into(),
                state: "Todo".into(),
                backend: "codex".into(),
                workspace_path: None,
                session_id: None,
                profile_id: None,
                instance_name: None,
            }],
            running: vec![RunningSnapshot {
                issue_id: "GHI_1".into(),
                identifier: "#1".into(),
                state: "Todo".into(),
                backend: "codex".into(),
                workspace_path: Some("/tmp/ws".into()),
                session_id: Some("session".into()),
                profile_id: Some("codex-alpha".into()),
                instance_name: Some("Codex Alpha".into()),
            }],
            retrying: vec![RetrySnapshot {
                issue_id: "GHI_2".into(),
                identifier: "#2".into(),
                attempt: 2,
                due_in_ms: 5000,
                error: Some("rate limited".into()),
            }],
            codex_totals: TokenTotals {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                seconds_running: 4,
            },
            polling: PollingSnapshot {
                checking: true,
                next_poll_in_ms: Some(1000),
                poll_interval_ms: 5000,
            },
            sessions: vec![SessionStatusSnapshot {
                session_id: "shea-main-1-attempt-1".into(),
                lane: "main".into(),
                backend: "tmux".into(),
                run_id: None,
                status: "waiting_for_approval".into(),
                evidence_source: "pane".into(),
                evidence: "Approval required: allow this command?".into(),
                issue_identifier: Some("#1".into()),
                issue_title: Some("Wire runtime".into()),
                attach_command: Some("tmux attach-session -t shea-main-1-attempt-1".into()),
                log_path: Some("/tmp/shea-main-1.log".into()),
                updated_at_ms: 10,
            }],
            skipped: vec![SkippedIssue {
                issue_id: "GHI_3".into(),
                identifier: "#3".into(),
                reason: "issue quality gate did not pass".into(),
                gate: Some(GateDecision {
                    kind: GateDecisionKind::NeedToClarify,
                    missing: vec!["scope".into()],
                    assumptions: Vec::new(),
                    notes: Vec::new(),
                }),
            }],
            integration_gaps: vec!["missing token".into()],
            latest_status: Some(LatestStatus {
                lane: "main".into(),
                category: "handoff".into(),
                action: "pr_created".into(),
                issue_identifier: Some("#1".into()),
                issue_title: Some("Wire runtime".into()),
                actor_label: Some("Shea Symphony Agent".into()),
                workspace: Some("/tmp/ws".into()),
                branch: Some("feature/issue-1".into()),
                session_id: Some("session".into()),
                next: Some("Agent Review".into()),
            }),
            event_log_path: Some("/tmp/events.jsonl".into()),
        });

        assert!(rendered.contains("Latest: main | #1 | handoff | pr_created"));
        assert!(rendered.contains("planned issues:"));
        assert!(rendered.contains("running issues:"));
        assert!(rendered.contains("runtime sessions:"));
        assert!(rendered.contains("backend=tmux"));
        assert!(rendered.contains("status=waiting_for_approval"));
        assert!(rendered.contains("profile=codex-alpha"));
        assert!(rendered.contains("retrying issues:"));
        assert!(rendered.contains("skipped issues:"));
        assert!(rendered.contains("gate=NeedToClarify"));
        assert!(!rendered.contains("integration gaps:"));
        assert!(!rendered.contains("event_log=/tmp/events.jsonl"));
    }

    #[test]
    fn renders_latest_status_without_optional_fields() {
        let rendered = render_latest_status_bar(&LatestStatus {
            lane: "merge".into(),
            category: "idle".into(),
            action: "no_merging_issue".into(),
            issue_identifier: None,
            issue_title: None,
            actor_label: None,
            workspace: None,
            branch: None,
            session_id: None,
            next: Some("wait".into()),
        });

        assert_eq!(
            rendered,
            "Latest: merge | no-issue | idle | no_merging_issue | next=wait"
        );
    }

    #[test]
    fn snapshot_omits_idle_no_issue_latest_status() {
        let rendered = render_snapshot(&RuntimeSnapshot {
            latest_status: Some(LatestStatus {
                lane: "merge".into(),
                category: "idle".into(),
                action: "no_merging_issue".into(),
                issue_identifier: None,
                issue_title: None,
                actor_label: None,
                workspace: None,
                branch: None,
                session_id: None,
                next: Some("wait".into()),
            }),
            event_log_path: Some("/tmp/events.jsonl".into()),
            ..RuntimeSnapshot::default()
        });

        assert!(!rendered.contains("Latest:"));
        assert!(!rendered.contains("event_log="));
    }
}
