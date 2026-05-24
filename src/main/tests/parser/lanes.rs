use super::*;

#[test]
fn parses_review_loop_flags() {
    let command = Command::parse(vec![
        "review".into(),
        "loop".into(),
        "examples/review-fixture-workflow.md".into(),
        "--max-iterations".into(),
        "2".into(),
        "--fake-outcome".into(),
        "confirmed".into(),
        "--max-concurrent".into(),
        "2".into(),
        "--write".into(),
    ])
    .unwrap();

    let Command::ReviewLoop { options } = command else {
        panic!("expected review loop command");
    };

    assert_eq!(
        options.workflow_path,
        PathBuf::from("examples/review-fixture-workflow.md")
    );
    assert_eq!(options.max_iterations, Some(2));
    assert_eq!(
        options.fake_outcome,
        Some(FakeReviewOutcome::ConfirmedFinding)
    );
    assert_eq!(options.max_concurrent, Some(2));
    assert!(options.write);
}

#[test]
fn review_loop_once_overrides_max_iterations() {
    let command = Command::parse(vec![
        "review".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "4".into(),
        "--once".into(),
    ])
    .unwrap();

    let Command::ReviewLoop { options } = command else {
        panic!("expected review loop command");
    };

    assert_eq!(options.iteration_limit(), Some(1));
}

#[test]
fn parses_review_status_flags() {
    let command = Command::parse(vec![
        "review".into(),
        "status".into(),
        "examples/review-fixture-workflow.md".into(),
        "--issue".into(),
        "#313".into(),
        "--recent".into(),
        "3".into(),
        "--verbose".into(),
        "--json".into(),
    ])
    .unwrap();

    let Command::ReviewStatus { options } = command else {
        panic!("expected review status command");
    };

    assert_eq!(
        options.workflow_path,
        PathBuf::from("examples/review-fixture-workflow.md")
    );
    assert_eq!(options.issue_filter.as_deref(), Some("#313"));
    assert_eq!(options.recent_limit, 3);
    assert!(options.verbose);
    assert!(options.json);
}

#[test]
fn parses_merge_loop_flags() {
    let command = Command::parse(vec![
        "merge".into(),
        "loop".into(),
        "examples/github-project-workflow.md".into(),
        "--max-iterations".into(),
        "3".into(),
        "--max-concurrent".into(),
        "2".into(),
        "--write".into(),
    ])
    .unwrap();

    let Command::MergeLoop { options } = command else {
        panic!("expected merge loop command");
    };

    assert_eq!(
        options.workflow_path,
        PathBuf::from("examples/github-project-workflow.md")
    );
    assert_eq!(options.max_iterations, Some(3));
    assert_eq!(options.max_concurrent, Some(2));
    assert_eq!(options.worker_limit(&test_config()), 2);
    assert!(options.write);
    assert!(options.recover);
}

#[test]
fn merge_loop_no_recover_disables_write_default() {
    let command = Command::parse(vec![
        "merge".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "1".into(),
        "--write".into(),
        "--no-recover".into(),
    ])
    .unwrap();

    let Command::MergeLoop { options } = command else {
        panic!("expected merge loop command");
    };

    assert!(options.write);
    assert!(!options.recover);
}

#[test]
fn merge_loop_once_overrides_max_iterations() {
    let command = Command::parse(vec![
        "merge".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "4".into(),
        "--once".into(),
    ])
    .unwrap();

    let Command::MergeLoop { options } = command else {
        panic!("expected merge loop command");
    };

    assert_eq!(options.iteration_limit(), Some(1));
}

#[test]
fn parses_unbounded_merge_loop_without_max_iterations() {
    let command =
        Command::parse(vec!["merge".into(), "loop".into(), "WORKFLOW.md".into()]).unwrap();

    let Command::MergeLoop { options } = command else {
        panic!("expected merge loop command");
    };

    assert_eq!(options.iteration_limit(), None);
}

#[test]
fn rejects_zero_merge_loop_iterations() {
    assert!(Command::parse(vec![
        "merge".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "0".into(),
    ])
    .is_err());
}

#[test]
fn rejects_zero_merge_loop_max_concurrent() {
    assert!(Command::parse(vec![
        "merge".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "1".into(),
        "--max-concurrent".into(),
        "0".into(),
    ])
    .is_err());
}

#[test]
fn parses_run_loop_flags() {
    let command = Command::parse(vec![
        "main".into(),
        "loop".into(),
        "examples/dry-run-workflow.md".into(),
        "--max-iterations".into(),
        "3".into(),
        "--max-concurrent".into(),
        "4".into(),
        "--display".into(),
        "tui".into(),
        "--dry-run".into(),
    ])
    .unwrap();

    let Command::RunLoop { options } = command else {
        panic!("expected main loop command");
    };

    assert_eq!(
        options.workflow_path,
        PathBuf::from("examples/dry-run-workflow.md")
    );
    assert_eq!(options.max_iterations, Some(3));
    assert_eq!(options.max_concurrent, Some(4));
    assert_eq!(options.worker_limit(&test_config()), 4);
    assert_eq!(options.display, DisplayMode::Tui);
    assert!(!options.once);
    assert!(!options.write);
    assert!(!options.recover);
}

#[test]
fn run_loop_write_defaults_to_recover() {
    let command = Command::parse(vec![
        "main".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--write".into(),
    ])
    .unwrap();

    let Command::RunLoop { options } = command else {
        panic!("expected main loop command");
    };

    assert!(options.write);
    assert!(options.recover);
}

#[test]
fn run_loop_no_recover_disables_write_default() {
    let command = Command::parse(vec![
        "main".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--write".into(),
        "--no-recover".into(),
    ])
    .unwrap();

    let Command::RunLoop { options } = command else {
        panic!("expected main loop command");
    };

    assert!(options.write);
    assert!(!options.recover);
}

#[test]
fn run_loop_once_overrides_max_iterations() {
    let command = Command::parse(vec![
        "main".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "9".into(),
        "--once".into(),
        "--write".into(),
    ])
    .unwrap();

    let Command::RunLoop { options } = command else {
        panic!("expected main loop command");
    };

    assert_eq!(options.iteration_limit(), Some(1));
    assert!(options.write);
    assert!(options.recover);
}

#[test]
fn parses_merge_once_command() {
    let command = Command::parse(vec![
        "merge".into(),
        "once".into(),
        "examples/github-project-workflow.md".into(),
        "--dry-run".into(),
    ])
    .unwrap();

    assert_eq!(
        command,
        Command::MergeOnce {
            workflow_path: PathBuf::from("examples/github-project-workflow.md"),
            write: false
        }
    );

    assert!(Command::parse(vec![
        "land".into(),
        "examples/github-project-workflow.md".into(),
        "--write".into()
    ])
    .is_err());
}

#[test]
fn rejects_zero_run_loop_iterations() {
    let error = Command::parse(vec![
        "main".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "0".into(),
    ])
    .unwrap_err();

    assert!(error.contains("Usage:"));
}

#[test]
fn rejects_zero_run_loop_max_concurrent() {
    let error = Command::parse(vec![
        "main".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "1".into(),
        "--max-concurrent".into(),
        "0".into(),
    ])
    .unwrap_err();

    assert!(error.contains("Usage:"));
}

#[test]
fn rejects_zero_review_loop_iterations() {
    let error = Command::parse(vec![
        "review".into(),
        "loop".into(),
        "WORKFLOW.md".into(),
        "--max-iterations".into(),
        "0".into(),
    ])
    .unwrap_err();

    assert!(error.contains("Usage:"));
}
