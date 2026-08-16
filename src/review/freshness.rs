use serde::{Deserialize, Serialize};

use crate::workflow::WorkflowDefinition;
use crate::workpad_templates::{render_workpad_template, WorkpadTemplateId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStaleReason {
    MergeConflict,
    BaseBranchUpdated,
    ReviewOutdated,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewReworkClass {
    MechanicalConflictResolution,
    BaseRefresh,
    SemanticChange,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewFreshnessDecisionKind {
    PriorReviewStillValid,
    PriorReviewInvalidated,
    NeedsHumanInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFreshnessInput {
    pub issue_ref: String,
    pub prior_head_sha: String,
    pub current_head_sha: String,
    pub prior_base_sha: String,
    pub current_base_sha: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
    pub stale_reason: ReviewStaleReason,
    pub rework_class: ReviewReworkClass,
    pub patch_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFreshnessDecision {
    pub kind: ReviewFreshnessDecisionKind,
    pub prior_human_review_valid: bool,
    pub human_rereview_required: bool,
    pub main_agent_target_state: String,
    pub authorized_next_state: Option<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewFreshnessReport {
    pub input: ReviewFreshnessInput,
    pub decision: ReviewFreshnessDecision,
}

pub fn classify_review_freshness(input: ReviewFreshnessInput) -> ReviewFreshnessReport {
    let decision = match input.rework_class {
        ReviewReworkClass::MechanicalConflictResolution | ReviewReworkClass::BaseRefresh => {
            ReviewFreshnessDecision {
                kind: ReviewFreshnessDecisionKind::PriorReviewStillValid,
                prior_human_review_valid: true,
                human_rereview_required: false,
                main_agent_target_state: "agent_review".into(),
                authorized_next_state: Some("merging".into()),
                rationale: "Rework is classified as mechanical; prior Human Review can be preserved when evidence is recorded.".into(),
            }
        }
        ReviewReworkClass::SemanticChange => ReviewFreshnessDecision {
            kind: ReviewFreshnessDecisionKind::PriorReviewInvalidated,
            prior_human_review_valid: false,
            human_rereview_required: true,
            main_agent_target_state: "agent_review".into(),
            authorized_next_state: Some("agent_review".into()),
            rationale: "Semantic implementation changes invalidate prior Human Review and require the normal Agent Review then Human Review path.".into(),
        },
        ReviewReworkClass::Unknown => ReviewFreshnessDecision {
            kind: ReviewFreshnessDecisionKind::NeedsHumanInput,
            prior_human_review_valid: false,
            human_rereview_required: true,
            main_agent_target_state: "agent_review".into(),
            authorized_next_state: Some("need_human_input".into()),
            rationale: "Rework class is unknown, so prior review freshness cannot be safely preserved.".into(),
        },
    };

    ReviewFreshnessReport { input, decision }
}

pub fn render_review_freshness_workpad(
    workflow: Option<&WorkflowDefinition>,
    report: &ReviewFreshnessReport,
) -> Result<String, crate::prompt::PromptError> {
    const RECORD_SEPARATOR: &str = "\u{1e}";
    let input = &report.input;
    let decision = &report.decision;
    render_workpad_template(
        workflow,
        WorkpadTemplateId::ReviewFreshness,
        &[
            ("issue_ref", input.issue_ref.clone()),
            ("stale_reason", format!("{:?}", input.stale_reason)),
            ("rework_class", format!("{:?}", input.rework_class)),
            ("prior_head_sha", input.prior_head_sha.clone()),
            ("current_head_sha", input.current_head_sha.clone()),
            ("prior_base_sha", input.prior_base_sha.clone()),
            ("current_base_sha", input.current_base_sha.clone()),
            (
                "prior_human_review_valid",
                decision.prior_human_review_valid.to_string(),
            ),
            (
                "human_rereview_required",
                decision.human_rereview_required.to_string(),
            ),
            (
                "main_agent_target_state",
                decision.main_agent_target_state.clone(),
            ),
            (
                "authorized_next_state",
                decision
                    .authorized_next_state
                    .clone()
                    .unwrap_or_else(|| "none".into()),
            ),
            ("decision", format!("{:?}", decision.kind)),
            ("rationale", decision.rationale.clone()),
            ("changed_files", input.changed_files.join(RECORD_SEPARATOR)),
            (
                "patch_summary",
                input
                    .patch_summary
                    .as_deref()
                    .filter(|summary| !summary.trim().is_empty())
                    .unwrap_or("Not recorded.")
                    .into(),
            ),
            ("record_separator", RECORD_SEPARATOR.into()),
        ],
    )
}
