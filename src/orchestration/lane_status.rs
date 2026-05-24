use jade_symphony::config::RuntimeConfig;
use jade_symphony::model::{LatestStatus, TrackerIssue};
use jade_symphony::status_surface::render_latest_status_bar;

pub(crate) fn latest_status_for_issue(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    lane: &str,
    category: &str,
    action: &str,
    next: Option<String>,
) -> LatestStatus {
    LatestStatus {
        lane: lane.into(),
        category: category.into(),
        action: action.into(),
        issue_identifier: Some(issue.identifier.clone()),
        issue_title: Some(issue.title.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        workspace: None,
        branch: issue.branch_name.clone(),
        session_id: None,
        next,
    }
}

pub(crate) fn print_latest_status(status: &LatestStatus) {
    println!("{}", render_latest_status_bar(status));
}

pub(crate) fn unbounded_loop_sleep_ms(limit: Option<usize>, poll_interval_ms: u64) -> Option<u64> {
    limit.is_none().then_some(poll_interval_ms)
}
