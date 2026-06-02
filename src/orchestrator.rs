use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::config::RuntimeConfig;
use crate::model::{
    native_subissue_gate_blocker, normalize_state, RunningSnapshot, RuntimeSnapshot, SkippedIssue,
    TrackerIssue,
};
use crate::quality_gate::evaluate_issue_with_dependency_preflight;

#[derive(Debug, Clone)]
pub struct Orchestrator {
    config: RuntimeConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchPlan {
    pub selected: Vec<TrackerIssue>,
    pub snapshot: RuntimeSnapshot,
    pub integration_gaps: Vec<String>,
}

impl Orchestrator {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }

    pub fn plan_dispatch(&self, issues: Vec<TrackerIssue>) -> DispatchPlan {
        let active_states: BTreeSet<_> = self.config.active_state_set().into_iter().collect();
        let terminal_states: BTreeSet<_> = self.config.terminal_state_set().into_iter().collect();
        let mut selected = Vec::new();
        let mut skipped = Vec::new();
        let mut state_counts: BTreeMap<String, usize> = BTreeMap::new();

        for issue in sort_issues_for_dispatch(issues) {
            let normalized = issue.normalized_state();

            if !active_states.contains(&normalized) || terminal_states.contains(&normalized) {
                continue;
            }

            if issue_blocked_by_non_terminal(&issue, &terminal_states) {
                skipped.push(skip(&issue, "issue has unresolved tracker dependencies"));
                continue;
            }

            if let Some(reason) = native_subissue_gate_blocker(&issue, &terminal_states) {
                skipped.push(skip(&issue, &reason));
                continue;
            }

            if issue_has_rich_contract(&issue) {
                let gate = evaluate_issue_with_dependency_preflight(&issue, &terminal_states);
                if !gate.is_dispatchable() {
                    skipped.push(SkippedIssue {
                        issue_id: issue.id.clone(),
                        identifier: issue.identifier.clone(),
                        reason: "issue quality gate did not pass".into(),
                        gate: Some(gate),
                    });
                    continue;
                }
            }

            if selected.len() >= self.config.agent.max_concurrent_agents {
                skipped.push(skip(&issue, "global concurrency limit reached"));
                continue;
            }

            let state_limit = self
                .config
                .agent
                .max_concurrent_agents_by_state
                .get(&normalized)
                .copied()
                .unwrap_or(self.config.agent.max_concurrent_agents);
            let used_for_state = state_counts.get(&normalized).copied().unwrap_or(0);
            if used_for_state >= state_limit {
                skipped.push(skip(&issue, "state concurrency limit reached"));
                continue;
            }

            *state_counts.entry(normalized).or_insert(0) += 1;
            selected.push(issue);
        }

        let snapshot = RuntimeSnapshot {
            planned: selected
                .iter()
                .map(|issue| RunningSnapshot {
                    issue_id: issue.id.clone(),
                    identifier: issue.identifier.clone(),
                    state: issue.state.clone(),
                    backend: self.config.backend.kind.clone(),
                    workspace_path: None,
                    session_id: None,
                    profile_id: None,
                    instance_name: None,
                })
                .collect(),
            running: Vec::new(),
            retrying: Vec::new(),
            codex_totals: Default::default(),
            polling: crate::model::PollingSnapshot {
                checking: false,
                next_poll_in_ms: Some(self.config.polling.interval_ms),
                poll_interval_ms: self.config.polling.interval_ms,
            },
            sessions: Vec::new(),
            skipped,
            integration_gaps: Vec::new(),
            latest_status: None,
            event_log_path: None,
        };

        DispatchPlan {
            selected,
            snapshot,
            integration_gaps: Vec::new(),
        }
    }

    pub fn retry_delay_ms(&self, attempt: u32, continuation: bool) -> u64 {
        if continuation && attempt == 1 {
            return 1_000;
        }

        let power = attempt.saturating_sub(1).min(10);
        let delay = 10_000u64.saturating_mul(1u64 << power);
        delay.min(self.config.agent.max_retry_backoff_ms)
    }
}

fn sort_issues_for_dispatch(mut issues: Vec<TrackerIssue>) -> Vec<TrackerIssue> {
    issues.sort_by(|left, right| {
        priority_rank(left.priority)
            .cmp(&priority_rank(right.priority))
            .then_with(|| created_sort_key(left).cmp(created_sort_key(right)))
            .then_with(|| left.identifier.cmp(&right.identifier))
    });
    issues
}

fn priority_rank(priority: Option<i64>) -> i64 {
    priority
        .filter(|value| (1..=4).contains(value))
        .unwrap_or(5)
}

fn created_sort_key(issue: &TrackerIssue) -> &str {
    issue
        .created_at
        .as_deref()
        .unwrap_or("9999-12-31T23:59:59Z")
}

fn issue_blocked_by_non_terminal(issue: &TrackerIssue, terminal_states: &BTreeSet<String>) -> bool {
    matches!(issue.normalized_state().as_str(), "todo" | "rework")
        && issue.blocked_by.iter().any(|blocker| {
            blocker
                .state
                .as_deref()
                .map(normalize_state)
                .map(|state| !terminal_states.contains(&state))
                .unwrap_or(true)
        })
}

fn issue_has_rich_contract(issue: &TrackerIssue) -> bool {
    issue
        .description
        .as_deref()
        .map(|description| !description.trim().is_empty())
        .unwrap_or(false)
}

