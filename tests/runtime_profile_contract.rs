use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn main_runtime_readiness_precedes_all_claim_status_and_backend_writes() {
    let source = repo_file("src/lanes/main_loop/write_candidate.rs");
    let worktree = source
        .find("let live_worktree = if run_loop_live_handoff_enabled")
        .expect("Main should prepare the live issue worktree");
    let readiness = source
        .find("let readiness = match resolve_runtime_readiness")
        .expect("Main should resolve repository readiness");
    let reread = source
        .find("main_post_readiness_issue_read")
        .expect("Main should reread tracker truth after readiness");
    let claim = source
        .find("let event = match claim_action")
        .expect("Main should retain the guarded claim transition");
    let backend = source
        .find("let mut session_reconciliation =")
        .expect("Main should retain backend/session execution");

    assert!(worktree < readiness);
    assert!(readiness < reread);
    assert!(reread < claim);
    assert!(claim < backend);
    assert!(source.contains("tracker_mutation=false"));
    assert!(source.contains("persist_runtime_readiness_failure"));
}

#[test]
fn manual_main_claim_requires_adopted_workspace_and_readiness_before_write() {
    let source = repo_file("src/commands/session/claim.rs");
    let adoption = source
        .find("discover_issue_workspaces(&config, &issue, &repository_root)")
        .expect("manual Main claim should discover adopted workspace evidence");
    let readiness = source
        .find("match resolve_runtime_readiness")
        .expect("manual Main claim should run readiness");
    let reread = source
        .find("issue disappeared after Main readiness")
        .expect("manual Main claim should reread the issue");
    let claim_write = source
        .find("let outcome = set_project_field_with_recovery")
        .expect("manual lane claim should retain the guarded claim write");
    let registry = claim_write
        + source[claim_write..]
            .find("record_manual_lane_claim_evidence")
            .expect("manual lane claim should retain local registry evidence");

    assert!(adoption < readiness);
    assert!(readiness < reread);
    assert!(reread < claim_write);
    assert!(claim_write < registry);
    assert!(source.contains("blocked before tracker mutation"));
}

#[test]
fn runtime_profile_is_main_only_and_shared_with_handoff_verification() {
    let execution = repo_file("src/lanes/main_loop/execution.rs");
    let handoff = repo_file("src/lanes/main_loop/handoff.rs");
    let session = repo_file("src/commands/session/start.rs");

    assert!(execution.contains("apply_runtime_profile_environment"));
    assert!(handoff.contains("apply_runtime_profile_environment"));
    assert!(handoff.contains("run_workspace_command_with_env"));
    assert!(session.contains("if lane == AgentSessionLaneArg::Main"));
    assert!(
        !repo_file("src/lanes/review/automatic.rs").contains("apply_runtime_profile_environment")
    );
    assert!(!repo_file("src/lanes/merge.rs").contains("apply_runtime_profile_environment"));
}

#[test]
fn app_profile_and_runtime_profile_remain_separate_contracts() {
    let config = repo_file("src/config.rs");
    let docs = repo_file("docs/runtime-profiles.md");
    let gitignore = repo_file(".gitignore");

    assert!(config.contains("pub runtime_profile: RuntimeProfileConfig"));
    assert!(config.contains("root.get(\"runtime_profile\")"));
    assert!(!config.contains("root.get(\"app_profile\")"));
    assert!(docs.contains("`.shea/app-profile.json`"));
    assert!(docs.contains("`.shea/runtime-profile.json`"));
    assert!(gitignore
        .lines()
        .any(|line| line == ".shea/runtime-profile.json"));
}
