use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::DEFAULT_WORKFLOW_PATH;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoloopOptions {
    pub workflow_path: Option<String>,
    pub max_iterations: Option<usize>,
    pub once: Option<bool>,
    pub continuous: Option<bool>,
    pub write: Option<bool>,
    pub signal_format: Option<String>,
    pub poll_interval_ms: Option<u64>,
    pub main_max_concurrent: Option<usize>,
    pub review_max_concurrent: Option<usize>,
    pub merge_max_concurrent: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoopStateSnapshot {
    pub running: bool,
    pub stopping: bool,
    pub pid: Option<u32>,
    pub mode: String,
    pub workflow_path: String,
    pub started_at_ms: Option<u128>,
    pub stopped_at_ms: Option<u128>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub lanes: BTreeMap<String, LaneSnapshot>,
    pub recent_lines: Vec<AutoloopLine>,
}

impl Default for LoopStateSnapshot {
    fn default() -> Self {
        Self {
            running: false,
            stopping: false,
            pid: None,
            mode: "dry-run".into(),
            workflow_path: DEFAULT_WORKFLOW_PATH.into(),
            started_at_ms: None,
            stopped_at_ms: None,
            exit_code: None,
            error: None,
            lanes: default_lanes(),
            recent_lines: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneSnapshot {
    pub lane: String,
    pub status: String,
    pub action: Option<String>,
    pub selected: Option<String>,
    pub target: Option<String>,
    pub work_unit_completed: Option<bool>,
    pub completed_work_units: Option<usize>,
    pub issue_ref: Option<String>,
    pub latest_result: Option<String>,
    pub max_concurrent: Option<usize>,
    pub running_count: Option<usize>,
    pub queued_count: Option<usize>,
    pub blocked_count: Option<usize>,
    pub idle_count: Option<usize>,
    pub completed_count: Option<usize>,
    pub recover: Option<bool>,
    pub updated_at_ms: Option<u128>,
    pub latest_line: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoloopLine {
    pub stream: String,
    pub line: String,
    pub at_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoloopStarted {
    pub pid: u32,
    pub command: Vec<String>,
    pub mode: String,
    pub workflow_path: String,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoloopStopped {
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoloopError {
    pub message: String,
    pub at_ms: u128,
}

#[derive(Debug, Default)]
pub struct LoopRuntime {
    pub state: LoopStateSnapshot,
}

#[derive(Clone, Default)]
pub struct LoopManager {
    pub inner: Arc<Mutex<LoopRuntime>>,
}

pub fn default_lanes() -> BTreeMap<String, LaneSnapshot> {
    ["main", "review", "merge"]
        .into_iter()
        .map(|lane| (lane.to_string(), default_lane(lane)))
        .collect()
}

pub fn default_lane(lane: &str) -> LaneSnapshot {
    LaneSnapshot {
        lane: lane.to_string(),
        status: "idle".into(),
        action: None,
        selected: None,
        target: None,
        work_unit_completed: None,
        completed_work_units: None,
        issue_ref: None,
        latest_result: None,
        max_concurrent: None,
        running_count: None,
        queued_count: None,
        blocked_count: None,
        idle_count: Some(1),
        completed_count: None,
        recover: None,
        updated_at_ms: None,
        latest_line: None,
    }
}
