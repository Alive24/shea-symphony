use crate::model::{GateDecision, LatestStatus, RuntimeSnapshot};

pub fn render_snapshot(snapshot: &RuntimeSnapshot) -> String {
    let mut lines = Vec::new();
    lines.push("JADE SYMPHONY STATUS".to_string());
    lines.push(format!(
        "polling: checking={} interval_ms={} next_poll_in_ms={}",
        snapshot.polling.checking,
        snapshot.polling.poll_interval_ms,
        snapshot
            .polling
            .next_poll_in_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".into())
    ));
    lines.push(format!(
        "activity: running={} retrying={} skipped={}",
        snapshot.running.len(),
        snapshot.retrying.len(),
        snapshot.skipped.len(),
    ));
    lines.push(format!(
        "tokens: input={} output={} total={} seconds_running={}",
        snapshot.codex_totals.input_tokens,
        snapshot.codex_totals.output_tokens,
        snapshot.codex_totals.total_tokens,
        snapshot.codex_totals.seconds_running,
    ));

    if let Some(status) = &snapshot.latest_status {
        lines.push(render_latest_status_bar(status));
    }

    if let Some(path) = &snapshot.event_log_path {
        lines.push(format!("event_log={path}"));
    }

    render_running(snapshot, &mut lines);
    render_sessions(snapshot, &mut lines);
    render_retrying(snapshot, &mut lines);
    render_skipped(snapshot, &mut lines);
    render_integration_gaps(snapshot, &mut lines);

    lines.join("\n")
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

    lines.push("tmux sessions:".into());
    for entry in &snapshot.sessions {
        lines.push(format!(
            "- {} lane={} issue={} status={} source={} evidence=\"{}\" attach={} log={}",
            entry.session_id,
            entry.lane,
            entry.issue_identifier.as_deref().unwrap_or("n/a"),
            entry.status,
            entry.evidence_source,
            entry.evidence,
            entry.attach_command.as_deref().unwrap_or("n/a"),
            entry.log_path.as_deref().unwrap_or("n/a"),
        ));
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

    lines.push("skipped issues:".into());
    for entry in &snapshot.skipped {
        lines.push(format!(
            "- {} {} reason={}",
            entry.issue_id, entry.identifier, entry.reason
        ));
        if let Some(gate) = &entry.gate {
            render_gate(gate, lines);
        }
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

fn render_integration_gaps(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.integration_gaps.is_empty() {
        return;
    }

    lines.push("integration gaps:".into());
    for gap in &snapshot.integration_gaps {
        lines.push(format!("- {gap}"));
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
        assert!(rendered.contains("JADE SYMPHONY STATUS"));
        assert!(rendered.contains("activity: running=0"));
        assert!(rendered.contains("tokens: input=0"));
    }

    #[test]
    fn renders_runtime_categories_and_gaps() {
        let rendered = render_snapshot(&RuntimeSnapshot {
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
                session_id: "jade-main-1-attempt-1".into(),
                lane: "main".into(),
                status: "waiting_for_approval".into(),
                evidence_source: "pane".into(),
                evidence: "Approval required: allow this command?".into(),
                issue_identifier: Some("#1".into()),
                issue_title: Some("Wire runtime".into()),
                attach_command: Some("tmux attach-session -t jade-main-1-attempt-1".into()),
                log_path: Some("/tmp/jade-main-1.log".into()),
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
                actor_label: Some("Jade Symphony Agent".into()),
                workspace: Some("/tmp/ws".into()),
                branch: Some("feature/issue-1".into()),
                session_id: Some("session".into()),
                next: Some("Agent Review".into()),
            }),
            event_log_path: Some("/tmp/events.jsonl".into()),
        });

        assert!(rendered.contains("Latest: main | #1 | handoff | pr_created"));
        assert!(rendered.contains("running issues:"));
        assert!(rendered.contains("tmux sessions:"));
        assert!(rendered.contains("status=waiting_for_approval"));
        assert!(rendered.contains("profile=codex-alpha"));
        assert!(rendered.contains("retrying issues:"));
        assert!(rendered.contains("skipped issues:"));
        assert!(rendered.contains("gate=NeedToClarify"));
        assert!(rendered.contains("integration gaps:"));
        assert!(rendered.contains("event_log=/tmp/events.jsonl"));
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
}
