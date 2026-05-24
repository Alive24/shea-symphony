use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use tempfile::TempDir;

const LIVE_SMOKE_ENV: &str = "SHEA_LIVE_LINEAR_SMOKE";
const PROJECT_SLUG_ENV: &str = "SHEA_LINEAR_PROJECT_SLUG";

fn live_smoke_enabled() -> bool {
    matches!(
        std::env::var(LIVE_SMOKE_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn combined_output(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn require_live_smoke() -> Option<String> {
    if !live_smoke_enabled() {
        eprintln!("skipping live Linear smoke; set {LIVE_SMOKE_ENV}=1 to enable");
        return None;
    }

    assert!(
        std::env::var_os("LINEAR_API_KEY").is_some(),
        "live Linear smoke requires LINEAR_API_KEY"
    );

    let project_slug = std::env::var(PROJECT_SLUG_ENV)
        .unwrap_or_else(|_| panic!("live Linear smoke requires {PROJECT_SLUG_ENV}"));
    Some(project_slug)
}

fn live_workflow(project_slug: &str) -> (TempDir, PathBuf) {
    let tempdir = tempfile::tempdir().expect("failed to create tempdir");
    let workflow_path = tempdir.path().join("linear-live-workflow.md");
    let workspace_root = tempdir.path().join("workspaces");
    let logs_root = tempdir.path().join("logs");
    let workflow = format!(
        r#"---
tracker:
  kind: linear
  project_slug: {project_slug:?}
  api_key: $LINEAR_API_KEY
workspace:
  root: {workspace_root:?}
observability:
  logs_root: {logs_root:?}
---
Live Linear read smoke.
"#
    );
    fs::write(&workflow_path, workflow).expect("failed to write live workflow");
    (tempdir, workflow_path)
}

fn run_shea(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shea-symphony"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("failed to execute shea-symphony binary")
}

#[test]
fn live_linear_project_inspect_smoke() {
    let Some(project_slug) = require_live_smoke() else {
        return;
    };
    let (_tempdir, workflow_path) = live_workflow(&project_slug);
    let workflow_arg = workflow_path.to_string_lossy().to_string();

    let output = run_shea(&["inspect", &workflow_arg]);
    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("issues="), "{text}");
    assert!(
        !text.contains("missing Linear API token"),
        "inspect should not report missing Linear auth when live smoke is enabled\n{text}"
    );
}
