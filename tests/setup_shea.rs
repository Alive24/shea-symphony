use std::path::Path;
use std::process::Command;

fn repo_path(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn setup_shea_controller_fixture_suite_passes() {
    let test = repo_path("skills/shea-symphony/suite/setup-shea/scripts/setup-shea.test.mjs");
    let output = Command::new("node")
        .args(["--test", test.to_str().unwrap()])
        .output()
        .expect("Node.js is required by the standard Skills CLI setup contract");
    assert!(
        output.status.success(),
        "setup-shea fixture suite failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn release_workflow_uses_the_documented_native_target_matrix() {
    let workflow =
        std::fs::read_to_string(repo_path(".github/workflows/release-legacy-cli.yml")).unwrap();
    let packager = std::fs::read_to_string(repo_path("scripts/package-legacy-release.py")).unwrap();
    let manifest =
        std::fs::read_to_string(repo_path("scripts/build-legacy-release-manifest.mjs")).unwrap();

    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
    ] {
        assert!(workflow.contains(target), "workflow missing {target}");
        assert!(
            manifest.contains(target),
            "release manifest missing {target}"
        );
    }
    assert!(workflow.contains("macos-15-intel"));
    assert!(workflow.contains("macos-15"));
    assert!(workflow.contains("ubuntu-24.04"));
    assert!(workflow.contains("SHA256SUMS"));
    assert!(workflow.contains("legacy-release.json"));
    assert!(packager.contains("binary_role"));
    assert!(packager.contains("legacy_cli"));
    assert!(packager.contains("source_revision"));
    assert!(packager.contains("shea-legacy-cli-v1"));
    assert!(!workflow.contains("shea-symphony --runtime-info"));
}
