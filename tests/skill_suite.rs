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

    assert!(template.starts_with("<!-- jade-symphony-workpad -->"));
    assert!(template.contains("## Human Review Decision Note"));
    assert!(template.contains("Approve for Merging"));
    assert!(template.contains("Decision timestamp: <YYYY-MM-DD HH:MM timezone>"));
    assert!(template.contains("Request Rework"));
    assert!(template.contains("Need Human Input"));
    assert!(template.contains("Defer"));
    assert!(template.contains("Target state after explicit confirmation"));
}
