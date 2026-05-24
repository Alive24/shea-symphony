use super::*;

#[test]
fn parses_forge_create_flags() {
    let command = Command::parse(vec![
        "forge".into(),
        "create".into(),
        "--workflow".into(),
        "examples/dry-run-workflow.md".into(),
        "--title".into(),
        "Create issue".into(),
        "--body".into(),
        forge_contract(),
        "--status".into(),
        "todo".into(),
        "--project".into(),
        "workflow".into(),
        "--project-field".into(),
        "Capability=CLI".into(),
        "--assignee".into(),
        "@Alive24".into(),
        "--write".into(),
    ])
    .unwrap();

    let Command::ForgeCreate {
        workflow_path,
        title,
        markdown,
        status,
        project,
        project_fields,
        assignees,
        write,
        dry_run,
    } = command
    else {
        panic!("expected forge create command");
    };

    assert_eq!(workflow_path, PathBuf::from("examples/dry-run-workflow.md"));
    assert_eq!(title, "Create issue");
    assert!(markdown.contains("## Issue Goal"));
    assert_eq!(status, ForgeStatusArg::Todo);
    assert_eq!(project.as_deref(), Some("workflow"));
    assert_eq!(
        project_fields,
        vec![ProjectFieldAssignment {
            name: "Capability".into(),
            value: "CLI".into()
        }]
    );
    assert_eq!(assignees, vec!["@Alive24".to_string()]);
    assert!(write);
    assert!(!dry_run);
}

#[test]
fn parses_forge_promote_flags() {
    let command = Command::parse(vec![
        "forge".into(),
        "promote".into(),
        "#241".into(),
        "--workflow".into(),
        "examples/dry-run-workflow.md".into(),
        "--title".into(),
        "Promoted issue".into(),
        "--body".into(),
        forge_contract(),
        "--operator-confirmation".into(),
        "promote it".into(),
        "--decision".into(),
        "Keep this as an in-place promotion.".into(),
        "--scope-change".into(),
        "Promoted body is now executable.".into(),
        "--dependency-context".into(),
        "Dependencies: none.".into(),
        "--readback-summary".into(),
        "Operator confirmed the dry-run preview before write.".into(),
        "--dry-run".into(),
    ])
    .unwrap();

    let Command::ForgePromote {
        workflow_path,
        issue_ref,
        title,
        markdown,
        promotion_note,
        write,
        dry_run,
    } = command
    else {
        panic!("expected forge promote command");
    };

    assert_eq!(workflow_path, PathBuf::from("examples/dry-run-workflow.md"));
    assert_eq!(issue_ref, "#241");
    assert_eq!(title, "Promoted issue");
    assert!(markdown.contains("## Issue Goal"));
    assert_eq!(promotion_note.operator_confirmation, "promote it");
    assert_eq!(
        promotion_note.decisions,
        vec!["Keep this as an in-place promotion.".to_string()]
    );
    assert_eq!(
        promotion_note.readback_summaries,
        vec!["Operator confirmed the dry-run preview before write.".to_string()]
    );
    assert!(!write);
    assert!(dry_run);
}

#[test]
fn parses_forge_rework_flags() {
    let temp = tempfile::tempdir().unwrap();
    let body_path = temp.path().join("body.md");
    let evidence_path = temp.path().join("evidence.md");
    std::fs::write(&body_path, forge_contract()).unwrap();
    std::fs::write(&evidence_path, "Reviewer changed the execution contract.").unwrap();

    let command = Command::parse(vec![
        "forge".into(),
        "rework".into(),
        "#282".into(),
        "--workflow".into(),
        "examples/dry-run-workflow.md".into(),
        "--title".into(),
        "Reworked contract".into(),
        "--body-file".into(),
        body_path.display().to_string(),
        "--evidence-file".into(),
        evidence_path.display().to_string(),
        "--operator-confirmation".into(),
        "send it back to Rework".into(),
        "--dry-run".into(),
    ])
    .unwrap();

    let Command::ForgeRework { options } = command else {
        panic!("expected forge rework command");
    };

    assert_eq!(
        options.workflow_path,
        PathBuf::from("examples/dry-run-workflow.md")
    );
    assert_eq!(options.issue_ref, "#282");
    assert_eq!(options.title, "Reworked contract");
    assert!(options.markdown.contains("## Issue Goal"));
    assert_eq!(options.evidence, "Reviewer changed the execution contract.");
    assert_eq!(options.operator_confirmation, "send it back to Rework");
    assert!(!options.write);
    assert!(options.dry_run);
}

