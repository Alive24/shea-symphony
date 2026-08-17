use std::path::{Path, PathBuf};

use shea_symphony::{RuntimeConfig, WorkflowDefinition};

fn repo_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn obsolete_public_fixture_surface_stays_absent() {
    let obsolete_surface = repo_path(&format!("{}{}", "examples", "/"));
    assert!(
        !obsolete_surface.exists(),
        "test evidence must remain under tests/fixtures instead of returning as a public surface"
    );

    let manifest = std::fs::read_to_string(repo_path(".shea/resources.v1.json")).unwrap();
    assert!(
        !manifest.contains("tests/fixtures"),
        "test-owned fixtures must not enter the installable Shea resource manifest"
    );
}

#[test]
fn retained_workflow_fixtures_load_from_the_test_owned_boundary() {
    for relative_path in [
        "tests/fixtures/workflows/claude-main.md",
        "tests/fixtures/workflows/claude-review.md",
        "tests/fixtures/workflows/cockpit-profiles.md",
        "tests/fixtures/workflows/dry-run.md",
        "tests/fixtures/workflows/llm-gate.md",
        "tests/fixtures/workflows/merge-conflict-repair.md",
        "tests/fixtures/workflows/merge.md",
        "tests/fixtures/workflows/promote.md",
        "tests/fixtures/workflows/review.md",
    ] {
        let path = repo_path(relative_path);
        let workflow = WorkflowDefinition::load(&path)
            .unwrap_or_else(|error| panic!("{relative_path} did not load: {error}"));
        let config = RuntimeConfig::from_workflow(&workflow, &path)
            .unwrap_or_else(|error| panic!("{relative_path} did not resolve: {error}"));
        config
            .validate()
            .unwrap_or_else(|error| panic!("{relative_path} did not validate: {error}"));
    }
}
