use crate::model::RuntimeSnapshot;

pub fn render_snapshot(snapshot: &RuntimeSnapshot) -> String {
    let mut lines = Vec::new();
    lines.push("JADE SYMPHONY STATUS".to_string());
    lines.push(format!(
        "poll_interval_ms={} next_poll_in_ms={}",
        snapshot.polling.poll_interval_ms,
        snapshot
            .polling
            .next_poll_in_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".into())
    ));
    lines.push(format!(
        "running={} retrying={} skipped={} tokens={}",
        snapshot.running.len(),
        snapshot.retrying.len(),
        snapshot.skipped.len(),
        snapshot.codex_totals.total_tokens
    ));

    if !snapshot.running.is_empty() {
        lines.push("running issues:".into());
        for entry in &snapshot.running {
            lines.push(format!(
                "- {} {} state={} backend={}",
                entry.issue_id, entry.identifier, entry.state, entry.backend
            ));
        }
    }

    if !snapshot.skipped.is_empty() {
        lines.push("skipped issues:".into());
        for entry in &snapshot.skipped {
            lines.push(format!(
                "- {} {} reason={}",
                entry.issue_id, entry.identifier, entry.reason
            ));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_operator_readable_snapshot() {
        let rendered = render_snapshot(&RuntimeSnapshot::default());
        assert!(rendered.contains("JADE SYMPHONY STATUS"));
        assert!(rendered.contains("running=0"));
    }
}
