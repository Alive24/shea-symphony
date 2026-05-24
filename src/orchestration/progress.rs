use jade_symphony::config::RuntimeConfig;
use jade_symphony::progress::ProgressHeartbeatSpec;

pub(crate) fn progress_spec_for_config(
    config: &RuntimeConfig,
    wait: &str,
) -> ProgressHeartbeatSpec {
    ProgressHeartbeatSpec::new(wait).actor(
        config.identity.actor_role.clone(),
        config.identity.actor_label.clone(),
    )
}

pub(crate) fn progress_spec_with_event_log(
    config: &RuntimeConfig,
    wait: &str,
) -> ProgressHeartbeatSpec {
    progress_spec_for_config(config, wait)
        .event_log_path(config.observability.logs_root.join("jade-symphony.jsonl"))
}
