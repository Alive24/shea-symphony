//! Activation, identity, and Temporal start policy for Coordinator episodes.
//!
//! The activation contract classifies tracker snapshots that a caller has
//! already observed without I/O. The start boundary consumes only those
//! validated executable facts and performs one Temporal start plus one
//! immediate Describe. Neither boundary reads tracker or SQLite state.

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]
// This contract intentionally lands before its in-crate consumers in #502-#505.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::{OffsetDateTime, UtcOffset};

use super::{IssueRef, WorkflowId};

pub(crate) mod reconcile;
pub(crate) mod start;

/// Maximum accepted byte length of an activation's audit reason.
pub(crate) const MAX_AUDIT_REASON_BYTES: usize = 512;

/// Maximum byte length of a Coordinator-generated Temporal Workflow ID.
pub(crate) const MAX_WORKFLOW_ID_BYTES: usize = 256;

/// Tracker states understood by the Coordinator activation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CoordinatorTrackerState {
    /// Work has not yet been admitted for execution.
    Backlog,
    /// Work is queued for contract validation or implementation.
    Todo,
    /// The issue contract requires clarification before execution.
    NeedToClarify,
    /// Main implementation work is active or resumable.
    InProgress,
    /// Execution is waiting for a human decision or unavailable prerequisite.
    NeedHumanInput,
    /// Main implementation is ready for independent agent review.
    AgentReview,
    /// Independent review has passed and the issue awaits human review.
    HumanReview,
    /// Confirmed findings require implementation changes.
    Rework,
    /// Approved work is ready for the merge lane.
    Merging,
    /// The issue lifecycle is complete.
    Done,
}

impl CoordinatorTrackerState {
    /// Returns the durable lowercase kebab-case identity spelling.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "todo",
            Self::NeedToClarify => "need-to-clarify",
            Self::InProgress => "in-progress",
            Self::NeedHumanInput => "need-human-input",
            Self::AgentReview => "agent-review",
            Self::HumanReview => "human-review",
            Self::Rework => "rework",
            Self::Merging => "merging",
            Self::Done => "done",
        }
    }

    /// Derives the one target kind owned by an executable tracker state.
    const fn target_kind(self) -> Option<CoordinatorTargetKind> {
        match self {
            Self::Todo | Self::InProgress => Some(CoordinatorTargetKind::Work),
            Self::AgentReview => Some(CoordinatorTargetKind::Review),
            Self::Rework => Some(CoordinatorTargetKind::Rework),
            Self::Merging => Some(CoordinatorTargetKind::Merge),
            Self::Backlog
            | Self::NeedToClarify
            | Self::NeedHumanInput
            | Self::HumanReview
            | Self::Done => None,
        }
    }
}

/// Coordinator-owned intent for an executable workflow episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CoordinatorTargetKind {
    /// Contract validation or main implementation work.
    Work,
    /// Independent agent review work.
    Review,
    /// Implementation repair after confirmed findings.
    Rework,
    /// Approved branch landing work.
    Merge,
}

impl CoordinatorTargetKind {
    /// Returns the durable lowercase kebab-case identity spelling.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Review => "review",
            Self::Rework => "rework",
            Self::Merge => "merge",
        }
    }
}

/// Stable category for the event that caused activation evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CoordinatorSourceKind {
    /// A tracker observation or tracker revision changed.
    Tracker,
    /// A structured human or operator action requested work.
    OperatorAction,
    /// A bounded Doctor operation produced a new executable condition.
    Doctor,
    /// Reconciliation produced a new executable condition.
    Reconciliation,
}

impl CoordinatorSourceKind {
    /// Returns the durable lowercase kebab-case identity spelling.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Tracker => "tracker",
            Self::OperatorAction => "operator-action",
            Self::Doctor => "doctor",
            Self::Reconciliation => "reconciliation",
        }
    }
}

/// Identifies which repository component failed request validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum RepositoryComponent {
    /// Repository source host.
    #[error("host")]
    Host,
    /// Repository owner or organization.
    #[error("owner")]
    Owner,
    /// Repository name.
    #[error("repository")]
    Repository,
}

/// Identifies which tracker revision failed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum TrackerRevisionKind {
    /// Optional revision supplied as an optimistic expectation.
    #[error("expected")]
    Expected,
    /// Revision carried by the already-observed tracker snapshot.
    #[error("observed")]
    Observed,
}

