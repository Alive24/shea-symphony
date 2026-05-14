use std::process::{Command, Output};

const LIVE_SMOKE_ENV: &str = "JADE_LIVE_GITHUB_SMOKE";
const WORKFLOW: &str = "workflows/jade-symphony.md";

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

fn require_live_smoke() -> bool {
    if !live_smoke_enabled() {
        eprintln!("skipping live GitHub smoke; set {LIVE_SMOKE_ENV}=1 to enable");
        return false;
    }

    let output = Command::new("gh")
        .args(["auth", "status"])
        .current_dir(repo_root())
        .output()
        .expect("failed to execute `gh auth status`");
    assert!(
        output.status.success(),
        "live smoke requires usable gh auth\n{}",
        combined_output(&output)
    );
    true
}

fn run_jade(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jade-symphony"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("failed to execute jade-symphony binary")
}

#[test]
fn live_github_project_inspect_smoke() {
    if !require_live_smoke() {
        return;
    }

    let output = run_jade(&["inspect", WORKFLOW]);
    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("issues="), "{text}");
    assert!(
        !text.contains("no usable GitHub auth was detected"),
        "inspect should not report missing auth when live smoke is enabled\n{text}"
    );
}

#[test]
fn live_github_dogfood_smoke_dry_run_is_read_only() {
    if !require_live_smoke() {
        return;
    }

    let output = run_jade(&["dogfood-smoke", WORKFLOW, "--dry-run"]);
    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("dogfood_smoke="), "{text}");
    assert!(
        text.contains("dry_run") || text.contains("blocked"),
        "dogfood smoke should remain dry-run/read-only in this smoke\n{text}"
    );
}
