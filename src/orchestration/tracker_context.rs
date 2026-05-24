use shea_symphony::config::RuntimeConfig;
use shea_symphony::model::TrackerIssue;
use shea_symphony::tracker::{TrackerAdapter, TrackerError};

pub(crate) fn tracker_backend_label(config: &RuntimeConfig) -> &'static str {
    match config.tracker.kind.as_str() {
        "github_project_v2" => "gh",
        "linear" => "linear",
        "memory" => "memory",
        _ => "tracker",
    }
}

pub(crate) fn live_github_tracker(config: &RuntimeConfig) -> bool {
    config.tracker.kind == "github_project_v2" && config.tracker.fixture_path.is_none()
}

pub(crate) fn hydrate_issue_for_evidence(
    adapter: &dyn TrackerAdapter,
    issue: TrackerIssue,
    project_context: &[TrackerIssue],
) -> Result<TrackerIssue, TrackerError> {
    adapter.hydrate_issue_evidence(issue, project_context)
}

pub(crate) fn hydrate_issues_for_review_lane(
    adapter: &dyn TrackerAdapter,
    issues: Vec<TrackerIssue>,
) -> Result<Vec<TrackerIssue>, TrackerError> {
    let project_context = issues.clone();
    issues
        .into_iter()
        .map(|issue| hydrate_issue_for_evidence(adapter, issue, &project_context))
        .collect()
}

pub(crate) fn all_mapped_tracker_states(config: &RuntimeConfig) -> Vec<String> {
    let state_map = &config.tracker.state_map;
    vec![
        state_map.backlog.clone(),
        state_map.todo.clone(),
        state_map.need_to_clarify.clone(),
        state_map.in_progress.clone(),
        state_map.need_human_input.clone(),
        state_map.agent_review.clone(),
        state_map.human_review.clone(),
        state_map.rework.clone(),
        state_map.merging.clone(),
        state_map.done.clone(),
    ]
}
