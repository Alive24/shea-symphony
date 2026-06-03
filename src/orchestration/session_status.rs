use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::SessionStatusSnapshot;
use shea_symphony::session_registry::{
    capture_tmux_pane_tail, classify_session_record, load_session_registry, read_log_tail,
    session_registry_path, unix_timestamp_ms,
};

pub(crate) const DEFAULT_SESSION_STATUS_LINES: usize = 80;
pub(crate) const DEFAULT_SESSION_STALE_AFTER_MS: u64 = 30 * 60 * 1000;

pub(crate) fn session_status_snapshots(
    config: &RuntimeConfig,
) -> Result<Vec<SessionStatusSnapshot>, Box<dyn std::error::Error>> {
    let registry = load_session_registry(&session_registry_path(config))?;
    let now_ms = unix_timestamp_ms();
    let mut snapshots = Vec::new();

    for record in registry.sessions.iter().rev().take(20).rev() {
        let is_tmux_session = record.backend == "tmux";
        let pane_tail = if is_tmux_session {
            capture_tmux_pane_tail(
                &config.tmux.command,
                &record.pane_target,
                DEFAULT_SESSION_STATUS_LINES,
            )
            .ok()
        } else {
            None
        };
        let log_tail = if is_tmux_session {
            read_log_tail(&record.log_path, DEFAULT_SESSION_STATUS_LINES)?
        } else {
            None
        };
        let probe = classify_session_record(
            record,
            pane_tail.as_deref(),
            log_tail.as_deref(),
            now_ms,
            config.codex.session_stale_after_ms,
        );
        snapshots.push(SessionStatusSnapshot {
            session_id: record.session_name.clone(),
            lane: record.lane.clone(),
            backend: record.backend.clone(),
            run_id: record.run_id.clone(),
            status: probe.status.as_str().into(),
            evidence_source: probe.source.as_str().into(),
            evidence: probe.evidence,
            issue_identifier: record.issue_identifier.clone(),
            issue_title: record.issue_title.clone(),
            attach_command: is_tmux_session.then(|| record.attach_command.clone()),
            log_path: is_tmux_session.then(|| record.log_path.display().to_string()),
            updated_at_ms: record.updated_at_ms,
        });
    }

    Ok(snapshots)
}
