//! Serialized contracts exchanged with Temporal Workflows and Activities.
//!
//! Workflow inputs, state, Activity payloads, and terminal results can be
//! persisted in Temporal history and replayed long after the originating process
//! exits. Keep these DTOs backward-compatible and intentionally small. Large
//! transcripts, diffs, tracker payloads, and report bodies belong in referenced
//! artifacts or rebuildable local projections, not in history.

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Durable input used to start one executable Issue Workflow episode.
///
/// Every field may be recorded in Temporal history. Add fields only when replay
/// and orchestration require the value; prefer stable references over embedded
/// issue bodies, comments, logs, transcripts, or diffs.
pub struct IssueWorkflowInput {
    /// Stable, application-assigned Temporal Workflow ID for this execution.
    pub workflow_id: String,
    /// Stable repository identity, normally host/owner/repository derived.
    pub repo_id: String,
    /// Tracker-native issue reference such as `#477`.
    pub issue_ref: String,
    /// Authoritative tracker state that made this execution eligible to start.
    pub from_tracker_state: String,
    /// Requested orchestration target, such as contract check or agent work.
    pub target_kind: String,
    /// Reference to the tracker transition or operator action that activated work.
    pub source_ref: String,
    /// Tracker revision observed when the start decision was made.
    pub source_tracker_revision: String,
    /// Human-readable UTC timestamp captured outside deterministic Workflow code.
    pub started_at: String,
    /// Optional durable reference to the operator action that requested execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_action_ref: Option<String>,
    /// Optional reference to the capacity policy used when admitting the work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_policy_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Replay-derived state owned by an Issue Workflow execution.
///
/// This is authoritative for the execution's orchestration decisions, but not
/// for external tracker state. Artifact bodies remain outside this DTO and are
/// represented only by stable references.
pub struct IssueWorkflowState {
    /// Stable application-assigned Workflow ID.
    pub workflow_id: String,
    /// Temporal Run ID when known to the Workflow state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Stable repository identity associated with the issue.
    pub repo_id: String,
    /// Tracker-native issue reference.
    pub issue_ref: String,
    /// Last tracker state accepted by the Workflow's ordered decision logic.
    pub current_tracker_state: String,
    /// Current deterministic orchestration step.
    pub active_step: String,
    /// Terminal outcome once the execution has completed or failed permanently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    /// Stable references to external artifacts; never embedded artifact bodies.
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    /// Compact operator-facing health summary derived by the Workflow.
    pub runtime_health_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Read-only projection returned by the Issue Workflow state Query.
///
/// A query result is an observation of replay-derived Workflow state. It cannot
/// authorize tracker writes or imply that an external side effect succeeded.
pub struct IssueWorkflowQueryResult {
    /// Stable application-assigned Workflow ID.
    pub workflow_id: String,
    /// Temporal Run ID when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Tracker-native issue reference.
    pub issue_ref: String,
    /// Last tracker state accepted by Workflow decision logic.
    pub current_tracker_state: String,
    /// Current deterministic orchestration step.
    pub active_step: String,
    /// Terminal outcome when the execution has reached one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_outcome: Option<String>,
    /// Compact operator-facing runtime health summary.
    pub runtime_health_summary: String,
    /// Stable references to external artifacts relevant to the current state.
    #[serde(default)]
    pub artifact_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Internal payload used to prove durable Activity scheduling and routing.
pub struct NoopActivityRequest {
    /// Workflow that scheduled the Activity.
    pub(crate) workflow_id: String,
    /// Durable Activity type name being exercised.
    pub(crate) activity_kind: String,
    /// Tracker issue associated with the Activity.
    pub(crate) issue_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Internal compact result returned by placeholder Activities.
pub struct NoopActivityResult {
    /// Machine-readable placeholder outcome.
    pub(crate) outcome: String,
    /// Short operator-readable description; never a transcript or report body.
    pub(crate) summary: String,
    /// Stable references to external artifacts produced by an Activity.
    #[serde(default)]
    pub(crate) artifact_refs: Vec<String>,
}

const MAX_COMPACT_TRANSITION_VALUE_BYTES: usize = 256;
const MAX_TRANSITION_EVIDENCE_REFS: usize = 8;
const MAX_TRANSITION_IDEMPOTENCY_KEY_BYTES: usize = 4_096;
const TRANSITION_IDEMPOTENCY_KEY_VERSION: &str = "symphony.transition.v1";

/// Validation failure for the compact TrackerTransitionActivity contract.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum TrackerTransitionContractError {
    /// A compact contract value was missing.
    #[error("{field} must not be empty")]
    EmptyValue {
        /// Semantic name of the rejected field.
        field: &'static str,
    },
    /// A compact contract value exceeded its fixed byte budget.
    #[error("{field} must not exceed {max_bytes} bytes")]
    ValueTooLong {
        /// Semantic name of the rejected field.
        field: &'static str,
        /// Maximum serialized UTF-8 byte length.
        max_bytes: usize,
    },
    /// A compact contract value contained a control character.
    #[error("{field} must not contain control characters")]
    ControlCharacter {
        /// Semantic name of the rejected field.
        field: &'static str,
    },
    /// The bounded evidence list contained too many references.
    #[error("evidence_refs supports at most {max_refs} references")]
    TooManyEvidenceRefs {
        /// Maximum permitted number of evidence references.
        max_refs: usize,
    },
    /// A serialized idempotency key did not carry the current format version.
    #[error("idempotency_key must use the {TRANSITION_IDEMPOTENCY_KEY_VERSION} format")]
    UnsupportedIdempotencyKeyVersion,
}

/// Small validated string used by the transition DTO's opaque references.
///
/// The value stays opaque to Symphony. Validation only bounds Temporal history
/// payloads and rejects control characters that could smuggle a log or body
/// into this compact contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct CompactTransitionValue(String);

// These constructors become reachable when a Workflow starts scheduling the
// deliberately unscheduled Activity; #494 retains them as the contract seam.
#[allow(dead_code)]
impl CompactTransitionValue {
    /// Builds one compact opaque value without normalizing its tracker meaning.
    pub(crate) fn new(
        field: &'static str,
        value: impl Into<String>,
    ) -> Result<Self, TrackerTransitionContractError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(TrackerTransitionContractError::EmptyValue { field });
        }
        if value.len() > MAX_COMPACT_TRANSITION_VALUE_BYTES {
            return Err(TrackerTransitionContractError::ValueTooLong {
                field,
                max_bytes: MAX_COMPACT_TRANSITION_VALUE_BYTES,
            });
        }
        if value.chars().any(char::is_control) {
            return Err(TrackerTransitionContractError::ControlCharacter { field });
        }