#[test]
fn parses_link_pr_flags() {
    let command = Command::parse(vec![
        "project".into(),
        "link-pr".into(),
        "examples/github-project-workflow.md".into(),
        "#127".into(),
        "https://github.com/Alive24/jade-symphony/pull/128".into(),
        "--write".into(),
    ])
    .unwrap();

    let Command::LinkPr {
        workflow_path,
        issue_ref,
        pr_ref,
        write,
    } = command
    else {
        panic!("expected link-pr command");
    };

    assert_eq!(
        workflow_path,
        PathBuf::from("examples/github-project-workflow.md")
    );
    assert_eq!(issue_ref, "#127");
    assert_eq!(pr_ref, "https://github.com/Alive24/jade-symphony/pull/128");
    assert!(write);
}

#[test]
fn parses_forge_validate_issue_flags() {
    let command = Command::parse(vec![
        "forge".into(),
        "validate".into(),
        "--workflow".into(),
        "examples/github-project-workflow.md".into(),
        "--issue".into(),
        "#248".into(),
        "--status".into(),
        "todo".into(),
    ])
    .unwrap();

    let Command::ForgeValidate {
        workflow_path,
        status,
        title,
        markdown,
        issue_ref,
    } = command
    else {
        panic!("expected forge validate command");
    };

    assert_eq!(
        workflow_path,
        PathBuf::from("examples/github-project-workflow.md")
    );
    assert_eq!(status, Some(ForgeStatusArg::Todo));
    assert!(title.is_empty());
    assert!(markdown.is_empty());
    assert_eq!(issue_ref.as_deref(), Some("#248"));
}

#[test]
fn parses_forge_validate_issue_with_candidate_body_flags() {
    let temp = tempfile::tempdir().unwrap();
    let body_path = temp.path().join("candidate.md");
    std::fs::write(&body_path, forge_contract()).unwrap();

    let command = Command::parse(vec![
        "forge".into(),
        "validate".into(),
        "--workflow".into(),
        "examples/github-project-workflow.md".into(),
        "--issue".into(),
        "#293".into(),
        "--status".into(),
        "todo".into(),
        "--title".into(),
        "Candidate promoted title".into(),
        "--body-file".into(),
        body_path.display().to_string(),
    ])
    .unwrap();

    let Command::ForgeValidate {
        status,
        title,
        markdown,
        issue_ref,
        ..
    } = command
    else {
        panic!("expected forge validate command");
    };

    assert_eq!(status, Some(ForgeStatusArg::Todo));
    assert_eq!(title, "Candidate promoted title");
    assert!(markdown.contains("## Issue Goal"));
    assert_eq!(issue_ref.as_deref(), Some("#293"));
}

#[test]
fn rejects_removed_flat_forge_commands() {
    let error = Command::parse(vec![
        "forge-create".into(),
        "--workflow".into(),
        "workflows/jade-symphony.md".into(),
    ])
    .unwrap_err();

    assert!(error.contains("Usage:"));
}

#[test]
fn rejects_forge_create_with_both_body_and_file() {
    let error = Command::parse(vec![
        "forge".into(),
        "create".into(),
        "--workflow".into(),
        "WORKFLOW.md".into(),
        "--title".into(),
        "Create issue".into(),
        "--body".into(),
        forge_contract(),
        "--body-file".into(),
        "issue.md".into(),
    ])
    .unwrap_err();

    assert!(error.contains("Usage:"));
}