/// Validation failures produced by the pure activation and identity contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum CoordinatorActivationError {
    /// A required repository identity component was empty.
    #[error("repository {component} must not be empty")]
    EmptyRepositoryComponent {
        /// Empty component.
        component: RepositoryComponent,
    },
    /// Issue number zero cannot identify a tracker issue.
    #[error("issue number must be non-zero")]
    ZeroIssueNumber,
    /// Episode timestamps must explicitly use the UTC offset.
    #[error("episode time must use the UTC offset")]
    EpisodeTimeNotUtc,
    /// Episode timestamps must not carry subsecond data.
    #[error("episode time must have second precision")]
    EpisodeTimeNotSecondPrecision,
    /// Episode timestamps must fit the four-digit identity grammar.
    #[error("episode year {year} is outside the four-digit Workflow ID grammar")]
    EpisodeYearOutOfRange {
        /// Rejected year.
        year: i32,
    },
    /// Source references are required identity input.
    #[error("source reference must not be empty")]
    EmptySourceRef,
    /// Tracker revisions, when present, must identify an observation.
    #[error("{kind} tracker revision must not be empty")]
    EmptyTrackerRevision {
        /// Revision role.
        kind: TrackerRevisionKind,
    },
    /// Audit reasons are required after trimming surrounding whitespace.
    #[error("audit reason must not be empty")]
    EmptyAuditReason,
    /// Audit reason length is measured in UTF-8 bytes after trimming.
    #[error("audit reason is {actual_bytes} bytes; maximum is {max_bytes}")]
    AuditReasonTooLong {
        /// Actual UTF-8 byte length.
        actual_bytes: usize,
        /// Contract maximum.
        max_bytes: usize,
    },
    /// The complete encoded Workflow ID exceeded the internal limit.
    #[error("Workflow ID is {actual_bytes} bytes; maximum is {max_bytes}")]
    WorkflowIdTooLong {
        /// Actual encoded byte length.
        actual_bytes: usize,
        /// Contract maximum.
        max_bytes: usize,
    },
}

/// Tracker state and revision already observed by an I/O-owning caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedTrackerSnapshot {
    state: CoordinatorTrackerState,
    revision: String,
}

impl ObservedTrackerSnapshot {
    /// Builds a snapshot without reading the tracker.
    pub(crate) fn new(
        state: CoordinatorTrackerState,
        revision: impl Into<String>,
    ) -> Result<Self, CoordinatorActivationError> {
        let revision = revision.into();
        if revision.trim().is_empty() {
            return Err(CoordinatorActivationError::EmptyTrackerRevision {
                kind: TrackerRevisionKind::Observed,
            });
        }

        Ok(Self { state, revision })
    }

    /// Returns the observed tracker state.
    pub(crate) const fn state(&self) -> CoordinatorTrackerState {
        self.state
    }

    /// Borrows the opaque observed tracker revision.
    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
}

/// Validated, side-effect-free input for one activation evaluation.
///
/// Target kind is deliberately absent: only the Coordinator may derive it
/// from the observed tracker state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinatorActivationRequest {
    issue_ref: IssueRef,
    expected_tracker_state: Option<CoordinatorTrackerState>,
    expected_tracker_revision: Option<String>,
    episode_time: OffsetDateTime,
    source_kind: CoordinatorSourceKind,
    source_ref: String,
    audit_reason: String,
}

