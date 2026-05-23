use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn skill_suite_lists_human_review_skill() {
    let manifest = repo_file("skills/jade-symphony/manifest.toml");
    let readme = repo_file("skills/jade-symphony/README.md");
    let skill = repo_file("skills/jade-symphony/suite/jade-symphony-human-review/SKILL.md");

    assert!(manifest.contains("name = \"jade-symphony-human-review\""));
    assert!(manifest.contains("path = \"suite/jade-symphony-human-review\""));
    assert!(readme.contains("`jade-symphony-human-review`"));
    assert!(skill.contains("Accepted Human Review routes to `Merging`"));
    assert!(skill.contains("Never mutate Project state until the operator explicitly confirms"));
}

#[test]
fn human_review_template_supports_all_decisions() {
    let template = repo_file("workflows/template/workpad/human-review.md");

    assert!(!template.contains("<!-- jade-symphony-workpad -->"));
    assert!(template.contains("## Jade Symphony Human Review Decision"));
    assert!(template.contains("Approve for Merging"));
    assert!(template.contains("Decision timestamp: <YYYY-MM-DD HH:MM timezone>"));
    assert!(template.contains("Request Rework"));
    assert!(template.contains("Need Human Input"));
    assert!(template.contains("Defer"));
    assert!(template.contains("Target state after explicit confirmation"));
}

#[test]
fn autopilot_dogfood_docs_prefer_foreground_loop() {
    let command_reference = repo_file("docs/cli-command-reference.md");
    let operator_dogfood = repo_file("docs/operator-dogfood.md");
    let supervised_runbook = repo_file("docs/supervised-live-dogfood.md");
    let launcher = repo_file("scripts/jade-dogfood");
    let suite_readme = repo_file("skills/jade-symphony/README.md");
    let manual_main = repo_file("skills/jade-symphony/suite/jade-symphony-manual-main/SKILL.md");
    let manual_review =
        repo_file("skills/jade-symphony/suite/jade-symphony-manual-review/SKILL.md");
    let manual_merge = repo_file("skills/jade-symphony/suite/jade-symphony-manual-merge/SKILL.md");

    for document in [
        &command_reference,
        &operator_dogfood,
        &supervised_runbook,
        &suite_readme,
        &manual_main,
        &manual_review,
        &manual_merge,
    ] {
        assert!(document.contains("autopilot plan"));
        assert!(document.contains("autopilot loop"));
    }

    assert!(command_reference.contains("not a daemon"));
    assert!(operator_dogfood.contains("not a daemon"));
    assert!(supervised_runbook.contains("not a daemon"));
    assert!(suite_readme.contains("not a daemon"));
    assert!(suite_readme.contains("app-server"));
    assert!(command_reference.contains("debugging"));
    assert!(suite_readme.contains("focused debugging"));
    assert!(launcher.contains("autopilot plan"));
    assert!(launcher.contains("autopilot loop"));
    assert!(!launcher.contains("run-loop"));
    assert!(!launcher.contains("project-state"));
}
