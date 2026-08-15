use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CANONICAL_SKILLS: &[&str] = &[
    "shea-halo-research-seed",
    "shea-symphony-doctor",
    "shea-symphony-human-review",
    "shea-symphony-issue-forge",
    "shea-symphony-issue-forge-dream",
    "shea-symphony-issue-forge-reflect",
    "shea-symphony-manual-main",
    "shea-symphony-manual-merge",
    "shea-symphony-manual-review",
    "shea-symphony-runtime-onboarding",
];

fn repo_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn repo_file(path: &str) -> String {
    fs::read_to_string(repo_path(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

fn skill_file(skill: &str, path: &str) -> String {
    repo_file(&format!(".agents/skills/{skill}/{path}"))
}

fn frontmatter_value<'a>(source: &'a str, key: &str) -> Option<&'a str> {
    let frontmatter = source.strip_prefix("---\n")?.split_once("\n---\n")?.0;
    frontmatter.lines().find_map(|line| {
        let (candidate, value) = line.split_once(':')?;
        (candidate.trim() == key).then(|| value.trim())
    })
}

fn relative_markdown_links(source: &str) -> Vec<&str> {
    source
        .split("](")
        .skip(1)
        .filter_map(|tail| tail.split_once(')').map(|(target, _)| target))
        .filter(|target| {
            !target.is_empty()
                && !target.starts_with('#')
                && !target.starts_with("http://")
                && !target.starts_with("https://")
        })
        .collect()
}

#[test]
fn agents_skills_is_the_only_complete_first_party_inventory() {
    let root = repo_path(".agents/skills");
    let actual = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    let expected = CANONICAL_SKILLS
        .iter()
        .map(|name| (*name).to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert!(!repo_path("skills/shea-symphony").exists());
    assert!(!repo_path("scripts/install-shea-symphony-skills.js").exists());
    assert!(!repo_path(".shea-example").exists());
}

#[test]
fn canonical_skill_frontmatter_metadata_and_resources_are_structurally_valid() {
    for skill in CANONICAL_SKILLS {
        let root = repo_path(&format!(".agents/skills/{skill}"));
        let source = skill_file(skill, "SKILL.md");

        assert_eq!(frontmatter_value(&source, "name"), Some(*skill));
        assert!(
            frontmatter_value(&source, "description").is_some_and(|value| value.len() >= 40),
            "{skill} needs a useful frontmatter description"
        );
        assert!(
            !source.contains("suite-version:"),
            "{skill} retains suite versioning"
        );
        assert!(
            !source.contains("source-suite-path:"),
            "{skill} retains a managed source-copy path"
        );

        for target in relative_markdown_links(&source) {
            let target = target.split('#').next().unwrap_or(target);
            assert!(
                root.join(target).exists(),
                "{skill} references missing local resource {target}"
            );
        }

        let metadata = root.join("agents/openai.yaml");
        if metadata.exists() {
            let metadata = fs::read_to_string(&metadata).unwrap();
            assert!(
                metadata.contains("display_name:"),
                "{skill} metadata missing display_name"
            );
            assert!(
                metadata.contains("short_description:"),
                "{skill} metadata missing short_description"
            );
            assert!(
                metadata.contains("default_prompt:"),
                "{skill} metadata missing default_prompt"
            );
        }

        for resource_dir in ["references", "fixtures"] {
            let resource_dir = root.join(resource_dir);
            if resource_dir.exists() {
                let files = walk_files(&resource_dir);
                assert!(!files.is_empty(), "{skill}/{resource_dir:?} is empty");
                for file in files {
                    assert!(
                        fs::metadata(&file).unwrap().len() > 0,
                        "empty skill resource: {file:?}"
                    );
                }
            }
        }
    }
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk_files(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn doctor_skill_keeps_repairs_bounded_without_upstream_parity_management() {
    let doctor = skill_file("shea-symphony-doctor", "SKILL.md");
    let reference = skill_file(
        "shea-symphony-doctor",
        "references/repository-contract-repair.md",
    );
    let metadata = skill_file("shea-symphony-doctor", "agents/openai.yaml");

    for marker in [
        "repository_contract_repair",
        "Observed evidence",
        "Doctor inference",
        "missing_completion_invariant",
        "contradictory_instruction",
        "lane_leakage",
        "unsafe_simplification",
        "Show a focused unified diff before writing",
        "complete allowed path set",
        "runtime envelopes and tracker mutation mechanics are not editable contracts",
        "Repository-contract repair itself must not change Project status",
        "Vendored repository skills are owned by that repository",
        "Do not compare them\n  with upstream text or versions",
    ] {
        assert!(doctor.contains(marker), "Doctor skill missing {marker}");
    }
    assert!(!doctor.contains("whole-suite installer"));
    assert!(!doctor.contains("source/rendered-copy synchronization"));
    assert!(reference.contains("## Shea Symphony Contract Repair Plan"));
    assert!(reference.contains("## Shea Symphony Doctor Contract Repair"));
    assert!(metadata.contains("$shea-symphony-doctor"));
}

#[test]
fn doctor_contract_repair_fixtures_cover_safe_refused_and_no_change_results() {
    let cases = [
        (
            "bloated-contradictory.md",
            &[
                "duplicated_instruction",
                "contradictory_instruction",
                "lane_leakage",
            ][..],
            "`proposal`",
        ),
        (
            "implicit-completion.md",
            &["missing_completion_invariant"][..],
            "`proposal`",
        ),
        (
            "safe-simplification.md",
            &["duplicated_instruction", "stale_or_unreachable_text"][..],
            "`proposal`",
        ),
        (
            "unsafe-removal.md",
            &["unsafe_simplification"][..],
            "`refused_unsafe`",
        ),
        ("no-change.md", &["no_change"][..], "`no_change`"),
    ];

    for (name, classifications, disposition) in cases {
        let source = skill_file(
            "shea-symphony-doctor",
            &format!("fixtures/repository-contract-repair/{name}"),
        );
        assert!(source.contains("## Observed evidence"));
        assert!(source.contains("## Expected classification"));
        assert!(source.contains("## Expected disposition"));
        for classification in classifications {
            assert!(
                source.contains(classification),
                "{name} missing {classification}"
            );
        }
        assert!(source.contains(disposition), "{name} missing {disposition}");
    }
}

#[test]
fn lane_skills_preserve_review_workspace_and_subissue_boundaries() {
    let forge = skill_file("shea-symphony-issue-forge", "SKILL.md");
    let reflect = skill_file("shea-symphony-issue-forge-reflect", "SKILL.md");
    let manual_main = skill_file("shea-symphony-manual-main", "SKILL.md");
    let manual_review = skill_file("shea-symphony-manual-review", "SKILL.md");
    let manual_merge = skill_file("shea-symphony-manual-merge", "SKILL.md");

    assert!(forge.contains("the parent owns final Human Review and UAT"));
    assert!(forge.contains("Record a Subissue Human Review Exception"));
    assert!(reflect.contains("ordinary children pass Agent Review to Merging"));
    assert!(manual_review.contains("routes to `Merging`, not `Human Review`"));
    assert!(manual_merge.contains("Do not route native subissue merge repair to `Rework`"));
    assert!(manual_main.contains("Execute one operator-selected Main issue in the current task"));
    assert!(manual_main.contains("Do not create another task"));
    assert!(manual_main.contains("workspace adopt"));
    assert!(manual_main.contains("source=github_native"));
    assert!(manual_main.contains("source=fallback_diagnostic"));
}

#[test]
fn human_review_contract_and_templates_support_operator_owned_decisions() {
    let skill = skill_file("shea-symphony-human-review", "SKILL.md");
    let template = repo_file(".shea/template/workpad/human-review.md");
    let brief = repo_file(".shea/template/workpad/parent-batch-human-review-brief.md");
    let handoff = repo_file(".shea/prompts/human-review-handoff.md");

    for field in [
        "**Problem**",
        "**Delivered change**",
        "**Resulting effect**",
        "**Evidence**",
        "**Human decision needed**",
    ] {
        assert!(skill.contains(field), "missing visible brief field {field}");
    }
    assert!(skill.contains("Accepted Human Review routes to `Merging`"));
    assert!(skill.contains("Never mutate Project state until the operator explicitly confirms"));
    assert!(template.contains("Approve for Merging"));
    assert!(template.contains("Request Rework"));
    assert!(template.contains("Need Human Input"));
    assert!(template.contains("Defer"));
    assert!(brief.contains("This brief is read-only and advisory"));
    assert!(brief.contains("does not prove parent acceptance"));
    assert!(handoff.contains("sole authoritative Human Review contract"));
}

#[test]
fn autoloop_dogfood_docs_and_lane_skills_prefer_the_foreground_loop() {
    let command_reference = repo_file("docs/cli-command-reference.md");
    let operator_dogfood = repo_file("docs/operator-dogfood.md");
    let supervised_runbook = repo_file("docs/supervised-live-dogfood.md");
    let manual_review = skill_file("shea-symphony-manual-review", "SKILL.md");
    let manual_merge = skill_file("shea-symphony-manual-merge", "SKILL.md");

    for document in [
        &command_reference,
        &operator_dogfood,
        &supervised_runbook,
        &manual_review,
        &manual_merge,
    ] {
        assert!(document.contains("autopilot plan"));
        assert!(document.contains("autopilot loop"));
    }
    assert!(command_reference.contains("not a daemon"));
    assert!(operator_dogfood.contains("one operator-controlled supervisor over independent"));
    assert!(supervised_runbook.contains("not a daemon"));
}