        Ok(Self(value))
    }

    /// Returns the original opaque value for serialization or stable key input.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    fn known(value: &'static str) -> Self {
        Self(value.to_string())
    }
}

impl<'de> Deserialize<'de> for CompactTransitionValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new("transition value", value).map_err(serde::de::Error::custom)
    }
}

/// Stable Workflow identity used to scope one tracker-transition intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TrackerTransitionWorkflowId(CompactTransitionValue);

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerTransitionWorkflowId {
    /// Builds an opaque Workflow identity without consulting Temporal.
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, TrackerTransitionContractError> {
        CompactTransitionValue::new("workflow_id", value).map(Self)
    }

    /// Returns the stable Workflow identity.
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Fully qualified tracker identity without embedding a full `TrackerIssue`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrackerTransitionIssueRef {
    /// Tracker backend or namespace that resolves this identity.
    pub(crate) tracker_kind: CompactTransitionValue,
    /// Stable tracker-native repository or project scope.
    pub(crate) repository: CompactTransitionValue,
    /// Tracker-native issue identity within the repository or project scope.
    pub(crate) issue: CompactTransitionValue,
}

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerTransitionIssueRef {
    /// Builds a fully qualified reference using only stable tracker identity.
    pub(crate) fn new(
        tracker_kind: impl Into<String>,
        repository: impl Into<String>,
        issue: impl Into<String>,
    ) -> Result<Self, TrackerTransitionContractError> {
        Ok(Self {
            tracker_kind: CompactTransitionValue::new("issue_ref.tracker_kind", tracker_kind)?,
            repository: CompactTransitionValue::new("issue_ref.repository", repository)?,
            issue: CompactTransitionValue::new("issue_ref.issue", issue)?,
        })
    }
}

/// Opaque, validated tracker state string.
///
/// State names remain tracker-owned strings so this first contract does not
/// freeze GitHub Project lane vocabulary into a Symphony enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TrackerState(CompactTransitionValue);

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerState {
    /// Builds a bounded tracker state without interpreting its external value.
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, TrackerTransitionContractError> {
        CompactTransitionValue::new("tracker state", value).map(Self)
    }

    /// Returns the untouched tracker-owned state value.
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Opaque Symphony transition-kind label supplied by the Workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TrackerTransitionKind(CompactTransitionValue);

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerTransitionKind {
    /// Builds a compact transition-kind label without defining a global enum.
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, TrackerTransitionContractError> {
        CompactTransitionValue::new("transition_kind", value).map(Self)
    }

    /// Returns the stable transition-kind label.
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Opaque requester identity for a transition intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct TrackerTransitionRequester(CompactTransitionValue);

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerTransitionRequester {
    /// Builds a compact requester identity without binding it to tracker users.
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, TrackerTransitionContractError> {
        CompactTransitionValue::new("requester", value).map(Self)
    }
}

/// Bounded structured reason supplied with a state-transition intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TrackerTransitionReason {
    /// Stable reason code owned by the requesting Symphony path.
    pub(crate) code: CompactTransitionValue,
    /// Optional concise operator-facing detail, never a transcript or report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<CompactTransitionValue>,
}

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerTransitionReason {
    /// Builds a reason from a stable code and optional bounded detail.
    pub(crate) fn new(
        code: impl Into<String>,
        detail: Option<String>,
    ) -> Result<Self, TrackerTransitionContractError> {
        Ok(Self {
            code: CompactTransitionValue::new("reason.code", code)?,
            detail: detail
                .map(|value| CompactTransitionValue::new("reason.detail", value))
                .transpose()?,
        })
    }
}

/// Small bounded list of external evidence references, without evidence bodies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct TrackerTransitionEvidenceRefs(Vec<CompactTransitionValue>);

// This internal constructor surface is retained for future Workflow callers.
#[allow(dead_code)]
impl TrackerTransitionEvidenceRefs {
    /// Builds a bounded evidence-reference list from compact stable references.
    pub(crate) fn new(refs: Vec<String>) -> Result<Self, TrackerTransitionContractError> {
        let refs = refs
            .into_iter()
            .map(|reference| CompactTransitionValue::new("evidence reference", reference))
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_values(refs)
    }

    /// Returns whether the list can be omitted from a compact serialized DTO.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn from_values(
        refs: Vec<CompactTransitionValue>,
    ) -> Result<Self, TrackerTransitionContractError> {
        if refs.len() > MAX_TRANSITION_EVIDENCE_REFS {
            return Err(TrackerTransitionContractError::TooManyEvidenceRefs {
                max_refs: MAX_TRANSITION_EVIDENCE_REFS,
            });
        }

        Ok(Self(refs))
    }
}

impl<'de> Deserialize<'de> for TrackerTransitionEvidenceRefs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let refs = Vec::<CompactTransitionValue>::deserialize(deserializer)?;
        Self::from_values(refs).map_err(serde::de::Error::custom)
    }
}

