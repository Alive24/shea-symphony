use crate::model::{GateDecision, RuntimeSnapshot};

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

    if let Some(path) = &snapshot.event_log_path {
        lines.push(format!("event_log={path}"));
    }

    render_running(snapshot, &mut lines);
    render_retrying(snapshot, &mut lines);
    render_skipped(snapshot, &mut lines);
    render_integration_gaps(snapshot, &mut lines);

    lines.join("\n")
}

fn render_running(snapshot: &RuntimeSnapshot, lines: &mut Vec<String>) {
    if snapshot.running.is_empty() {
        return;
    }

    lines.push("running issues:".into());
    for entry in &snapshot.running {
        lines.push(format!(
            "- {} {} state={} backend={} workspace={} session={}",
            entry.issue_id,
            entry.identifier,
            entry.state,
            entry.backend,
            entry.workspace_path.as_deref().unwrap_or("n/a"),
            entry.session_id.as_deref().unwrap_or("n/a"),
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
        GateDecision, GateDecisionKind, PollingSnapshot, RetrySnapshot, RunningSnapshot,
        RuntimeSnapshot, SkippedIssue, TokenTotals,
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
            event_log_path: Some("/tmp/events.jsonl".into()),
        });

        assert!(rendered.contains("running issues:"));
        assert!(rendered.contains("retrying issues:"));
        assert!(rendered.contains("skipped issues:"));
        assert!(rendered.contains("gate=NeedToClarify"));
        assert!(rendered.contains("integration gaps:"));
        assert!(rendered.contains("event_log=/tmp/events.jsonl"));
    }
}
