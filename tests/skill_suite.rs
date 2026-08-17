use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CANONICAL_SKILLS: &[&str] = &[
    "setup-shea",
    "shea-halo-research-seed",
    "shea-backlog",
    "shea-deepen",
    "shea-doctor",
    "shea-human-review",
    "shea-issue-forge",
    "shea-manual-main",
    "shea-manual-merge",
    "shea-agent-review",
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

#[test]
fn installable_manifest_declares_core_and_explicit_optional_groups() {
    let manifest: serde_json::Value =
        serde_json::from_str(&repo_file(".shea/resources.v1.json")).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["core_group"], "core");
    let groups = manifest["groups"].as_object().unwrap();
    assert_eq!(
        groups.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "core".into(),
            "halo_research".into(),
            "deepen".into(),
            "parent_subissues".into(),
            "shea_docs".into(),
        ])
    );
    assert_eq!(groups["core"]["optional"], false);
    assert_eq!(groups["shea_docs"]["available"], false);
    let core = serde_json::to_string(&groups["core"]).unwrap();
    assert!(!core.contains("setup-shea"));
    assert!(!core.contains("shea-deepen"));
    assert!(!core.contains("shea-halo-research-seed"));
    assert!(!core.contains("parent-batch-readiness-report"));
}

#[test]
fn renamed_core_skills_have_no_compatibility_alias_directories() {
    for removed in [
        "shea-symphony-issue-forge",
        "shea-symphony-backlog",
        "shea-symphony-doctor",
        "shea-symphony-human-review",
        "shea-symphony-manual-main",
        "shea-symphony-manual-review",
        "shea-symphony-manual-merge",
        "shea-symphony-improve",
    ] {
        assert!(!repo_path(&format!(".agents/skills/{removed}")).exists());
    }
}

#[test]
fn issue_forge_is_a_short_phase_router_with_complete_guidance() {
    let skill = skill_file("shea-issue-forge", "SKILL.md");
    assert!(skill.lines().count() < 70);
    let links = relative_markdown_links(&skill)
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        links,
        BTreeSet::from([
            "references/contract.md".into(),
            "references/creation.md".into(),
            "references/discussion.md".into(),
            "references/promotion.md".into(),
            "references/rework.md".into(),
            "references/tracker-hygiene.md".into(),
        ])
    );
    let combined = links
        .iter()
        .map(|path| skill_file("shea-issue-forge", path))
        .collect::<Vec<_>>()
        .join("\n");
    for marker in [
        "Issue Quality Gate",
        "Prepare, Confirm, Execute",
        "`issue.create`",
        "`issue.promote`",
        "`issue.rework`",
        "Subissue Human Review Exception",
    ] {
        assert!(
            combined.contains(marker),
            "missing Forge coverage: {marker}"
        );
    }
}

#[test]
fn retained_templates_have_taxonomy_and_identified_consumers() {
    let root = repo_path(".shea/template");
    let audit = repo_file("docs/template-consumer-audit.md");
    let files = walk_files(&root)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 25);
    for path in files {
        let relative = path.strip_prefix(repo_path(".shea")).unwrap();
        let relative = relative.to_string_lossy();
        assert!(
            relative.starts_with("template/workpad/")
                || relative.starts_with("template/evidence/")
                || relative.starts_with("template/decision/")
                || relative.starts_with("template/report/"),
            "unclassified template {relative}"
        );
        assert!(
            audit.contains(relative.as_ref()),
            "no consumer audit for {relative}"
        );
    }
    assert!(!repo_path(".shea/template/workpad/rework-run.md").exists());
    assert!(!repo_path(".shea/template/dogfood-body.md").exists());
}