fn skip(issue: &TrackerIssue, reason: &str) -> SkippedIssue {
    SkippedIssue {
        issue_id: issue.id.clone(),
        identifier: issue.identifier.clone(),
        reason: reason.into(),
        gate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimeConfig;
    use crate::workflow::WorkflowDefinition;

    fn config() -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\nagent:\n  max_concurrent_agents: 1\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, std::path::Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn contract() -> String {
        [
            "## Issue Setup",
            "- UAT Required: No",
            "## Issue Goal",
            "Goal.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Dependencies",
            "- No blocking dependencies.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "Scope.",
            "## Canonical References",
            "### Target Repository / Package",
            "- repo",
            "## Verification",
            "### Completion Criteria",
            "- Pass.",
        ]
        .join("\n")
    }

    fn issue(id: &str, priority: i64) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: id.into(),
            item_id: None,
            identifier: format!("#{id}"),
            title: "Title".into(),
            description: Some(contract()),
            url: None,
            state: "Todo".into(),
            labels: vec![],
            assignees: vec![],
            priority: Some(priority),
            branch_name: None,
            linked_pull_requests: vec![],
            blocked_by: vec![],
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    fn parent_issue_with_subissues(subissues: Vec<serde_json::Value>) -> TrackerIssue {
        let mut parent = issue("243", 1);
        parent.project_fields.insert(
            "GitHub Native Subissues".into(),
            serde_json::Value::Array(subissues),
        );
        parent
    }

    #[test]
    fn dispatches_by_priority_and_limit() {
        let plan = Orchestrator::new(config()).plan_dispatch(vec![issue("2", 3), issue("1", 1)]);
        assert_eq!(plan.selected.len(), 1);
        assert_eq!(plan.selected[0].identifier, "#1");
        assert_eq!(plan.snapshot.skipped.len(), 1);
    }

    #[test]
    fn ignores_non_active_and_terminal_issues_before_skipped_candidates() {
        let mut backlog = issue("2", 2);
        backlog.state = "Backlog".into();
        let mut done = issue("3", 3);
        done.state = "Done".into();

        let plan = Orchestrator::new(config()).plan_dispatch(vec![issue("1", 1), backlog, done]);

        assert_eq!(plan.selected.len(), 1);
        assert_eq!(plan.selected[0].identifier, "#1");
        assert!(plan.snapshot.skipped.is_empty());
    }

    #[test]
    fn computes_reference_retry_delays() {
        let orchestrator = Orchestrator::new(config());
        assert_eq!(orchestrator.retry_delay_ms(1, true), 1_000);
        assert_eq!(orchestrator.retry_delay_ms(2, false), 20_000);
    }

    #[test]
    fn skips_blocked_todo_issue_before_claim() {
        let mut blocked = issue("1", 1);
        blocked.blocked_by = vec![crate::model::BlockerRef {
            id: None,
            identifier: Some("#parent".into()),
            state: Some("In Progress".into()),
        }];

        let plan = Orchestrator::new(config()).plan_dispatch(vec![blocked]);

        assert!(plan.selected.is_empty());
        assert_eq!(plan.snapshot.skipped[0].identifier, "#1");
        assert!(plan.snapshot.skipped[0].reason.contains("dependencies"));
    }

    #[test]
    fn skips_blocked_rework_issue_before_claim() {
        let mut blocked = issue("1", 1);
        blocked.state = "Rework".into();
        blocked.blocked_by = vec![crate::model::BlockerRef {
            id: None,
            identifier: Some("#parent".into()),
            state: Some("In Progress".into()),
        }];

        let plan = Orchestrator::new(config()).plan_dispatch(vec![blocked]);

        assert!(plan.selected.is_empty());
        assert_eq!(plan.snapshot.skipped[0].identifier, "#1");
        assert!(plan.snapshot.skipped[0].reason.contains("dependencies"));
    }

    #[test]
    fn dispatch_planning_allows_lightweight_issue_without_contract_body() {
        let mut lightweight = issue("1", 1);
        lightweight.description = None;
        lightweight.linked_pull_requests = Vec::new();

        let plan = Orchestrator::new(config()).plan_dispatch(vec![lightweight]);

        assert_eq!(plan.selected.len(), 1);
        assert_eq!(plan.selected[0].identifier, "#1");
        assert!(plan.snapshot.skipped.is_empty());
    }

    #[test]
    fn skips_parent_with_incomplete_native_subissues() {
        let parent = parent_issue_with_subissues(vec![
            serde_json::json!({"identifier": "#272", "project_state": "Done"}),
            serde_json::json!({"identifier": "#273", "project_state": "Todo"}),
        ]);

        let plan = Orchestrator::new(config()).plan_dispatch(vec![parent]);

        assert!(plan.selected.is_empty());
        assert_eq!(plan.snapshot.skipped[0].identifier, "#243");
        assert!(plan.snapshot.skipped[0]
            .reason
            .contains("blocked by incomplete native subissues"));
        assert!(plan.snapshot.skipped[0].reason.contains("#273=Todo"));
    }

    #[test]
    fn dispatches_parent_after_all_native_subissues_are_done() {
        let parent = parent_issue_with_subissues(vec![
            serde_json::json!({"identifier": "#272", "project_state": "Done"}),
            serde_json::json!({"identifier": "#273", "project_state": "Done"}),
        ]);

        let plan = Orchestrator::new(config()).plan_dispatch(vec![parent]);

        assert_eq!(plan.selected.len(), 1);
        assert_eq!(plan.selected[0].identifier, "#243");
    }

    #[test]
    fn treats_missing_native_subissue_project_state_as_incomplete() {
        let parent = parent_issue_with_subissues(vec![
            serde_json::json!({"identifier": "#272", "project_state": "Done"}),
            serde_json::json!({"identifier": "#274", "state": "closed"}),
        ]);

        let plan = Orchestrator::new(config()).plan_dispatch(vec![parent]);

        assert!(plan.selected.is_empty());
        assert!(plan.snapshot.skipped[0]
            .reason
            .contains("#274=missing Project status"));
    }
}
