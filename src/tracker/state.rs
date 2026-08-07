use std::collections::BTreeSet;

use thiserror::Error;

use crate::config::{AssigneeFilter, RuntimeConfig, StateMap};
use crate::model::{normalize_state, TrackerIssue};

const CONFIGURED_STATE_KEYS: [&str; 10] = [
    "backlog",
    "todo",
    "need_to_clarify",
    "in_progress",
    "need_human_input",
    "agent_review",
    "human_review",
    "rework",
    "merging",
    "done",
];

/// A configured tracker state resolved to Symphony's canonical key and provider display value.
///
/// The canonical key is used for internal comparison and transition idempotency;
/// the display value remains the provider-facing value configured in `state_map`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTrackerState {
    canonical_key: &'static str,
    display_value: String,
}

impl ResolvedTrackerState {
    /// Returns the stable internal `state_map` key for this configured state.
    pub fn canonical_key(&self) -> &'static str {
        self.canonical_key
    }

    /// Returns the configured provider display value without canonicalizing it.
    pub fn display_value(&self) -> &str {
        &self.display_value
    }
}

/// Typed diagnostic emitted when a value cannot identify exactly one configured tracker state.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TrackerStateResolutionError {
    /// The supplied value was neither a configured canonical key nor a configured display value.
    #[error("unknown configured tracker state input {input:?}")]
    UnknownInput {
        /// The unsupported input exactly as supplied by the caller.
        input: String,
    },
    /// Multiple configured states accept the same trim- and case-normalized input.
    #[error(
        "ambiguous configured tracker state input {input:?}; it resolves to {}",
        canonical_keys.join(", ")
    )]
    AmbiguousConfiguration {
        /// The input that matched more than one configured state.
        input: String,
        /// Canonical keys that collide for this input.
        canonical_keys: Vec<String>,
    },
}

