use jade_symphony::config::RuntimeConfig;
use jade_symphony::event_log::{EventLog, EventRecord};
use jade_symphony::runtime_state::RuntimeState;

pub(crate) fn append_runtime_supervision_event(
    config: &RuntimeConfig,
    state: Option<&RuntimeState>,
    event: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    let active_issue = state.and_then(|state| state.active_issue.as_ref());
    log.append(&EventRecord {
        event: event.into(),
        issue_id: active_issue.map(|issue| issue.id.clone()),
        issue_identifier: active_issue.map(|issue| issue.identifier.clone()),
        session_id: state.and_then(|state| state.backend_session_id.clone()),
        profile_id: state.and_then(|state| state.profile_id.clone()),
        instance_name: state.and_then(|state| state.instance_name.clone()),
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        tracker_mutation: None,
        message: message.into(),
    })?;
    Ok(())
}