/// Versioned retry identity for one intended tracker state transition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub(crate) struct TransitionIdempotencyKey(String);

// The retry-key constructor is intentionally unused until the Activity is
// scheduled; keeping it internal prevents a wider API from leaking early.
#[allow(dead_code)]
impl TransitionIdempotencyKey {
    /// Deterministically derives a key from the immutable intended side effect.
    ///
    /// The Workflow constructs and carries this key before an Activity starts.
    /// Activity retries reuse that carried value, so retry count, wall-clock
    /// time, randomness, and process-local state cannot change write identity.
    pub(crate) fn for_transition(
        workflow_id: &TrackerTransitionWorkflowId,
        issue_ref: &TrackerTransitionIssueRef,
        expected_from_state: &TrackerState,
        requested_to_state: &TrackerState,
        transition_kind: &TrackerTransitionKind,
        attempt_slot: u32,
    ) -> Self {
        let mut key = TRANSITION_IDEMPOTENCY_KEY_VERSION.to_string();
        let attempt_slot = attempt_slot.to_string();

        // Length-prefixing makes the fixed-order format injective even when
        // opaque tracker strings contain delimiter characters such as `:`.
        for ingredient in [
            workflow_id.as_str(),
            issue_ref.tracker_kind.as_str(),
            issue_ref.repository.as_str(),
            issue_ref.issue.as_str(),
            expected_from_state.as_str(),
            requested_to_state.as_str(),
            transition_kind.as_str(),
            attempt_slot.as_str(),
        ] {
            append_length_prefixed_key_component(&mut key, ingredient);
        }

        Self(key)
    }
}

