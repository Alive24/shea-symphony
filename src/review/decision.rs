use serde::{Deserialize, Serialize};

use crate::model::{is_native_subissue, native_subissue_human_review_exception, TrackerIssue};

use super::{
    gemini_review_health_diagnostic, review_required_operator_actions, ReviewJob, ReviewJobState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewOutcome {
    PassedToHumanReview,
    PassedToMerging,
    NeedsRework,
    InconclusiveNeedsRework,
    NeedsHumanInput,
    BackendUnavailable,
    StillRunning,
    Cancelled,
}

impl ReviewOutcome {
    pub fn is_passed(self) -> bool {
        matches!(self, Self::PassedToHumanReview | Self::PassedToMerging)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGateDecision {
    pub outcome: ReviewOutcome,
    pub target_state: Option<&'static str>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewActor {
    MainImplementationAgent,
    IndependentReviewAgent,
}

pub fn main_agent_completion_decision() -> ReviewGateDecision {
    ReviewGateDecision {
        outcome: ReviewOutcome::StillRunning,
        target_state: Some("agent_review"),
        message:
            "Main implementation agent completed local work; independent Agent Review is required."
                .into(),
    }
}

pub fn review_gate_decision(job: &ReviewJob) -> ReviewGateDecision {
    review_gate_decision_for_actor(job, ReviewActor::IndependentReviewAgent)
}

pub fn review_gate_decision_for_issue(job: &ReviewJob, issue: &TrackerIssue) -> ReviewGateDecision {
    let decision = review_gate_decision(job);
    if decision.outcome == ReviewOutcome::PassedToHumanReview
        && review_pass_target_state(issue) == "merging"
    {
        return ReviewGateDecision {
            outcome: ReviewOutcome::PassedToMerging,
            target_state: Some("merging"),
            message: "Independent Agent Review passed with recorded evidence; native subissue routes directly to Merging because the parent issue owns final Human Review and UAT.".into(),
        };
    }
    decision
}

pub fn review_pass_target_state(issue: &TrackerIssue) -> &'static str {
    if is_native_subissue(issue) && !native_subissue_human_review_exception(issue) {
        "merging"
    } else {
        "human_review"
    }
}

pub fn review_gate_decision_for_actor(job: &ReviewJob, actor: ReviewActor) -> ReviewGateDecision {
    if actor == ReviewActor::MainImplementationAgent {
        return main_agent_completion_decision();
    }

    match job.state {
        ReviewJobState::Queued | ReviewJobState::Running => ReviewGateDecision {
            outcome: ReviewOutcome::StillRunning,
            target_state: Some("agent_review"),
            message: "Agent review is still running; issue remains in Agent Review.".into(),
        },
        ReviewJobState::Completed => match &job.report {
            Some(report) if report.blocks_progress() => ReviewGateDecision {
                outcome: ReviewOutcome::NeedsRework,
                target_state: Some("rework"),
                message: "Confirmed Agent Review findings require Rework.".into(),
            },
            Some(report) if report.is_inconclusive() => ReviewGateDecision {
                outcome: ReviewOutcome::InconclusiveNeedsRework,
                target_state: Some("rework"),
                message: format!(
                    "Agent Review was inconclusive and requires Rework: {}.",
                    report
                        .inconclusive_reason()
                        .unwrap_or_else(|| "review could not complete with durable evidence".into())
                ),
            },
            Some(_) => ReviewGateDecision {
                outcome: ReviewOutcome::PassedToHumanReview,
                target_state: Some("human_review"),
                message: "Independent Agent Review passed with recorded evidence; issue is ready for Human Review.".into(),
            },
            None => ReviewGateDecision {
                outcome: ReviewOutcome::NeedsHumanInput,
                target_state: Some("need_human_input"),
                message: "Agent review completed without a report.".into(),
            },
        },
        ReviewJobState::Failed | ReviewJobState::TimedOut => {
            if let Some(diagnostic) = gemini_review_health_diagnostic(job) {
                if diagnostic.is_recoverable() {
                    return ReviewGateDecision {
                        outcome: ReviewOutcome::BackendUnavailable,
                        target_state: Some("agent_review"),
                        message: format!(
                            "Review backend is {}; issue remains in Agent Review for {}.",
                            diagnostic.category.as_str(),
                            diagnostic.recovery_policy.as_str()
                        ),
                    };
                }

                return ReviewGateDecision {
                    outcome: ReviewOutcome::NeedsHumanInput,
                    target_state: Some("need_human_input"),
                    message: format!(
                        "Review backend is blocked by {}; human input is required.",
                        diagnostic.category.as_str()
                    ),
                };
            }

            if review_required_operator_actions(job).is_some() {
                return ReviewGateDecision {
                    outcome: ReviewOutcome::BackendUnavailable,
                    target_state: Some("agent_review"),
                    message:
                        "Agent Review backend is blocked by required operator action; issue remains in Agent Review."
                            .into(),
                };
            }

            ReviewGateDecision {
                outcome: ReviewOutcome::NeedsHumanInput,
                target_state: Some("need_human_input"),
                message: "Agent review failed or timed out; human input is required.".into(),
            }
        }
        ReviewJobState::Cancelled => ReviewGateDecision {
            outcome: ReviewOutcome::Cancelled,
            target_state: Some("agent_review"),
            message: "Agent review was cancelled; issue remains in Agent Review.".into(),
        },
    }
}

pub fn transition_allowed_for_main_agent(normalized_state: &str) -> bool {
    !matches!(normalized_state, "human_review" | "human review")
}

pub fn transition_allowed_for_review_agent(
    normalized_state: &str,
    decision: &ReviewGateDecision,
) -> bool {
    match normalized_state {
        "human_review" | "human review" => decision.outcome == ReviewOutcome::PassedToHumanReview,
        "merging" => decision.outcome == ReviewOutcome::PassedToMerging,
        "rework" => matches!(
            decision.outcome,
            ReviewOutcome::NeedsRework | ReviewOutcome::InconclusiveNeedsRework
        ),
        "need_human_input" | "need human input" => {
            decision.outcome == ReviewOutcome::NeedsHumanInput
        }
        "agent_review" | "agent review" => {
            matches!(
                decision.outcome,
                ReviewOutcome::BackendUnavailable
                    | ReviewOutcome::StillRunning
                    | ReviewOutcome::Cancelled
            )
        }
        _ => true,
    }
}