#[test]
fn setup_shea_is_a_modular_immutable_release_workflow() {
    let skill = skill_file("setup-shea", "SKILL.md");
    let discovery = skill_file("setup-shea", "references/target-discovery.md");
    let release = skill_file("setup-shea", "references/immutable-release.md");
    let resources = skill_file("setup-shea", "references/resource-manifest.md");
    let workflow = skill_file("setup-shea", "references/workflow-project.md");
    let reconciliation = skill_file("setup-shea", "references/reconciliation.md");
    let runtime = skill_file("setup-shea", "references/runtime-profile.md");
    let readiness = skill_file("setup-shea", "references/readiness.md");

    assert!(
        skill.lines().count() < 90,
        "setup-shea must stay a concise router"
    );
    for reference in [
        "target-discovery.md",
        "immutable-release.md",
        "resource-manifest.md",
        "workflow-project.md",
        "reconciliation.md",
        "runtime-profile.md",
        "readiness.md",
    ] {
        assert!(
            skill.contains(&format!("references/{reference}")),
            "setup-shea does not route to {reference}"
        );
    }

    for harness in ["Codex", "Claude Code", "Antigravity"] {
        assert!(discovery.contains(harness), "missing harness {harness}");
    }
    for marker in [
        "latest-release endpoint",
        "full 40-character commit",
        "Do not fall back to `main`",
        "only resource revision for this run",
        "check out that commit detached",
        "leave the target repository and external Project unchanged",
    ] {
        assert!(
            release.contains(marker),
            "release contract missing {marker}"
        );
    }
    assert!(workflow.contains("supported deterministic surface"));
    assert!(workflow.contains("Do not create or rename Project fields/statuses"));
    for marker in [
        "standard Skills CLI",
        "temporary project-local staging root",
        "<detached-checkout>/.agents/skills",
        "--agent <selected-agent> --copy --yes",
        "do not transfer its temporary\n`skills-lock.json`",
        "must not invoke `skills check` or `skills update`",
        "conflict_keep",
        "upstream-hash registry",
        "planned preimage",
    ] {
        assert!(
            reconciliation.contains(marker),
            "reconciliation contract missing {marker}"
        );
    }
    for marker in [
        "credential-free",
        "git hash-object",
        "Write atomically",
        "routes repository discovery or profile\nreconciliation back to `setup-shea`",
    ] {
        assert!(
            runtime.contains(marker),
            "runtime contract missing {marker}"
        );
    }
    assert!(readiness.contains("Prove No Claim"));
    assert!(resources.contains("Install the complete core closure by default"));
    assert!(resources.contains("`setup-shea` is global"));
    assert!(resources.contains("exact staged files"));
    assert!(readiness.contains("launched no Main, Review, or Merge agent"));
    assert!(!repo_path(".agents/skills/setup-shea/assets").exists());
    assert!(!repo_path(".agents/skills/setup-shea/scripts").exists());
    assert!(!repo_path(".agents/skills/shea-symphony-runtime-onboarding/SKILL.md").exists());
}

#[test]
fn setup_shea_fixtures_cover_initial_repeat_conflict_failure_and_pin_cases() {
    let cases = [
        ("clean-target.md", &["`add`", "no lane claim"][..]),
        (
            "repeated-no-change.md",
            &["`unchanged`", "preserve every target byte"][..],
        ),
        (
            "customized-conflict.md",
            &["`conflict_keep`", "Never overwrite"][..],
        ),
        (
            "github-unavailable.md",
            &["leave repository", "lane claims unchanged"][..],
        ),
        (
            "immutable-release.md",
            &["one tag", "one full commit", "containing `main`"][..],
        ),
    ];

    for (name, markers) in cases {
        let source = skill_file("setup-shea", &format!("fixtures/{name}"));
        assert!(source.contains("## Observed input"));
        assert!(source.contains("## Expected plan"));
        assert!(source.contains("## Expected result"));
        for marker in markers {
            assert!(source.contains(marker), "{name} missing {marker}");
        }
    }
}

