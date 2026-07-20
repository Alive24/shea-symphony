use std::process::{Command, Output};

const LIVE_SMOKE_ENV: &str = "SHEA_LIVE_GITHUB_SMOKE";
const WORKFLOW: &str = ".shea/workflows/shea-symphony.md";

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

fn run_shea(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shea-symphony"))
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("failed to execute shea-symphony binary")
}

#[test]
fn live_github_project_inspect_smoke() {
    if !require_live_smoke() {
        return;
    }

    let output = run_shea(&["inspect", WORKFLOW]);
    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("issues="), "{text}");
    assert!(
        !text.contains("no usable GitHub auth was detected"),
        "inspect should not report missing auth when live smoke is enabled\n{text}"
    );
}

#[test]
fn live_github_debug_report_is_read_only() {
    if !require_live_smoke() {
        return;
    }

    let output = run_shea(&["debug", WORKFLOW]);
    let text = combined_output(&output);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("Shea Symphony Debug Report"), "{text}");
    assert!(text.contains("read_only=true"), "{text}");
    assert!(text.contains("Smoke Readiness"), "{text}");
}