impl<'de> Deserialize<'de> for TransitionIdempotencyKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let version_prefix = format!("{TRANSITION_IDEMPOTENCY_KEY_VERSION}:");
        if !value.starts_with(&version_prefix) {
            return Err(serde::de::Error::custom(
                TrackerTransitionContractError::UnsupportedIdempotencyKeyVersion,
            ));
        }
        if value.len() > MAX_TRANSITION_IDEMPOTENCY_KEY_BYTES {
            return Err(serde::de::Error::custom(
                TrackerTransitionContractError::ValueTooLong {
                    field: "idempotency_key",
                    max_bytes: MAX_TRANSITION_IDEMPOTENCY_KEY_BYTES,
                },
            ));
        }
        if value.chars().any(char::is_control) {
            return Err(serde::de::Error::custom(
                TrackerTransitionContractError::ControlCharacter {
                    field: "idempotency_key",
                },
            ));
        }

        Ok(Self(value))
    }
}

#[allow(dead_code)] // Called by the intentionally unscheduled key constructor above.
fn append_length_prefixed_key_component(output: &mut String, component: &str) {
    output.push(':');
    output.push_str(&component.len().to_string());
    output.push(':');
    output.push_str(component);
}

/// Symphony-owned result class for a state-transition Activity attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerTransitionOutcome {
    /// The requested tracker state was committed and read back by a future writer.
    Committed,
    /// Readback found the requested state already present.
    AlreadyApplied,
    /// Observed tracker facts did not match the requested precondition.
    Conflict,
    /// The requested intent was declined without an infrastructure failure.
    Rejected,
    /// The caller should wait before trying the same intent again.
    RetryLater,
    /// An operator must resolve a prerequisite before the Workflow can continue.
    NeedHumanInput,
    /// Symphony encountered a malformed contract or internal invariant failure.
    UnhandledError,
}

/// Symphony-owned explanation for a transition conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrackerTransitionConflictReason {
    /// An external actor changed the active tracker state.
    ExternalStateChanged,
    /// An external actor moved the issue to a terminal state.
    ExternalTerminalState,
    /// The expected tracker state could not be found.
    ExpectedStateMissing,
    /// The configured tracker project schema no longer supports the request.
    ProjectSchemaChanged,
    /// Required tracker permission or authorization scope is unavailable.
    PermissionOrScopeMissing,
    /// A Symphony-produced transition payload was invalid.
    MalformedRequest,
    /// Readback returned facts that could not be reconciled with the write.
    ReadbackInconsistent,
}

/// Compact, history-safe state-transition intent for `TrackerTransitionActivity`.
///
/// This is `pub` only because the Temporal activity macro emits a public SDK
/// registration interface. The private `symphony::dto` module and absent
/// re-export keep the contract internal to this crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerTransitionRequest {
    /// Stable identity of the Workflow that selected the intended transition.
    pub(crate) workflow_id: TrackerTransitionWorkflowId,
    /// Optional Temporal run reference for correlation, not idempotency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run_id: Option<CompactTransitionValue>,
    /// Fully qualified stable tracker identity; no rich issue payload is embedded.
    pub(crate) issue_ref: TrackerTransitionIssueRef,
    /// Tracker state that must still be observed before a future write can commit.
    pub(crate) expected_from_state: TrackerState,
    /// Opaque tracker state requested by the Workflow.
    pub(crate) requested_to_state: TrackerState,
    /// Stable transition action label supplied by the Workflow.
    pub(crate) transition_kind: TrackerTransitionKind,
    /// Identity of the Symphony path that requested the transition.
    pub(crate) requester: TrackerTransitionRequester,
    /// Bounded structured explanation of the intended state change.
    pub(crate) reason: TrackerTransitionReason,
    /// Optional bounded references to supporting external evidence.
    #[serde(
        default,
        skip_serializing_if = "TrackerTransitionEvidenceRefs::is_empty"
    )]
    pub(crate) evidence_refs: TrackerTransitionEvidenceRefs,
    /// Workflow-selected logical attempt slot, not Temporal's retry counter.
    pub(crate) attempt_slot: u32,
    /// Precomputed stable retry identity carried into every Activity attempt.
    pub(crate) idempotency_key: TransitionIdempotencyKey,
}