impl CoordinatorActivationRequest {
    /// Validates and builds a bounded activation request.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        issue_ref: IssueRef,
        expected_tracker_state: Option<CoordinatorTrackerState>,
        expected_tracker_revision: Option<String>,
        episode_time: OffsetDateTime,
        source_kind: CoordinatorSourceKind,
        source_ref: impl Into<String>,
        audit_reason: impl Into<String>,
    ) -> Result<Self, CoordinatorActivationError> {
        validate_issue_ref(&issue_ref)?;
        validate_episode_time(episode_time)?;

        if expected_tracker_revision
            .as_deref()
            .is_some_and(|revision| revision.trim().is_empty())
        {
            return Err(CoordinatorActivationError::EmptyTrackerRevision {
                kind: TrackerRevisionKind::Expected,
            });
        }

        let source_ref = source_ref.into();
        if source_ref.trim().is_empty() {
            return Err(CoordinatorActivationError::EmptySourceRef);
        }

        // Audit text is provenance, not identity; normalize only its documented
        // surrounding whitespace and measure the retained UTF-8 bytes.
        let audit_reason = audit_reason.into();
        let audit_reason = audit_reason.trim().to_owned();
        if audit_reason.is_empty() {
            return Err(CoordinatorActivationError::EmptyAuditReason);
        }
        if audit_reason.len() > MAX_AUDIT_REASON_BYTES {
            return Err(CoordinatorActivationError::AuditReasonTooLong {
                actual_bytes: audit_reason.len(),
                max_bytes: MAX_AUDIT_REASON_BYTES,
            });
        }

        Ok(Self {
            issue_ref,
            expected_tracker_state,
            expected_tracker_revision,
            episode_time,
            source_kind,
            source_ref,
            audit_reason,
        })
    }

    /// Borrows the existing fully qualified issue identity.
    pub(crate) const fn issue_ref(&self) -> &IssueRef {
        &self.issue_ref
    }

    /// Returns the optional optimistic state expectation.
    pub(crate) const fn expected_tracker_state(&self) -> Option<CoordinatorTrackerState> {
        self.expected_tracker_state
    }

    /// Borrows the optional optimistic tracker revision expectation.
    pub(crate) fn expected_tracker_revision(&self) -> Option<&str> {
        self.expected_tracker_revision.as_deref()
    }

    /// Returns the explicit UTC-second episode timestamp.
    pub(crate) const fn episode_time(&self) -> OffsetDateTime {
        self.episode_time
    }

    /// Returns the stable activation source category.
    pub(crate) const fn source_kind(&self) -> CoordinatorSourceKind {
        self.source_kind
    }

    /// Borrows the exact, unencoded source reference.
    pub(crate) fn source_ref(&self) -> &str {
        &self.source_ref
    }

    /// Borrows the trimmed audit reason, which is never embedded in identity.
    pub(crate) fn audit_reason(&self) -> &str {
        &self.audit_reason
    }

    /// Evaluates one already-observed tracker snapshot.
    ///
    /// Optimistic expectation checks happen before state classification.
    /// Consequently stale or static observations never fabricate executable
    /// activation facts or a Workflow ID.
    pub(crate) fn evaluate(
        &self,
        observed: &ObservedTrackerSnapshot,
    ) -> Result<CoordinatorActivationDecision, CoordinatorActivationError> {
        if self
            .expected_tracker_state
            .is_some_and(|expected| expected != observed.state)
            || self
                .expected_tracker_revision
                .as_deref()
                .is_some_and(|expected| expected != observed.revision)
        {
            return Ok(CoordinatorActivationDecision::StaleExpectation {
                issue_ref: self.issue_ref.clone(),
                expected_tracker_state: self.expected_tracker_state,
                expected_tracker_revision: self.expected_tracker_revision.clone(),
                observed: observed.clone(),
            });
        }

        let Some(target_kind) = observed.state.target_kind() else {
            return Ok(CoordinatorActivationDecision::Static {
                issue_ref: self.issue_ref.clone(),
                observed: observed.clone(),
            });
        };

        let workflow_id = build_workflow_id(self, observed.state, target_kind)?;
        Ok(CoordinatorActivationDecision::Executable(
            CoordinatorExecutableActivation {
                issue_ref: self.issue_ref.clone(),
                observed: observed.clone(),
                target_kind,
                episode_time: self.episode_time,
                source_kind: self.source_kind,
                source_ref: self.source_ref.clone(),
                audit_reason: self.audit_reason.clone(),
                workflow_id,
            },
        ))
    }
}

/// Result of evaluating a validated request against an observed snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoordinatorActivationDecision {
    /// The observation is a normal non-executable tracker lane.
    Static {
        /// Existing issue identity.
        issue_ref: IssueRef,
        /// State and revision that were classified.
        observed: ObservedTrackerSnapshot,
    },
    /// The observation may be used to start or find an execution.
    Executable(CoordinatorExecutableActivation),
    /// At least one supplied optimistic expectation no longer matches.
    StaleExpectation {
        /// Existing issue identity.
        issue_ref: IssueRef,
        /// State expectation supplied by the caller, when any.
        expected_tracker_state: Option<CoordinatorTrackerState>,
        /// Revision expectation supplied by the caller, when any.
        expected_tracker_revision: Option<String>,
        /// State and revision that invalidated the expectation.
        observed: ObservedTrackerSnapshot,
    },
}

