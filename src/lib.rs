//! Shea Symphony's reusable runtime and orchestration library.
//!
//! The legacy modules expose the 2606 MVP implementation while
//! [`symphony`] establishes the 2607 Temporal-backed execution boundary. The
//! Symphony API owns deterministic workflow orchestration and delegates all
//! external side effects to Activities; tracker state remains external truth
//! and SQLite remains a rebuildable local read model.

pub mod agent;
pub mod artifacts;
pub mod canonical_checkout;
pub mod codex_app_server;
pub mod config;
pub mod doctor;
pub mod dynamic_tool;
pub mod event_log;
pub mod git_handoff;
pub mod handoff;
pub mod issue_forge;
pub mod issue_workspace;
pub mod lane_claim;
pub mod merge_lane;
pub mod model;
pub mod observability_api;
pub mod orchestrator;
pub mod ownership;
pub mod presentation;
pub mod profiles;
pub mod progress;
pub mod prompt;
pub mod prompt_runtime;
pub mod quality_gate;
pub mod review;
pub mod review_status;
pub mod rework;
pub mod runtime_profile;
pub mod runtime_state;
pub mod session_registry;
pub mod skill_status;
pub mod status_surface;
/// Temporal-backed Symphony workflow, Activity, worker, and local-state contracts.
pub mod symphony;
pub mod target_runtime;
pub mod tracker;
pub mod workflow;
pub mod workpad_templates;
pub mod workspace;

pub use config::RuntimeConfig;
pub use workflow::{WorkflowDefinition, WorkflowStore};