#[test]
fn backlog_skill_owns_bounded_memory_but_not_promotion_or_execution() {
    let backlog = skill_file("shea-backlog", "SKILL.md");

    assert!(
        backlog.lines().count() < 80,
        "Backlog expanded into a runbook"
    );
    assert!(backlog.contains(".shea/contracts/workflow-capability.v1.md"));
    assert!(backlog.contains("`issue.create`"));
    assert!(!backlog.contains("`issue.promote`"));
    assert!(backlog.contains("$shea-issue-forge"));
}

#[test]
fn deepen_skill_is_a_bounded_report_only_router() {
    let skill = skill_file("shea-deepen", "SKILL.md");
    let metadata = skill_file("shea-deepen", "agents/openai.yaml");

    assert!(
        skill.lines().count() < 70,
        "Deepen must stay a concise router"
    );
    let description = frontmatter_value(&skill, "description").unwrap();
    for trigger in [
        "code",
        "runtime",
        "workflow-contract",
        "test area",
        "cross-file change friction",
        "deepen modules",
        "test seams",
    ] {
        assert!(
            description.contains(trigger),
            "Deepen missing trigger: {trigger}"
        );
    }
    for exclusion in [
        "documentation correctness",
        "freshness",
        "reconciliation",
        "OpenWiki",
        "concrete failure",
        "stuck-execution",
        "faulty-configuration",
        "implementation",
    ] {
        assert!(
            description.contains(exclusion),
            "Deepen missing exclusion: {exclusion}"
        );
    }
    assert!(skill.contains("Route by primary object"));
    assert!(skill.contains("behavior-bearing Markdown"));
    assert!(skill.contains("$shea-doctor"));
    assert!(skill.contains("$shea-docs"));
    let references = relative_markdown_links(&skill)
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        references,
        BTreeSet::from([
            "references/architecture-lens.md".to_string(),
            "references/report-and-retention.md".to_string(),
            "references/scope-and-evidence.md".to_string(),
        ])
    );
    assert!(!metadata.contains("allow_implicit_invocation: false"));
    assert!(metadata.contains("$shea-deepen"));
    assert!(!repo_path(".agents/skills/codebase-design").exists());
    assert!(!repo_path(".agents/skills/shea-deepen/scripts").exists());
    assert!(!repo_path(".agents/skills/shea-deepen/assets").exists());
}