/// Complete immutable facts for one executable activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoordinatorExecutableActivation {
    issue_ref: IssueRef,
    observed: ObservedTrackerSnapshot,
    target_kind: CoordinatorTargetKind,
    episode_time: OffsetDateTime,
    source_kind: CoordinatorSourceKind,
    source_ref: String,
    audit_reason: String,
    workflow_id: WorkflowId,
}

impl CoordinatorExecutableActivation {
    /// Borrows the fully qualified issue identity.
    pub(crate) const fn issue_ref(&self) -> &IssueRef {
        &self.issue_ref
    }

    /// Borrows the observed tracker state and revision.
    pub(crate) const fn observed(&self) -> &ObservedTrackerSnapshot {
        &self.observed
    }

    /// Returns the Coordinator-derived execution target.
    pub(crate) const fn target_kind(&self) -> CoordinatorTargetKind {
        self.target_kind
    }

    /// Returns the explicit episode timestamp used in identity.
    pub(crate) const fn episode_time(&self) -> OffsetDateTime {
        self.episode_time
    }

    /// Returns the stable provenance category.
    pub(crate) const fn source_kind(&self) -> CoordinatorSourceKind {
        self.source_kind
    }

    /// Borrows the exact, unencoded provenance reference.
    pub(crate) fn source_ref(&self) -> &str {
        &self.source_ref
    }

    /// Borrows the trimmed audit reason, which is not identity input.
    pub(crate) fn audit_reason(&self) -> &str {
        &self.audit_reason
    }

    /// Borrows the validated Temporal Workflow ID.
    pub(crate) const fn workflow_id(&self) -> &WorkflowId {
        &self.workflow_id
    }
}

fn validate_issue_ref(issue_ref: &IssueRef) -> Result<(), CoordinatorActivationError> {
    for (component, value) in [
        (RepositoryComponent::Host, issue_ref.repo_id.host.as_str()),
        (RepositoryComponent::Owner, issue_ref.repo_id.owner.as_str()),
        (
            RepositoryComponent::Repository,
            issue_ref.repo_id.repo.as_str(),
        ),
    ] {
        if value.trim().is_empty() {
            return Err(CoordinatorActivationError::EmptyRepositoryComponent { component });
        }
    }

    if issue_ref.number == 0 {
        return Err(CoordinatorActivationError::ZeroIssueNumber);
    }

    Ok(())
}

fn validate_episode_time(episode_time: OffsetDateTime) -> Result<(), CoordinatorActivationError> {
    if episode_time.offset() != UtcOffset::UTC {
        return Err(CoordinatorActivationError::EpisodeTimeNotUtc);
    }
    if episode_time.nanosecond() != 0 {
        return Err(CoordinatorActivationError::EpisodeTimeNotSecondPrecision);
    }
    if !(0..=9999).contains(&episode_time.year()) {
        return Err(CoordinatorActivationError::EpisodeYearOutOfRange {
            year: episode_time.year(),
        });
    }

    Ok(())
}

fn build_workflow_id(
    request: &CoordinatorActivationRequest,
    from_state: CoordinatorTrackerState,
    target_kind: CoordinatorTargetKind,
) -> Result<WorkflowId, CoordinatorActivationError> {
    let repo = &request.issue_ref.repo_id;
    let episode_time = request.episode_time;
    let workflow_id = format!(
        "issue:{}/{}/{}:{}:pulse:{}-to-{}:{:04}{:02}{:02}T{:02}{:02}{:02}Z:{}-{}",
        encode_component(&repo.host),
        encode_component(&repo.owner),
        encode_component(&repo.repo),
        request.issue_ref.number,
        from_state.as_str(),
        target_kind.as_str(),
        episode_time.year(),
        u8::from(episode_time.month()),
        episode_time.day(),
        episode_time.hour(),
        episode_time.minute(),
        episode_time.second(),
        request.source_kind.as_str(),
        encode_component(&request.source_ref),
    );

    // Percent-encoded UTF-8 can expand substantially, so enforce the portable
    // internal limit only after the complete retry-stable ID is assembled.
    if workflow_id.len() > MAX_WORKFLOW_ID_BYTES {
        return Err(CoordinatorActivationError::WorkflowIdTooLong {
            actual_bytes: workflow_id.len(),
            max_bytes: MAX_WORKFLOW_ID_BYTES,
        });
    }

    Ok(WorkflowId::new(workflow_id))
}