// The Activity is registered but unscheduled in this slice, so future callers
// own this constructor without widening the crate's public API prematurely.
#[allow(dead_code)]
impl TrackerTransitionRequest {
    /// Builds a compact transition intent and its deterministic retry identity.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        workflow_id: impl Into<String>,
        run_id: Option<String>,
        issue_ref: TrackerTransitionIssueRef,
        expected_from_state: TrackerState,
        requested_to_state: TrackerState,
        transition_kind: TrackerTransitionKind,
        requester: TrackerTransitionRequester,
        reason: TrackerTransitionReason,
        evidence_refs: TrackerTransitionEvidenceRefs,
        attempt_slot: u32,
    ) -> Result<Self, TrackerTransitionContractError> {
        let workflow_id = TrackerTransitionWorkflowId::new(workflow_id)?;
        let run_id = run_id
            .map(|value| CompactTransitionValue::new("run_id", value))
            .transpose()?;
        let idempotency_key = TransitionIdempotencyKey::for_transition(
            &workflow_id,
            &issue_ref,
            &expected_from_state,
            &requested_to_state,
            &transition_kind,
            attempt_slot,
        );

        Ok(Self {
            workflow_id,
            run_id,
            issue_ref,
            expected_from_state,
            requested_to_state,
            transition_kind,
            requester,
            reason,
            evidence_refs,
            attempt_slot,
            idempotency_key,
        })
    }
}

/// Compact typed result returned by `TrackerTransitionActivity`.
///
/// This is `pub` only because the Temporal activity macro emits a public SDK
/// registration interface. The private `symphony::dto` module and absent
/// re-export keep the contract internal to this crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackerTransitionResult {
    /// Symphony-owned outcome class; Workflow routing remains a separate concern.
    pub(crate) outcome: TrackerTransitionOutcome,
    /// Stable issue identity associated with the attempted transition.
    pub(crate) issue_ref: TrackerTransitionIssueRef,
    /// Tracker state observed by a future writer before it decides an outcome.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) observed_from_state: Option<TrackerState>,
    /// Tracker state confirmed by future write/readback logic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) committed_to_state: Option<TrackerState>,
    /// Typed conflict context when `outcome` is [`TrackerTransitionOutcome::Conflict`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) conflict_reason: Option<TrackerTransitionConflictReason>,
    /// Bounded evidence references returned by a future write/readback slice.
    #[serde(
        default,
        skip_serializing_if = "TrackerTransitionEvidenceRefs::is_empty"
    )]
    pub(crate) evidence_refs: TrackerTransitionEvidenceRefs,
    /// Optional compact audit reference, never an audit body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) audit_ref: Option<CompactTransitionValue>,
    /// Optional provider-directed wait without defining Temporal retry policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) retry_after_ms: Option<u64>,
    /// Bounded operator-readable result summary.
    pub(crate) summary: CompactTransitionValue,
}

impl TrackerTransitionResult {
    /// Returns the explicit zero-side-effect result for the registered placeholder.
    pub(crate) fn not_implemented(request: &TrackerTransitionRequest) -> Self {
        Self {
            outcome: TrackerTransitionOutcome::Rejected,
            issue_ref: request.issue_ref.clone(),
            observed_from_state: None,
            committed_to_state: None,
            conflict_reason: None,
            evidence_refs: TrackerTransitionEvidenceRefs::default(),
            audit_ref: None,
            retry_after_ms: None,
            summary: CompactTransitionValue::known(
                "TrackerTransitionActivity is registered but intentionally inert (not implemented); no tracker mutation was attempted",
            ),
        }
    }
}

