use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const READS: &[&str] = &[
    "workflow.resolve",
    "issue.read",
    "issue.inspect",
    "evidence.read",
    "pull_request.read",
    "relationships.read",
];

const ACTIONS: &[&str] = &[
    "workspace.adopt",
    "lane.claim",
    "workpad.upsert",
    "timeline.append",
    "issue.transition",
    "pull_request.link",
    "relationship.add_blocked_by",
    "relationship.add_subissue",
    "issue.create",
    "issue.promote",
    "issue.revise",
    "issue.rework",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityMetadata {
    kind: String,
    contract_version: u64,
    active_workflow: String,
    adapters: Vec<AdapterReference>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterReference {
    id: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterMetadata {
    kind: String,
    adapter_id: String,
    adapter_version: u64,
    capability: String,
    runtime_role: String,
    compatibility: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsumerFixtureMetadata {
    kind: String,
    fixture_version: u64,
    consumer: String,
    capability: String,
    adapter: String,
    required_reads: Vec<String>,
    guarded_actions: Vec<String>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_file(path: &str) -> String {
    fs::read_to_string(repo_root().join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

fn front_matter<T: for<'de> Deserialize<'de>>(source: &str) -> Result<(T, &str), String> {
    let source = source
        .strip_prefix("---\n")
        .ok_or_else(|| "missing YAML front matter".to_string())?;
    let (yaml, body) = source
        .split_once("\n---\n")
        .ok_or_else(|| "unterminated YAML front matter".to_string())?;
    let metadata = serde_yaml::from_str(yaml).map_err(|error| error.to_string())?;
    Ok((metadata, body))
}

fn reject_target_or_owned_values(source: &str) -> Result<(), String> {
    for forbidden in [
        "Alive24/",
        "github.com/",
        "/Users/",
        "/home/",
        "C:\\\\",
        "project_number:",
        "base_branch:",
        "cli_path:",
        "verification.commands",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "contract duplicates workflow/profile ownership or target identity: {forbidden}"
            ));
        }
    }
    Ok(())
}

fn validate_capability(source: &str) -> Result<CapabilityMetadata, String> {
    let (metadata, body): (CapabilityMetadata, _) = front_matter(source)?;
    if metadata.kind != "shea-workflow-capability" || metadata.contract_version != 1 {
        return Err("unsupported or missing capability version".into());
    }
    if metadata.active_workflow.is_empty() || Path::new(&metadata.active_workflow).is_absolute() {
        return Err("active workflow reference must be relative".into());
    }
    if metadata.adapters.is_empty() {
        return Err("capability must reference at least one adapter".into());
    }
    reject_target_or_owned_values(source)?;
    if body.lines().count() > 130 {
        return Err("stable contract is oversized".into());
    }
    for name in READS.iter().chain(ACTIONS) {
        if !body.contains(&format!("`{name}`")) {
            return Err(format!("stable contract is missing semantic name {name}"));
        }
    }
    for phase in [
        "**Prepare**",
        "**Confirm**",
        "**Execute**",
        "**Targeted readback**",
    ] {
        if !body.contains(phase) {
            return Err(format!("stable contract is missing mutation phase {phase}"));
        }
    }
    for classification in [
        "`applied`",
        "`not_applied`",
        "`rejected`",
        "`uncertain`",
        "`ambiguous`",
    ] {
        if !body.contains(classification) {
            return Err(format!("stable contract is missing {classification}"));
        }
    }
    for legacy_syntax in ["`CLI ", "`gh ", "--write", "`project ", "`forge "] {
        if body.contains(legacy_syntax) {
            return Err(format!(
                "stable contract contains adapter syntax: {legacy_syntax}"
            ));
        }
    }
    Ok(metadata)
}

fn validate_adapter(source: &str) -> Result<AdapterMetadata, String> {
    let (metadata, body): (AdapterMetadata, _) = front_matter(source)?;
    if metadata.kind != "shea-workflow-capability-adapter"
        || metadata.adapter_id != "legacy-cli-v1"
        || metadata.adapter_version != 1
        || metadata.runtime_role != "legacy_cli"
        || metadata.compatibility != "shea-legacy-cli-v1"
    {
        return Err("unsupported or missing adapter identity".into());
    }
    reject_target_or_owned_values(source)?;
    if body.lines().count() > 150 {
        return Err("Legacy adapter is oversized".into());
    }
    if !body.contains("The stable contract owns") {
        return Err("adapter must preserve stable-contract ownership".into());
    }
    for name in READS.iter().chain(ACTIONS) {
        if !body.contains(&format!("`{name}`")) {
            return Err(format!("adapter is missing mapping for {name}"));
        }
    }
    for expected in [
        "project issue",
        "project inspect",
        "project relationship list",
        "project workpad",
        "project set-state",
        "project link-pr",
        "forge validate",
        "forge revise",
        "--write",
    ] {
        if !body.contains(expected) {
            return Err(format!("adapter is missing Legacy surface {expected}"));
        }
    }
    Ok(metadata)
}

fn validate_fixture(source: &str) -> Result<ConsumerFixtureMetadata, String> {
    let (metadata, body): (ConsumerFixtureMetadata, _) = front_matter(source)?;
    if metadata.kind != "shea-workflow-capability-consumer-fixture" || metadata.fixture_version != 1
    {
        return Err("unsupported or missing consumer fixture version".into());
    }
    reject_target_or_owned_values(source)?;
    if body.lines().count() > 45 {
        return Err("consumer fixture is an oversized runbook".into());
    }
    for name in &metadata.required_reads {
        if !READS.contains(&name.as_str()) {
            return Err(format!("fixture uses unknown read {name}"));
        }
    }
    for name in &metadata.guarded_actions {
        if !ACTIONS.contains(&name.as_str()) {
            return Err(format!("fixture uses unknown action {name}"));
        }
    }
    for runbook_syntax in ["project issue", "forge create", "--write", "gh pr view"] {
        if body.contains(runbook_syntax) {
            return Err(format!(
                "fixture duplicates adapter syntax: {runbook_syntax}"
            ));
        }
    }
    if !body.contains("capability contract owns mutation ordering and uncertainty handling") {
        return Err("fixture must defer shared mutation semantics to the contract".into());
    }
    Ok(metadata)
}

#[test]
fn workflow_capability_and_legacy_adapter_are_versioned_and_cross_referenced() {
    let capability_path = repo_root().join(".shea/contracts/workflow-capability.v1.md");
    let adapter_path = repo_root().join(".shea/contracts/adapters/legacy-cli.v1.md");
    let capability_source = repo_file(".shea/contracts/workflow-capability.v1.md");
    let adapter_source = repo_file(".shea/contracts/adapters/legacy-cli.v1.md");

    let capability = validate_capability(&capability_source).expect("valid stable capability");
    let adapter = validate_adapter(&adapter_source).expect("valid Legacy adapter");

    assert!(capability_path
        .parent()
        .unwrap()
        .join(&capability.active_workflow)
        .is_file());
    let adapter_refs: BTreeSet<_> = capability
        .adapters
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(adapter_refs, BTreeSet::from(["legacy-cli-v1"]));
    assert!(capability_path
        .parent()
        .unwrap()
        .join(&capability.adapters[0].path)
        .is_file());
    assert_eq!(
        fs::canonicalize(adapter_path.parent().unwrap().join(&adapter.capability)).unwrap(),
        fs::canonicalize(capability_path).unwrap()
    );
}

#[test]
fn representative_consumers_reference_shared_contracts_without_commands() {
    for (path, consumer) in [
        (
            "tests/fixtures/workflow-capability/manual-main.md",
            "manual-main",
        ),
        ("tests/fixtures/workflow-capability/backlog.md", "backlog"),
    ] {
        let fixture = validate_fixture(&repo_file(path)).expect("valid consumer fixture");
        assert_eq!(fixture.consumer, consumer);
        assert!(repo_root().join(&fixture.capability).is_file());
        assert!(repo_root().join(&fixture.adapter).is_file());
        assert!(!fixture.required_reads.is_empty());
        assert!(!fixture.guarded_actions.is_empty());
    }
}

#[test]
fn operational_skills_are_real_capability_consumers_not_command_runbooks() {
    for skill in [
        "shea-manual-main",
        "shea-agent-review",
        "shea-manual-merge",
        "shea-human-review",
        "shea-doctor",
    ] {
        let path = format!(".agents/skills/{skill}/SKILL.md");
        let source = repo_file(&path);
        assert!(source.contains(".shea/contracts/workflow-capability.v1.md"));
        assert!(source.lines().count() < 70, "{skill} is oversized");
        for runbook_syntax in ["project issue", "project inspect", "gh pr view", "--write"] {
            assert!(
                !source.contains(runbook_syntax),
                "{skill} duplicates adapter syntax: {runbook_syntax}"
            );
        }
    }
}

#[test]
fn structural_validation_rejects_missing_refs_duplicated_ownership_and_runbooks() {
    let capability = repo_file(".shea/contracts/workflow-capability.v1.md");
    let fixture = repo_file("tests/fixtures/workflow-capability/manual-main.md");

    assert!(validate_capability(&capability.replace("contract_version: 1\n", "")).is_err());
    assert!(validate_capability(&capability.replace(
        "adapters:\n  - id: legacy-cli-v1\n    path: adapters/legacy-cli.v1.md",
        "adapters: []",
    ))
    .is_err());
    assert!(validate_capability(&format!("{capability}\nproject_number: 9")).is_err());
    assert!(validate_capability(&format!("{capability}\nAlive24/shea-symphony")).is_err());
    assert!(validate_capability(&format!("{capability}\n/Users/example/tool")).is_err());

    let oversized = format!("{fixture}\n{}", "- duplicated procedure\n".repeat(60));
    assert!(validate_fixture(&oversized).is_err());
    assert!(validate_fixture(&format!("{fixture}\nproject issue WORKFLOW ISSUE")).is_err());
}

#[test]
fn context_router_points_agents_to_the_authoritative_capability_contract() {
    let docs = repo_file("docs/README.md");
    assert!(docs.contains("`.shea/contracts/`, repository Skills, prompts, and templates"));
    assert!(docs.contains("`.shea/contracts/workflow-capability.v1.md`"));
    assert!(docs.contains("Do not maintain a second Markdown runbook"));
}
