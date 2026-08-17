mod automatic;
mod manual;
mod status;

#[cfg(test)]
pub(crate) use automatic::{
    apply_review_result, canonical_issue_body_without_workpad,
    check_review_verified_issue_body_checkboxes, render_automatic_review_prompt,
    render_automatic_review_prompt_for_backend, review_claim_for_issue, review_workspace_for_issue,
    strong_canonical_review_workspace, terminal_review_loop_claim_value,
    transition_issue_to_rework_with_diagnostic,
};
pub(crate) use automatic::{
    review_backend_kind, review_fake, review_loop, review_loop_with_summary, review_once,
    select_review_worker_issues, ReviewLoopOptions, ReviewLoopSummary,
};
#[cfg(test)]
pub(crate) use manual::{
    render_manual_review_workpad, terminal_review_claim_value, validate_active_manual_review_claim,
    validate_manual_review_pass_claim, ManualReviewWorkpadInput,
};
pub(crate) use manual::{
    review_claim, review_clear_claim, review_manual_pass, review_manual_reject,
};
pub(crate) use status::{review_freshness, review_status, ReviewStatusCliOptions};
