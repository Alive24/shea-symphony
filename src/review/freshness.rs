use serde::{Deserialize, Serialize};

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

pub fn render_review_freshness_workpad(report: &ReviewFreshnessReport) -> String {
    let input = &report.input;
    let decision = &report.decision;
    let mut lines = vec![
        "## Review Freshness".to_string(),
        String::new(),
        format!("- Issue: {}", input.issue_ref),
        format!("- Stale reason: {:?}", input.stale_reason),
        format!("- Rework class: {:?}", input.rework_class),
        format!("- Prior head SHA: `{}`", input.prior_head_sha),
        format!("- Current head SHA: `{}`", input.current_head_sha),
        format!("- Prior base SHA: `{}`", input.prior_base_sha),
        format!("- Current base SHA: `{}`", input.current_base_sha),
        format!(
            "- Prior Human Review still valid: `{}`",
            decision.prior_human_review_valid
        ),
        format!(
            "- Human re-review required: `{}`",
            decision.human_rereview_required
        ),
        format!(
            "- Main-agent target state: `{}`",
            decision.main_agent_target_state
        ),
        format!(
            "- Authorized next state after review freshness evidence: `{}`",
            decision.authorized_next_state.as_deref().unwrap_or("none")
        ),
        format!("- Decision: {:?}", decision.kind),
        format!("- Rationale: {}", decision.rationale),
    ];

    lines.push(String::new());
    lines.push("### Changed Files".into());
    if input.changed_files.is_empty() {
        lines.push("- None recorded.".into());
    } else {
        lines.extend(input.changed_files.iter().map(|file| format!("- `{file}`")));
    }

    lines.push(String::new());
    lines.push("### Patch Summary".into());
    lines.push(
        input
            .patch_summary
            .as_deref()
            .filter(|summary| !summary.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Not recorded.".into()),
    );

    lines.push(String::new());
    lines.push("### Authority Boundary".into());
    lines.push("- This freshness report is evidence, not an automatic approval.".into());
    lines.push("- Main implementation agent still stops at `Agent Review`.".into());
    lines.push("- `Human Review` remains reserved for an independent Review Agent or human-authorized workflow.".into());

    lines.join("\n")
}