/// Resolves an exact configured key or display value to one canonical internal tracker state.
///
/// Comparison trims only surrounding whitespace and folds case. It deliberately
/// does not normalize punctuation, underscores, interior whitespace, aliases,
/// or approximate spellings. A collision is rejected rather than depending on
/// configuration field order.
pub fn resolve_configured_tracker_state(
    state_map: &StateMap,
    input: &str,
) -> Result<ResolvedTrackerState, TrackerStateResolutionError> {
    let normalized_input = normalize_configured_state_input(input);
    let matches = configured_states(state_map)
        .into_iter()
        .filter(|(canonical_key, display_value)| {
            normalized_input == normalize_configured_state_input(canonical_key)
                || normalized_input == normalize_configured_state_input(display_value)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Err(TrackerStateResolutionError::UnknownInput {
            input: input.to_string(),
        }),
        [(canonical_key, display_value)] => Ok(ResolvedTrackerState {
            canonical_key,
            display_value: (*display_value).to_string(),
        }),
        _ => Err(TrackerStateResolutionError::AmbiguousConfiguration {
            input: input.to_string(),
            canonical_keys: matches
                .iter()
                .map(|(canonical_key, _)| (*canonical_key).to_string())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimDecision {
    AlreadyInProgress,
    Claimable,
    StopAndReplan { current_state: String },
}

pub fn claim_decision(issue: &TrackerIssue, config: &RuntimeConfig) -> ClaimDecision {
    match resolve_configured_tracker_state(&config.tracker.state_map, &issue.state) {
        Ok(state) if state.canonical_key() == "in_progress" => ClaimDecision::AlreadyInProgress,
        Ok(state) if matches!(state.canonical_key(), "todo" | "rework") => ClaimDecision::Claimable,
        Ok(_) | Err(_) => ClaimDecision::StopAndReplan {
            current_state: issue.state.clone(),
        },
    }
}

pub(in crate::tracker) fn status_update_required(issue: &TrackerIssue, target_state: &str) -> bool {
    normalize_configured_state_input(&issue.state) != normalize_configured_state_input(target_state)
}

pub(in crate::tracker) fn status_is_mapped(status: &str, config: &RuntimeConfig) -> bool {
    resolve_configured_tracker_state(&config.tracker.state_map, status).is_ok()
}

pub(in crate::tracker) fn issue_matches_assignee_filter(
    issue: &TrackerIssue,
    filter: &AssigneeFilter,
    current_login: Option<&str>,
) -> bool {
    if issue.assignees.is_empty() {
        return false;
    }

    let allowed: Vec<String> = current_login
        .into_iter()
        .chain(filter.additional_assignees.iter().map(String::as_str))
        .map(normalize_state)
        .collect();

    issue
        .assignees
        .iter()
        .any(|assignee| allowed.contains(&normalize_state(assignee)))
}

pub(in crate::tracker) fn configured_state_has_key(
    state_map: &StateMap,
    input: &str,
    canonical_keys: &[&str],
) -> bool {
    resolve_configured_tracker_state(state_map, input)
        .map(|state| canonical_keys.contains(&state.canonical_key()))
        .unwrap_or(false)
}

fn normalize_configured_state_input(input: &str) -> String {
    input.trim().to_lowercase()
}

fn configured_states(state_map: &StateMap) -> [(&'static str, &str); 10] {
    [
        (CONFIGURED_STATE_KEYS[0], &state_map.backlog),
        (CONFIGURED_STATE_KEYS[1], &state_map.todo),
        (CONFIGURED_STATE_KEYS[2], &state_map.need_to_clarify),
        (CONFIGURED_STATE_KEYS[3], &state_map.in_progress),
        (CONFIGURED_STATE_KEYS[4], &state_map.need_human_input),
        (CONFIGURED_STATE_KEYS[5], &state_map.agent_review),
        (CONFIGURED_STATE_KEYS[6], &state_map.human_review),
        (CONFIGURED_STATE_KEYS[7], &state_map.rework),
        (CONFIGURED_STATE_KEYS[8], &state_map.merging),
        (CONFIGURED_STATE_KEYS[9], &state_map.done),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_state_map() -> StateMap {
        StateMap {
            backlog: "Backlog".to_string(),
            todo: "Todo".to_string(),
            need_to_clarify: "Need to Clarify".to_string(),
            in_progress: "In Progress".to_string(),
            need_human_input: "Need Human Input".to_string(),
            agent_review: "Agent Review".to_string(),
            human_review: "Human Review".to_string(),
            rework: "Rework".to_string(),
            merging: "Merging".to_string(),
            done: "Done".to_string(),
        }
    }

    #[test]
    fn resolves_every_configured_key_and_display_value() {
        let state_map = default_state_map();
        for (key, display) in configured_states(&state_map) {
            assert_eq!(
                resolve_configured_tracker_state(&state_map, key)
                    .unwrap()
                    .canonical_key(),
                key
            );
            let resolved = resolve_configured_tracker_state(&state_map, display).unwrap();
            assert_eq!(resolved.canonical_key(), key);
            assert_eq!(resolved.display_value(), display);
        }
    }

    #[test]
    fn resolves_custom_displays_with_case_and_surrounding_whitespace() {
        let mut state_map = default_state_map();
        state_map.need_to_clarify = "Triage Needed".to_string();

        let resolved = resolve_configured_tracker_state(&state_map, "  tRiAgE nEeDeD  ").unwrap();

        assert_eq!(resolved.canonical_key(), "need_to_clarify");
        assert_eq!(resolved.display_value(), "Triage Needed");
    }

    #[test]
    fn rejects_unknown_aliases_and_punctuation_variants() {
        let state_map = default_state_map();
        for input in [
            "in-progress",
            "need-human-input",
            "need__human__input",
            "review",
        ] {
            assert_eq!(
                resolve_configured_tracker_state(&state_map, input),
                Err(TrackerStateResolutionError::UnknownInput {
                    input: input.to_string(),
                })
            );
        }
    }

    #[test]
    fn rejects_configuration_collisions_without_selecting_a_state() {
        let mut state_map = default_state_map();
        state_map.todo = "Review".to_string();
        state_map.rework = "review".to_string();

        assert_eq!(
            resolve_configured_tracker_state(&state_map, " REVIEW "),
            Err(TrackerStateResolutionError::AmbiguousConfiguration {
                input: " REVIEW ".to_string(),
                canonical_keys: vec!["rework".to_string(), "todo".to_string()],
            })
        );
    }
}
