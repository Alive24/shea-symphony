use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn skill_suite_lists_human_review_skill() {
    let manifest = repo_file("skills/shea-symphony/manifest.toml");
    let readme = repo_file("skills/shea-symphony/README.md");
    let skill = repo_file("skills/shea-symphony/suite/shea-symphony-human-review/SKILL.md");
    let installed = repo_file(".agents/skills/shea-symphony-human-review/SKILL.md");
    let metadata =
        repo_file("skills/shea-symphony/suite/shea-symphony-human-review/agents/openai.yaml");
    let installed_metadata =
        repo_file(".agents/skills/shea-symphony-human-review/agents/openai.yaml");

    assert!(manifest.contains("name = \"shea-symphony-human-review\""));
    assert!(manifest.contains("path = \"suite/shea-symphony-human-review\""));
    assert!(readme.contains("`shea-symphony-human-review`"));
    assert_eq!(skill, installed, "repo-local Human Review skill drifted");
    assert_eq!(
        metadata, installed_metadata,
        "repo-local Human Review metadata drifted"
    );
    assert!(skill.contains("Accepted Human Review routes to `Merging`"));
    assert!(skill.contains("Native GitHub subissues are not routine Human Review surfaces"));
    assert!(skill.contains("Subissue Human Review Exception: <reason>"));
    assert!(skill.contains("Never mutate Project state until the operator explicitly confirms"));
    assert!(
        skill.lines().count() <= 150,
        "Human Review core should stay compact"
    );
    for field in [
        "**Problem**",
        "**Delivered change**",
        "**Resulting effect**",
        "**Evidence**",
        "**Human decision needed**",
    ] {
        assert!(skill.contains(field), "missing visible brief field {field}");
    }
    assert!(skill.contains("reasoning, tool output"));
    assert!(skill.contains("test summaries do not"));
    assert!(skill.contains("satisfy this visible briefing contract"));
    assert!(skill.contains("## Operator-authorized UAT remediation"));
    assert!(skill.contains("Mark the prior Agent Review PASS stale"));
    assert!(skill.contains("Self-verification is not independent"));
}

#[test]
fn skill_suite_records_parent_owned_subissue_contract() {
    let forge = repo_file("skills/shea-symphony/suite/shea-symphony-issue-forge/SKILL.md");
    let reflect =
        repo_file("skills/shea-symphony/suite/shea-symphony-issue-forge-reflect/SKILL.md");
    let manual_review =
        repo_file("skills/shea-symphony/suite/shea-symphony-manual-review/SKILL.md");
    let manual_merge = repo_file("skills/shea-symphony/suite/shea-symphony-manual-merge/SKILL.md");

    assert!(forge.contains("draft the parent as the final"));
    assert!(forge.contains("Subissue Human Review Exception: <reason>"));
    assert!(reflect.contains("parent issue the"));
    assert!(reflect.contains("passing review to `Merging`"));
    assert!(manual_review.contains("routine native"));
    assert!(manual_review.contains("routes to `Merging`, not `Human Review`"));
    assert!(manual_merge.contains("For native subissue PRs"));
    assert!(manual_merge.contains("Do not route native subissue merge repair to `Rework`"));
    assert!(manual_merge.contains("Clean merge is CLI-owned and non-LLM"));
    assert!(manual_merge.contains("merge_lane.agent_backend: codex"));
}

#[test]
fn skill_suite_documents_app_server_first_manual_boundaries() {
    let human_review = repo_file("skills/shea-symphony/suite/shea-symphony-human-review/SKILL.md");
    let manual_merge = repo_file("skills/shea-symphony/suite/shea-symphony-manual-merge/SKILL.md");
    let manual_main = repo_file("skills/shea-symphony/suite/shea-symphony-manual-main/SKILL.md");

    assert!(human_review.contains("Match the operator-facing language"));
    assert!(human_review.contains("Do not force English"));
    assert!(human_review.contains("Run the PR freshness preflight automatically"));
    assert!(human_review.contains("not operator-owned UAT"));
    let freshness = human_review
        .find("Run the PR freshness preflight automatically")
        .expect("missing freshness step");
    let brief = human_review
        .find("Present one current five-field brief")
        .expect("missing visible brief step");
    assert!(
        freshness < brief,
        "freshness must precede the decision brief"
    );
    assert!(human_review.contains("A UAT result\nalone is not confirmation"));
    assert!(manual_main.contains("Execute one operator-selected Main issue in the current task"));
    assert!(manual_main.contains("Do not create another task"));
    assert!(manual_main.contains("workspace adopt"));
    assert!(manual_main.contains("do not\ncreate a nested worktree there"));
    assert!(manual_main
        .contains("Adoption transfers issue-workspace evidence, not cleanup\n   ownership"));
    assert!(manual_merge.contains("cargo run -- merge loop .shea/workflows/shea-symphony.md"));
    assert!(manual_merge.contains("app-server"));
    assert!(!manual_merge.contains("merge-once"));
}

#[test]
fn human_review_template_supports_all_decisions() {
    let template = repo_file(".shea/template/workpad/human-review.md");

    assert!(!template.contains("<!-- shea-symphony-workpad -->"));
    assert!(template.contains("## Shea Symphony Human Review Decision"));
    assert!(template.contains("Approve for Merging"));
    assert!(template.contains("Decision timestamp: <YYYY-MM-DD HH:MM timezone>"));
    assert!(template.contains("Request Rework"));
    assert!(template.contains("Need Human Input"));
    assert!(template.contains("Defer"));
    assert!(template.contains("Target state after explicit confirmation"));
    assert!(template.contains("### Decision Context"));
    assert!(template.contains("- Problem:"));
    assert!(template.contains("- Delivered change:"));
    assert!(template.contains("- Observed effect:"));
    assert!(template.contains("- Human decision needed:"));
}