impl IssueWorkflowState {
    /// Initializes deterministic Workflow state from a durable start payload.
    pub(crate) fn from_input(input: IssueWorkflowInput, run_id: Option<String>) -> Self {
        Self {
            workflow_id: input.workflow_id,
            run_id,
            repo_id: input.repo_id,
            issue_ref: input.issue_ref,
            current_tracker_state: input.from_tracker_state,
            active_step: format!("noop:{}", input.target_kind),
            terminal_outcome: None,
            artifact_refs: Vec::new(),
            runtime_health_summary: "initialized".to_string(),
        }
    }

    /// Builds the bounded, read-only Query projection of the current state.
    pub(crate) fn query_result(&self) -> IssueWorkflowQueryResult {
        IssueWorkflowQueryResult {
            workflow_id: self.workflow_id.clone(),
            run_id: self.run_id.clone(),
            issue_ref: self.issue_ref.clone(),
            current_tracker_state: self.current_tracker_state.clone(),
            active_step: self.active_step.clone(),
            terminal_outcome: self.terminal_outcome.clone(),
            runtime_health_summary: self.runtime_health_summary.clone(),
            artifact_refs: self.artifact_refs.clone(),
        }
    }
}

impl NoopActivityResult {
    /// Describes a successful no-op without claiming an external write occurred.
    pub(crate) fn success(request: &NoopActivityRequest) -> Self {
        Self {
            outcome: "noop_success".to_string(),
            summary: format!(
                "{} completed without side effects for {}",
                request.activity_kind, request.issue_ref
            ),
            artifact_refs: Vec::new(),
        }
    }