fn encode_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            // Encode UTF-8 bytes rather than Unicode scalar values so decoding
            // is lossless while identity separators remain unambiguous.
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use time::{Duration, Month};

    use super::*;
    use crate::symphony::{RepoId, TrackerBackend};

    const ALL_STATES: [CoordinatorTrackerState; 10] = [
        CoordinatorTrackerState::Todo,
        CoordinatorTrackerState::InProgress,
        CoordinatorTrackerState::AgentReview,
        CoordinatorTrackerState::Rework,
        CoordinatorTrackerState::Merging,
        CoordinatorTrackerState::Backlog,
        CoordinatorTrackerState::NeedToClarify,
        CoordinatorTrackerState::NeedHumanInput,
        CoordinatorTrackerState::HumanReview,
        CoordinatorTrackerState::Done,
    ];

    fn episode_time() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_784_992_800).unwrap()
    }

    fn issue_ref() -> IssueRef {
        IssueRef::new(
            TrackerBackend::GithubProjectV2,
            RepoId::new("github.com", "Alive24", "shea-symphony"),
            501,
        )
    }

    fn request(
        expected_tracker_state: Option<CoordinatorTrackerState>,
        expected_tracker_revision: Option<&str>,
    ) -> Result<CoordinatorActivationRequest, CoordinatorActivationError> {
        CoordinatorActivationRequest::new(
            issue_ref(),
            expected_tracker_state,
            expected_tracker_revision.map(str::to_owned),
            episode_time(),
            CoordinatorSourceKind::Tracker,
            "project-item:PVTI_501",
            "  Activate the observed tracker revision.  ",
        )
    }

    fn executable(decision: CoordinatorActivationDecision) -> CoordinatorExecutableActivation {
        let CoordinatorActivationDecision::Executable(activation) = decision else {
            panic!("expected executable activation");
        };
        activation
    }

    #[test]
    fn every_tracker_state_has_one_explicit_activation_classification() {
        let expected = [
            (
                CoordinatorTrackerState::Todo,
                Some(CoordinatorTargetKind::Work),
            ),
            (
                CoordinatorTrackerState::InProgress,
                Some(CoordinatorTargetKind::Work),
            ),
            (
                CoordinatorTrackerState::AgentReview,
                Some(CoordinatorTargetKind::Review),
            ),
            (
                CoordinatorTrackerState::Rework,
                Some(CoordinatorTargetKind::Rework),
            ),
            (
                CoordinatorTrackerState::Merging,
                Some(CoordinatorTargetKind::Merge),
            ),
            (CoordinatorTrackerState::Backlog, None),
            (CoordinatorTrackerState::NeedToClarify, None),
            (CoordinatorTrackerState::NeedHumanInput, None),
            (CoordinatorTrackerState::HumanReview, None),
            (CoordinatorTrackerState::Done, None),
        ];

        assert_eq!(ALL_STATES.len(), expected.len());
        for (state, expected_target) in expected {
            let observed = ObservedTrackerSnapshot::new(state, "rev-1").unwrap();
            let decision = request(None, None).unwrap().evaluate(&observed).unwrap();
            match (decision, expected_target) {
                (CoordinatorActivationDecision::Executable(activation), Some(target_kind)) => {
                    assert_eq!(activation.target_kind(), target_kind)
                }
                (CoordinatorActivationDecision::Static { observed, .. }, None) => {
                    assert_eq!(observed.state(), state);
                }
                (decision, target) => {
                    panic!("unexpected classification for {state:?}: {decision:?}, {target:?}")
                }
            }
        }
    }

    #[test]
    fn enum_identity_spellings_are_lowercase_kebab_case() {
        let state_spellings = ALL_STATES.map(CoordinatorTrackerState::as_str);
        assert_eq!(
            state_spellings,
            [
                "todo",
                "in-progress",
                "agent-review",
                "rework",
                "merging",
                "backlog",
                "need-to-clarify",
                "need-human-input",
                "human-review",
                "done",
            ]
        );
        assert_eq!(
            [
                CoordinatorTargetKind::Work,
                CoordinatorTargetKind::Review,
                CoordinatorTargetKind::Rework,
                CoordinatorTargetKind::Merge,
            ]
            .map(CoordinatorTargetKind::as_str),
            ["work", "review", "rework", "merge"]
        );
        assert_eq!(
            [
                CoordinatorSourceKind::Tracker,
                CoordinatorSourceKind::OperatorAction,
                CoordinatorSourceKind::Doctor,
                CoordinatorSourceKind::Reconciliation,
            ]
            .map(CoordinatorSourceKind::as_str),
            ["tracker", "operator-action", "doctor", "reconciliation"]
        );
        assert_eq!(
            serde_json::to_string(&CoordinatorTrackerState::NeedHumanInput).unwrap(),
            r#""need-human-input""#
        );
        assert_eq!(
            serde_json::to_string(&CoordinatorTargetKind::Review).unwrap(),
            r#""review""#
        );
        assert_eq!(
            serde_json::to_string(&CoordinatorSourceKind::OperatorAction).unwrap(),
            r#""operator-action""#
        );
    }

    #[test]
    fn normal_github_identity_is_readable_and_target_is_coordinator_derived() {
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Todo, "rev-501").unwrap();
        let activation = executable(request(None, None).unwrap().evaluate(&observed).unwrap());

        assert_eq!(activation.target_kind(), CoordinatorTargetKind::Work);
        assert_eq!(
            activation.workflow_id().as_str(),
            "issue:github.com/Alive24/shea-symphony:501:pulse:todo-to-work:20260725T152000Z:tracker-project-item%3APVTI_501"
        );
        assert_eq!(
            activation.audit_reason(),
            "Activate the observed tracker revision."
        );
        assert!(!activation
            .workflow_id()
            .as_str()
            .contains(activation.audit_reason()));
    }

    #[test]
    fn component_encoding_round_trips_case_unicode_spaces_and_reserved_bytes() {
        let raw = "MiXeD/雪 space:100%?#[]@!$&'()*+,;=";
        let encoded = encode_component(raw);

        assert_eq!(
            encoded,
            "MiXeD%2F%E9%9B%AA%20space%3A100%25%3F%23%5B%5D%40%21%24%26%27%28%29%2A%2B%2C%3B%3D"
        );
        assert_eq!(String::from_utf8(decode_component(&encoded)).unwrap(), raw);
    }

    #[test]
    fn repository_and_source_components_encode_without_separator_loss() {
        let issue = IssueRef::new(
            TrackerBackend::GithubProjectV2,
            RepoId::new("Git Hub:内部", "Alive/24", "shea%symphony"),
            7,
        );
        let request = CoordinatorActivationRequest::new(
            issue,
            None,
            None,
            episode_time(),
            CoordinatorSourceKind::OperatorAction,
            " Fix / PR:百分比 100% ",
            "Operator requested activation.",
        )
        .unwrap();
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Merging, "rev-7").unwrap();
        let activation = executable(request.evaluate(&observed).unwrap());

        assert_eq!(
            activation.workflow_id().as_str(),
            "issue:Git%20Hub%3A%E5%86%85%E9%83%A8/Alive%2F24/shea%25symphony:7:pulse:merging-to-merge:20260725T152000Z:operator-action-%20Fix%20%2F%20PR%3A%E7%99%BE%E5%88%86%E6%AF%94%20100%25%20"
        );
    }

    #[test]
    fn stale_expectations_take_precedence_and_never_create_identity() {
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Todo, "rev-new").unwrap();
        let decision = request(Some(CoordinatorTrackerState::InProgress), Some("rev-old"))
            .unwrap()
            .evaluate(&observed)
            .unwrap();

        let CoordinatorActivationDecision::StaleExpectation {
            expected_tracker_state,
            expected_tracker_revision,
            observed,
            ..
        } = decision
        else {
            panic!("expected a stale expectation");
        };
        assert_eq!(
            expected_tracker_state,
            Some(CoordinatorTrackerState::InProgress)
        );
        assert_eq!(expected_tracker_revision.as_deref(), Some("rev-old"));
        assert_eq!(observed.revision(), "rev-new");
    }

    #[test]
    fn matching_expectations_allow_evaluation() {
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::AgentReview, "rev-1").unwrap();
        let decision = request(Some(CoordinatorTrackerState::AgentReview), Some("rev-1"))
            .unwrap()
            .evaluate(&observed)
            .unwrap();

        assert_eq!(
            executable(decision).target_kind(),
            CoordinatorTargetKind::Review
        );
    }

    #[test]
    fn identity_is_retry_stable_and_changes_with_episode_or_source_identity() {
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Rework, "rev-1").unwrap();
        let first_request = request(None, None).unwrap();
        let retry_request = first_request.clone();

        let first = executable(first_request.evaluate(&observed).unwrap());
        let retry = executable(retry_request.evaluate(&observed).unwrap());
        assert_eq!(first.workflow_id(), retry.workflow_id());

        let later_request = CoordinatorActivationRequest::new(
            issue_ref(),
            None,
            None,
            episode_time() + Duration::SECOND,
            CoordinatorSourceKind::Tracker,
            "project-item:PVTI_501",
            "Activate later episode.",
        )
        .unwrap();
        let later = executable(later_request.evaluate(&observed).unwrap());
        assert_ne!(first.workflow_id(), later.workflow_id());

        let other_source_request = CoordinatorActivationRequest::new(
            issue_ref(),
            None,
            None,
            episode_time(),
            CoordinatorSourceKind::Doctor,
            "doctor-run:20260725",
            "Doctor created executable tracker state.",
        )
        .unwrap();
        let other_source = executable(other_source_request.evaluate(&observed).unwrap());
        assert_ne!(first.workflow_id(), other_source.workflow_id());
    }

    #[test]
    fn required_identity_components_and_issue_number_are_validated() {
        for (component, repo) in [
            (
                RepositoryComponent::Host,
                RepoId::new("", "Alive24", "shea-symphony"),
            ),
            (
                RepositoryComponent::Owner,
                RepoId::new("github.com", "", "shea-symphony"),
            ),
            (
                RepositoryComponent::Repository,
                RepoId::new("github.com", "Alive24", ""),
            ),
            (
                RepositoryComponent::Repository,
                RepoId::new("github.com", "Alive24", " \t"),
            ),
        ] {
            let issue = IssueRef::new(TrackerBackend::GithubProjectV2, repo, 501);
            let error = CoordinatorActivationRequest::new(
                issue,
                None,
                None,
                episode_time(),
                CoordinatorSourceKind::Tracker,
                "source",
                "reason",
            )
            .unwrap_err();
            assert_eq!(
                error,
                CoordinatorActivationError::EmptyRepositoryComponent { component }
            );
        }

        let issue = IssueRef::new(
            TrackerBackend::GithubProjectV2,
            RepoId::new("github.com", "Alive24", "shea-symphony"),
            0,
        );
        assert_eq!(
            CoordinatorActivationRequest::new(
                issue,
                None,
                None,
                episode_time(),
                CoordinatorSourceKind::Tracker,
                "source",
                "reason",
            )
            .unwrap_err(),
            CoordinatorActivationError::ZeroIssueNumber
        );
    }

    #[test]
    fn episode_time_must_be_explicit_utc_at_second_precision() {
        let non_utc = episode_time().to_offset(UtcOffset::from_hms(1, 0, 0).unwrap());
        assert_eq!(
            CoordinatorActivationRequest::new(
                issue_ref(),
                None,
                None,
                non_utc,
                CoordinatorSourceKind::Tracker,
                "source",
                "reason",
            )
            .unwrap_err(),
            CoordinatorActivationError::EpisodeTimeNotUtc
        );

        let subsecond = episode_time() + Duration::milliseconds(1);
        assert_eq!(
            CoordinatorActivationRequest::new(
                issue_ref(),
                None,
                None,
                subsecond,
                CoordinatorSourceKind::Tracker,
                "source",
                "reason",
            )
            .unwrap_err(),
            CoordinatorActivationError::EpisodeTimeNotSecondPrecision
        );

        let negative_year = OffsetDateTime::new_utc(
            time::Date::from_calendar_date(-1, Month::January, 1).unwrap(),
            time::Time::MIDNIGHT,
        );
        assert_eq!(
            CoordinatorActivationRequest::new(
                issue_ref(),
                None,
                None,
                negative_year,
                CoordinatorSourceKind::Tracker,
                "source",
                "reason",
            )
            .unwrap_err(),
            CoordinatorActivationError::EpisodeYearOutOfRange { year: -1 }
        );
    }

    #[test]
    fn source_revision_and_audit_reason_validation_is_typed() {
        for revision in ["", " \n"] {
            assert_eq!(
                ObservedTrackerSnapshot::new(CoordinatorTrackerState::Todo, revision).unwrap_err(),
                CoordinatorActivationError::EmptyTrackerRevision {
                    kind: TrackerRevisionKind::Observed
                }
            );
            assert_eq!(
                request(None, Some(revision)).unwrap_err(),
                CoordinatorActivationError::EmptyTrackerRevision {
                    kind: TrackerRevisionKind::Expected
                }
            );
        }

        for reason in ["", " \n\t "] {
            assert_eq!(
                CoordinatorActivationRequest::new(
                    issue_ref(),
                    None,
                    None,
                    episode_time(),
                    CoordinatorSourceKind::Tracker,
                    "source",
                    reason,
                )
                .unwrap_err(),
                CoordinatorActivationError::EmptyAuditReason
            );
        }

        let oversized = "é".repeat(257);
        assert_eq!(
            CoordinatorActivationRequest::new(
                issue_ref(),
                None,
                None,
                episode_time(),
                CoordinatorSourceKind::Tracker,
                "source",
                oversized,
            )
            .unwrap_err(),
            CoordinatorActivationError::AuditReasonTooLong {
                actual_bytes: 514,
                max_bytes: MAX_AUDIT_REASON_BYTES,
            }
        );

        for source_ref in ["", " \t"] {
            assert_eq!(
                CoordinatorActivationRequest::new(
                    issue_ref(),
                    None,
                    None,
                    episode_time(),
                    CoordinatorSourceKind::Tracker,
                    source_ref,
                    "reason",
                )
                .unwrap_err(),
                CoordinatorActivationError::EmptySourceRef
            );
        }
    }

    #[test]
    fn complete_encoded_workflow_id_rejects_overflow_without_fallback_identity() {
        let observed =
            ObservedTrackerSnapshot::new(CoordinatorTrackerState::Todo, "rev-1").unwrap();
        let one_byte_source = CoordinatorActivationRequest::new(
            issue_ref(),
            None,
            None,
            episode_time(),
            CoordinatorSourceKind::Tracker,
            "x",
            "Boundary source identity.",
        )
        .unwrap();
        let one_byte_id = executable(one_byte_source.evaluate(&observed).unwrap());
        let fixed_bytes = one_byte_id.workflow_id().as_str().len() - 1;
        let maximum_source = "x".repeat(MAX_WORKFLOW_ID_BYTES - fixed_bytes);
        let maximum_request = CoordinatorActivationRequest::new(
            issue_ref(),
            None,
            None,
            episode_time(),
            CoordinatorSourceKind::Tracker,
            maximum_source,
            "Boundary source identity.",
        )
        .unwrap();
        let maximum = executable(maximum_request.evaluate(&observed).unwrap());
        assert_eq!(maximum.workflow_id().as_str().len(), MAX_WORKFLOW_ID_BYTES);

        let request = CoordinatorActivationRequest::new(
            issue_ref(),
            None,
            None,
            episode_time(),
            CoordinatorSourceKind::Tracker,
            "雪".repeat(40),
            "Long source identity.",
        )
        .unwrap();

        let error = request.evaluate(&observed).unwrap_err();
        let CoordinatorActivationError::WorkflowIdTooLong {
            actual_bytes,
            max_bytes,
        } = error
        else {
            panic!("expected Workflow ID overflow");
        };
        assert!(actual_bytes > MAX_WORKFLOW_ID_BYTES);
        assert_eq!(max_bytes, MAX_WORKFLOW_ID_BYTES);
    }

    fn decode_component(encoded: &str) -> Vec<u8> {
        let bytes = encoded.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                decoded.push((hex_value(bytes[index + 1]) << 4) | hex_value(bytes[index + 2]));
                index += 3;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        decoded
    }

    fn hex_value(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'F' => byte - b'A' + 10,
            _ => panic!("invalid test encoding"),
        }
    }
}