#[test]
fn deepen_fixtures_cover_scope_candidates_routing_no_finding_and_report_limits() {
    let root = repo_path(".agents/skills/shea-deepen/fixtures");
    let actual = fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    let expected = [
        "focused-scope.md",
        "no-finding.md",
        "recent-hotspot.md",
        "report-constraints.md",
        "routing-boundary.md",
        "speculative-seam.md",
        "strong-candidate.md",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);

    let routing = skill_file("shea-deepen", "fixtures/routing-boundary.md");
    for prompt in [
        "评估一下当前 docs 的状态",
        "文档说的是 Temporal，但代码还是 Legacy",
        "这些 Markdown prompt 的组织方式导致跨文件修改",
        "Review prompt 让 Agent 卡住了，帮我修",
        "看看代码有哪些值得深化的模块边界",
    ] {
        assert!(routing.contains(prompt), "missing routing prompt: {prompt}");
    }
    for route in ["without Deepen or Doctor", "through Deepen", "$shea-doctor"] {
        assert!(routing.contains(route), "missing routing result: {route}");
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
    let doctor = skill_file("shea-doctor", "SKILL.md");
    let reference = skill_file("shea-doctor", "references/repository-contract-repair.md");
    let metadata = skill_file("shea-doctor", "agents/openai.yaml");
    let description = frontmatter_value(&doctor, "description").unwrap();

    for trigger in [
        "concrete Shea Symphony",
        "faulty-configuration",
        "stuck-execution",
        "diagnosis or repair",
    ] {
        assert!(
            description.contains(trigger),
            "Doctor missing trigger: {trigger}"
        );
    }
    for exclusion in [
        "general architecture",
        "change-locality",
        "documentation quality",
        "freshness",
        "reconciliation",
        "OpenWiki",
    ] {
        assert!(
            description.contains(exclusion),
            "Doctor missing exclusion: {exclusion}"
        );
    }
    assert!(doctor.contains("Route by primary object"));
    assert!(doctor.contains("$shea-deepen"));
    assert!(doctor.contains("$shea-docs"));

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
    assert!(metadata.contains("$shea-doctor"));
}

#[test]
fn removed_architecture_skill_identity_is_absent_from_active_sources() {
    let removed = ["shea", "improve"].join("-");
    assert!(!repo_path(&format!(".agents/skills/{removed}")).exists());

    let mut files = Vec::new();
    for root in [".agents", ".shea", "docs", "README.md", "tests"] {
        let path = repo_path(root);
        if path.is_dir() {
            files.extend(walk_files(&path));
        } else {
            files.push(path);
        }
    }

    for file in files {
        if let Ok(source) = fs::read_to_string(&file) {
            assert!(
                !source.contains(&removed),
                "removed architecture Skill identity remains in {file:?}"
            );
        }
    }
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
            "shea-doctor",
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
    let forge = skill_file("shea-issue-forge", "references/discussion.md");
    let manual_main = skill_file("shea-manual-main", "SKILL.md");
    let manual_review = skill_file("shea-agent-review", "SKILL.md");
    let manual_merge = skill_file("shea-manual-merge", "SKILL.md");

    assert!(forge.contains("the parent owns final Human Review and UAT"));
    assert!(forge.contains("Subissue Human Review Exception"));
    assert!(manual_review.contains("routes to `Merging`, not `Human Review`"));
    assert!(manual_merge.contains("Do not route native subissue merge repair to `Rework`"));
    assert!(manual_main.contains("Execute one operator-selected Main issue in the current task"));
    assert!(manual_main.contains("Do not create another task"));
    assert!(manual_main.contains("workspace.adopt"));
    assert!(manual_main.contains("source=github_native"));
    assert!(manual_main.contains("source=fallback_diagnostic"));
}

#[test]
fn normal_operational_skills_are_compact_capability_consumers() {
    let skills = [
        "shea-manual-main",
        "shea-agent-review",
        "shea-manual-merge",
        "shea-human-review",
        "shea-doctor",
    ];
    let mut total_lines = 0;

    for skill in skills {
        let source = skill_file(skill, "SKILL.md");
        let lines = source.lines().count();
        total_lines += lines;
        assert!(lines < 70, "{skill} expanded into a command runbook");
        assert!(source.contains(".shea/contracts/workflow-capability.v1.md"));
        for syntax in [
            "project issue",
            "project inspect",
            "gh pr view",
            "cargo run",
            ".shea/bin",
            "--write",
        ] {
            assert!(
                !source.contains(syntax),
                "{skill} duplicates adapter syntax {syntax}"
            );
        }
    }

    assert!(
        total_lines < 225,
        "operational skills regressed into runbooks"
    );
}

#[test]
fn human_review_contract_and_templates_support_operator_owned_decisions() {
    let skill = skill_file("shea-human-review", "SKILL.md");
    let template = repo_file(".shea/template/decision/human-review.md");
    let report = repo_file(".shea/template/report/parent-batch-readiness-report.md");
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
    assert!(report.contains("This readiness report is read-only and advisory"));
    assert!(report.contains("does not prove parent acceptance"));
    assert!(handoff.contains("sole authoritative Human Review contract"));
}

#[test]
fn autoloop_dogfood_docs_and_lane_skills_prefer_the_foreground_loop() {
    let command_reference = repo_file("docs/cli-command-reference.md");
    let operator_dogfood = repo_file("docs/operator-dogfood.md");
    let supervised_runbook = repo_file("docs/supervised-live-dogfood.md");
    let manual_review = skill_file("shea-agent-review", "SKILL.md");
    let manual_merge = skill_file("shea-manual-merge", "SKILL.md");

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