    /// Describes an intentionally inert placeholder Activity.
    pub(crate) fn not_implemented(request: &NoopActivityRequest) -> Self {
        Self {
            outcome: "not_implemented".to_string(),
            summary: format!(
                "{} is registered but intentionally inert for {}",
                request.activity_kind, request.issue_ref
            ),
            artifact_refs: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input() -> IssueWorkflowInput {
        IssueWorkflowInput {
            workflow_id: "issue:shea-symphony:475:pulse:todo-to-work:20260709-175700Z:project"
                .to_string(),
            repo_id: "Alive24/shea-symphony".to_string(),
            issue_ref: "#475".to_string(),
            from_tracker_state: "Todo".to_string(),
            target_kind: "work".to_string(),
            source_ref: "project-v2".to_string(),
            source_tracker_revision: "rev-1".to_string(),
            started_at: "2026-07-09T17:57:00Z".to_string(),
            operator_action_ref: None,
            capacity_policy_ref: Some("default-local".to_string()),
        }
    }

    #[test]
    fn issue_workflow_state_is_small_serializable_dto() {
        let state = IssueWorkflowState::from_input(input(), Some("temporal-run-id".to_string()));
        let value = serde_json::to_value(&state).unwrap();

        assert_eq!(value["issue_ref"], "#475");
        assert_eq!(value["artifact_refs"].as_array().unwrap().len(), 0);
        assert!(value.get("transcript").is_none());
        assert!(value.get("diff").is_none());
        assert!(value.get("tracker_payload").is_none());
    }

    #[test]
    fn query_result_exposes_state_without_repo_payload() {
        let state = IssueWorkflowState::from_input(input(), None);
        let query = state.query_result();

        assert_eq!(query.workflow_id, state.workflow_id);
        assert_eq!(query.issue_ref, "#475");
        assert!(!serde_json::to_value(query)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("repo_id"));
    }

    #[test]
    fn noop_activity_results_carry_summaries_not_artifacts() {
        let request = NoopActivityRequest {
            workflow_id: "workflow-id".to_string(),
            activity_kind: "NoopCoreActivity".to_string(),
            issue_ref: "#475".to_string(),
        };

        let result = NoopActivityResult::success(&request);

        assert_eq!(result.outcome, "noop_success");
        assert!(result.summary.contains("NoopCoreActivity"));
        assert!(result.artifact_refs.is_empty());
    }

    fn transition_request_with(
        workflow_id: &str,
        issue: &str,
        expected_from_state: &str,
        requested_to_state: &str,
        transition_kind: &str,
        attempt_slot: u32,
    ) -> TrackerTransitionRequest {
        TrackerTransitionRequest::new(
            workflow_id,
            Some("temporal-run-494".to_string()),
            TrackerTransitionIssueRef::new(
                "github_project_v2",
                "github.com/Alive24/shea-symphony",
                issue,
            )
            .unwrap(),
            TrackerState::new(expected_from_state).unwrap(),
            TrackerState::new(requested_to_state).unwrap(),
            TrackerTransitionKind::new(transition_kind).unwrap(),
            TrackerTransitionRequester::new("issue_workflow").unwrap(),
            TrackerTransitionReason::new(
                "implementation_complete",
                Some("compact contract test".to_string()),
            )
            .unwrap(),
            TrackerTransitionEvidenceRefs::new(vec!["artifact://run/494/summary".to_string()])
                .unwrap(),
            attempt_slot,
        )
        .unwrap()
    }

    #[test]
    fn tracker_transition_request_serializes_only_compact_contract_fields() {
        let mut request = transition_request_with(
            "issue:shea-symphony:494:pulse:handoff",
            "#494",
            "In Progress",
            "Agent Review",
            "main_handoff",
            0,
        );
        request.run_id = None;
        request.evidence_refs = TrackerTransitionEvidenceRefs::default();

        let value = serde_json::to_value(&request).unwrap();
        let object = value.as_object().unwrap();

        assert_eq!(
            value["workflow_id"],
            "issue:shea-symphony:494:pulse:handoff"
        );
        assert_eq!(
            value["issue_ref"],
            json!({
                "tracker_kind": "github_project_v2",
                "repository": "github.com/Alive24/shea-symphony",
                "issue": "#494",
            })
        );
        assert_eq!(value["expected_from_state"], "In Progress");
        assert_eq!(value["requested_to_state"], "Agent Review");
        assert!(value["idempotency_key"]
            .as_str()
            .unwrap()
            .starts_with("symphony.transition.v1:"));
        assert!(!object.contains_key("run_id"));
        assert!(!object.contains_key("evidence_refs"));
        for forbidden in [
            "tracker_issue",
            "tracker_payload",
            "project_payload",
            "issue_body",
            "comments",
            "logs",
            "transcript",
            "diff",
            "worktree_path",
        ] {
            assert!(
                !object.contains_key(forbidden),
                "{forbidden} must not enter Temporal history"
            );
        }

        let decoded = serde_json::from_value::<TrackerTransitionRequest>(value).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn transition_outcomes_and_conflict_reasons_have_stable_typed_serialization() {
        for (outcome, serialized) in [
            (TrackerTransitionOutcome::Committed, "committed"),
            (TrackerTransitionOutcome::AlreadyApplied, "already_applied"),
            (TrackerTransitionOutcome::Conflict, "conflict"),
            (TrackerTransitionOutcome::Rejected, "rejected"),
            (TrackerTransitionOutcome::RetryLater, "retry_later"),
            (TrackerTransitionOutcome::NeedHumanInput, "need_human_input"),
            (TrackerTransitionOutcome::UnhandledError, "unhandled_error"),
        ] {
            assert_eq!(serde_json::to_value(outcome).unwrap(), json!(serialized));
        }
        for (reason, serialized) in [
            (
                TrackerTransitionConflictReason::ExternalStateChanged,
                "external_state_changed",
            ),
            (
                TrackerTransitionConflictReason::ExternalTerminalState,
                "external_terminal_state",
            ),
            (
                TrackerTransitionConflictReason::ExpectedStateMissing,
                "expected_state_missing",
            ),
            (
                TrackerTransitionConflictReason::ProjectSchemaChanged,
                "project_schema_changed",
            ),
            (
                TrackerTransitionConflictReason::PermissionOrScopeMissing,
                "permission_or_scope_missing",
            ),
            (
                TrackerTransitionConflictReason::MalformedRequest,
                "malformed_request",
            ),
            (
                TrackerTransitionConflictReason::ReadbackInconsistent,
                "readback_inconsistent",
            ),
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), json!(serialized));
        }

        let request = transition_request_with(
            "issue:shea-symphony:494:pulse:handoff",
            "#494",
            "In Progress",
            "Agent Review",
            "main_handoff",
            0,
        );
        let conflict = TrackerTransitionResult {
            outcome: TrackerTransitionOutcome::Conflict,
            issue_ref: request.issue_ref.clone(),
            observed_from_state: Some(TrackerState::new("Rework").unwrap()),
            committed_to_state: None,
            conflict_reason: Some(TrackerTransitionConflictReason::ExternalStateChanged),
            evidence_refs: TrackerTransitionEvidenceRefs::default(),
            audit_ref: None,
            retry_after_ms: None,
            summary: CompactTransitionValue::known("tracker state changed externally"),
        };
        let value = serde_json::to_value(conflict).unwrap();

        assert_eq!(value["outcome"], "conflict");
        assert_eq!(value["conflict_reason"], "external_state_changed");
        assert_eq!(value["observed_from_state"], "Rework");
    }

    #[test]
    fn transition_idempotency_key_is_deterministic_and_distinguishes_each_input() {
        let base = transition_request_with(
            "issue:shea-symphony:494:pulse:handoff",
            "#494",
            "In Progress",
            "Agent Review",
            "main_handoff",
            0,
        );
        let retry = transition_request_with(
            "issue:shea-symphony:494:pulse:handoff",
            "#494",
            "In Progress",
            "Agent Review",
            "main_handoff",
            0,
        );

        assert_eq!(base.idempotency_key, retry.idempotency_key);
        for changed_intent in [
            transition_request_with(
                "issue:shea-symphony:494:pulse:other",
                "#494",
                "In Progress",
                "Agent Review",
                "main_handoff",
                0,
            ),
            transition_request_with(
                "issue:shea-symphony:494:pulse:handoff",
                "#495",
                "In Progress",
                "Agent Review",
                "main_handoff",
                0,
            ),
            transition_request_with(
                "issue:shea-symphony:494:pulse:handoff",
                "#494",
                "Rework",
                "Agent Review",
                "main_handoff",
                0,
            ),
            transition_request_with(
                "issue:shea-symphony:494:pulse:handoff",
                "#494",
                "In Progress",
                "Human Review",
                "main_handoff",
                0,
            ),
            transition_request_with(
                "issue:shea-symphony:494:pulse:handoff",
                "#494",
                "In Progress",
                "Agent Review",
                "human_handoff",
                0,
            ),
            transition_request_with(
                "issue:shea-symphony:494:pulse:handoff",
                "#494",
                "In Progress",
                "Agent Review",
                "main_handoff",
                1,
            ),
        ] {
            assert_ne!(base.idempotency_key, changed_intent.idempotency_key);
        }
    }

    #[test]
    fn transition_idempotency_key_length_prefixes_opaque_delimiters() {
        let delimiter_in_requested_state = transition_request_with(
            "workflow",
            "#494",
            "Todo",
            "In Progress:state",
            "handoff",
            0,
        );
        let delimiter_in_expected_state = transition_request_with(
            "workflow",
            "#494",
            "Todo:In Progress",
            "state",
            "handoff",
            0,
        );

        assert_ne!(
            delimiter_in_requested_state.idempotency_key,
            delimiter_in_expected_state.idempotency_key,
            "length prefixes prevent the collision a delimiter-only key would allow"
        );
    }

    #[test]
    fn tracker_states_and_evidence_references_stay_bounded_and_opaque() {
        let state = TrackerState::new("Provider Managed: Awaiting Review").unwrap();
        assert_eq!(state.as_str(), "Provider Managed: Awaiting Review");
        assert!(TrackerState::new("\n").is_err());
        assert!(serde_json::from_str::<TrackerState>(r#""\n""#).is_err());

        let too_many_refs = (0..=MAX_TRANSITION_EVIDENCE_REFS)
            .map(|index| format!("artifact://run/494/{index}"))
            .collect();
        assert!(TrackerTransitionEvidenceRefs::new(too_many_refs).is_err());
    }
}