#[test]
fn parent_batch_human_review_brief_preserves_order_and_boundaries() {
    let skill = repo_file("skills/shea-symphony/suite/shea-symphony-human-review/SKILL.md");
    let brief = repo_file(".shea/template/workpad/parent-batch-human-review-brief.md");
    let decision = repo_file(".shea/template/workpad/human-review.md");

    assert!(skill.contains(
        "first Human\nReview action is to prepare a compact parent-batch evidence brief"
    ));
    assert!(skill.contains(".shea/template/workpad/parent-batch-human-review-brief.md"));
    assert!(skill.contains("read-only and advisory"));
    assert!(skill.contains("Do not write tracker comments"));
    assert!(skill.contains("Child `Done`, child PR merge"));
    assert!(skill.contains("Agent Review PASS are inputs"));
    assert!(skill.contains("not proof that parent UAT passed"));

    assert!(brief.contains("## Shea Symphony Parent-Batch Human Review Brief"));
    assert!(brief.contains("This brief is read-only and advisory"));
    assert!(brief.contains("not a Human Review decision note"));
    assert!(brief.contains("must not write tracker comments"));
    assert!(brief.contains("Human Review-owned"));
    assert!(brief.contains("does not prove parent acceptance"));
    assert!(brief.contains("parent PR #421"));
    assert!(brief.contains("### Decision Context"));
    assert!(brief.contains("- Problem:"));
    assert!(brief.contains("- Delivered change:"));
    assert!(brief.contains("- Observed effect:"));
    assert!(brief.contains("- Human decision needed:"));

    let required_order = [
        "### 1. Remaining Parent UAT",
        "### 2. Parent PR And Readiness",
        "### 3. Child Batch Table",
        "### 4. Review Agent Evidence",
        "### 5. Risks, Stale Assumptions, Or Missing Evidence",
    ];
    let mut last_index = 0;
    for heading in required_order {
        let index = brief
            .find(heading)
            .unwrap_or_else(|| panic!("missing parent-batch heading {heading}"));
        assert!(index >= last_index, "heading out of order: {heading}");
        last_index = index;
    }

    assert!(decision.contains("## Shea Symphony Human Review Decision"));
    assert!(!decision.contains("## Shea Symphony Parent-Batch Human Review Brief"));
}

#[test]
fn human_review_handoff_launches_the_authoritative_skill_with_stale_context() {
    let prompt = repo_file(".shea/prompts/human-review-handoff.md");

    assert!(prompt.contains("Use the shea-symphony-human-review skill"));
    assert!(prompt.contains("{{ issue.identifier }} {{ issue.title }}"));
    assert!(prompt.contains("refresh them before relying on them"));
    assert!(prompt.contains("sole authoritative Human Review contract"));
    assert!(prompt.contains("onboarding/configuration drift"));
    for duplicated_contract in [
        "Delivered change:",
        "Human decision needed:",
        "until the operator gives explicit approval",
    ] {
        assert!(
            !prompt.contains(duplicated_contract),
            "handoff duplicates skill behavior: {duplicated_contract}"
        );
    }
}

#[test]
fn autoloop_dogfood_docs_prefer_foreground_loop() {
    let command_reference = repo_file("docs/cli-command-reference.md");
    let operator_dogfood = repo_file("docs/operator-dogfood.md");
    let supervised_runbook = repo_file("docs/supervised-live-dogfood.md");
    let launcher = repo_file("scripts/shea-dogfood");
    let suite_readme = repo_file("skills/shea-symphony/README.md");
    let manual_review =
        repo_file("skills/shea-symphony/suite/shea-symphony-manual-review/SKILL.md");
    let manual_merge = repo_file("skills/shea-symphony/suite/shea-symphony-manual-merge/SKILL.md");

    for document in [
        &command_reference,
        &operator_dogfood,
        &supervised_runbook,
        &suite_readme,
        &manual_review,
        &manual_merge,
    ] {
        assert!(document.contains("autopilot plan"));
        assert!(document.contains("autopilot loop"));
    }

    assert!(command_reference.contains("not a daemon"));
    assert!(command_reference.contains("Lane throughput is independent"));
    assert!(command_reference.contains("lane limit to `0`"));
    assert!(operator_dogfood.contains("not a daemon"));
    assert!(operator_dogfood.contains("Parent #405 UAT Checklist"));
    assert!(operator_dogfood.contains("one operator-controlled supervisor over independent"));
    assert!(supervised_runbook.contains("not a daemon"));
    assert!(docs_readiness_contains_independent_lane_model(&repo_file(
        "docs/dogfood-readiness.md"
    )));
    assert!(suite_readme.contains("not a daemon"));
    assert!(suite_readme.contains("app-server"));
    assert!(command_reference.contains("debugging"));
    assert!(suite_readme.contains("focused debugging"));
    assert!(launcher.contains("autopilot plan"));
    assert!(launcher.contains("autopilot loop"));
    assert!(!launcher.contains("run-loop"));
    assert!(!launcher.contains("project-state"));
}

fn docs_readiness_contains_independent_lane_model(document: &str) -> bool {
    document.contains("Autoloop lane throughput is independent")
        && document.contains("shared")
        && document.contains("global iteration gate")
}
