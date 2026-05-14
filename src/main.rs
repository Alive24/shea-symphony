use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{error::ErrorKind, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use jade_symphony::agent::{backend_from_config, usage_limit_pause_from_events, UsageLimitPause};
use jade_symphony::config::RuntimeConfig;
use jade_symphony::doctor::{
    audit_project_issues, human_review_repair_candidates, render_human_review_repair_workpad,
    render_project_audit_report, render_project_audit_report_json,
};
use jade_symphony::event_log::{EventLog, EventRecord};
use jade_symphony::git_handoff::{
    prepare_issue_worktree, publish_issue_pull_request, LiveWorktreeResult,
    ProcessHandoffCommandRunner, PullRequestPublication,
};
use jade_symphony::handoff::{
    evaluate_agent_review_handoff, plan_issue_handoff_for_profile,
    render_agent_review_handoff_workpad, AgentReviewHandoffEvidence, HandoffError,
    IssueHandoffPlan,
};
use jade_symphony::issue_forge::{
    discover_candidates, draft_from_template, find_issue_skill, interactive_forge,
    next_clarification_question, reflective_candidates_from_context, repair_markdown,
    validate_markdown, InteractiveForgeInput,
};
use jade_symphony::merge_lane::{
    expected_merge_base_branch, fetch_pull_request_status, merge_lane_decision, merge_lane_workpad,
    merge_pull_request, pull_request_status_from_linked, MergeLaneDecisionKind,
};
use jade_symphony::model::{normalize_state, GateDecision, GateDecisionKind, TrackerIssue};
use jade_symphony::observability_api::serve_once;
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::ownership::{
    render_runtime_ownership_marker, runtime_ownership_decision, RuntimeOwnershipDecision,
    RuntimeOwnershipMarker,
};
use jade_symphony::profiles::{discover_execution_profiles, selected_execution_profile};
use jade_symphony::prompt::render_prompt;
use jade_symphony::quality_gate::{
    evaluate_issue_with_llm_gate, evaluate_issue_with_source_alignment, LlmGateMode, LlmGateOptions,
};
use jade_symphony::review::{
    classify_review_freshness, render_review_freshness_workpad, render_review_workpad,
    review_gate_decision, review_run_eligibility, transition_allowed_for_main_agent,
    transition_allowed_for_review_agent, write_review_job_ledger_record, FakeReviewBackend,
    FakeReviewOutcome, GeminiCliReviewBackend, ReviewBackend, ReviewFreshnessInput, ReviewJob,
    ReviewRequest, ReviewReworkClass, ReviewRunEligibility, ReviewStaleReason,
};
use jade_symphony::rework::{
    render_rework_diagnostic_workpad, rework_diagnostic_from_review, rework_transition_expected,
    ReworkDiagnostic,
};
use jade_symphony::runtime_state::{
    clear_runtime_state, detect_runtime_stall, load_runtime_state, mark_runtime_state_updated,
    record_runtime_retry, runtime_state_path, save_runtime_state, RuntimeIssueState,
    RuntimeRetryState, RuntimeStallState, RuntimeState, RuntimeTransition,
};
use jade_symphony::status_surface::render_snapshot;
use jade_symphony::tracker::{
    adapter_from_config, claim_decision, ClaimDecision, FollowUpIssueInput, ProjectFieldAssignment,
    TrackerAdapter, TrackerError,
};
use jade_symphony::workflow::WorkflowDefinition;
use jade_symphony::workspace::{
    apply_local_git_identity, prepare_workspace, profile_scoped_identifier, remove_issue_workspace,
    run_after_run, run_before_run, run_workspace_command, GitIdentityApplyResult,
};

const DEFAULT_RUN_LOOP_BASE_BRANCH: &str = "main";

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = Command::parse(args)?;

    match command {
        Command::Plan {
            workflow_path,
            json,
        } => plan(workflow_path, json),
        Command::StatusApi {
            workflow_path,
            bind,
            once,
        } => status_api(workflow_path, bind, once),
        Command::Validate { workflow_path } => validate(workflow_path),
        Command::Inspect { workflow_path } => inspect(workflow_path),
        Command::Doctor { options } => doctor(options),
        Command::DoctorRepairHumanReview {
            workflow_path,
            write,
        } => doctor_repair_human_review(workflow_path, write),
        Command::Profiles { workflow_path } => list_profiles(workflow_path),
        Command::DogfoodSmoke {
            workflow_path,
            write,
        } => dogfood_smoke(workflow_path, write),
        Command::RunOnce { workflow_path } => run_once(workflow_path),
        Command::RunLoop { options } => run_loop(options),
        Command::CleanupWorkspaces {
            workflow_path,
            write,
        } => cleanup_workspaces(workflow_path, write),
        Command::MergeOnce {
            workflow_path,
            write,
        } => merge_once(workflow_path, write),
        Command::MergeLoop { options } => merge_loop(options),
        Command::SetState {
            workflow_path,
            issue_ref,
            state,
            write,
        } => set_state(workflow_path, issue_ref, state, write),
        Command::Workpad {
            workflow_path,
            issue_ref,
            markdown_path,
            write,
        } => upsert_workpad(workflow_path, issue_ref, markdown_path, write),
        Command::CreateFollowUp {
            workflow_path,
            title,
            body_path,
            write,
        } => create_follow_up(workflow_path, title, body_path, write),
        Command::AddToProject {
            workflow_path,
            issue_id,
            write,
        } => add_to_project(workflow_path, issue_id, write),
        Command::ReviewFake {
            workflow_path,
            issue_ref,
            outcome,
            write,
        } => review_fake(workflow_path, issue_ref, outcome, write),
        Command::ReviewOnce {
            workflow_path,
            issue_ref,
            write,
        } => review_once(workflow_path, issue_ref, write),
        Command::ReviewFreshness { input } => review_freshness(input),
        Command::ReviewLoop { options } => review_loop(options),
        Command::Gate {
            workflow_path,
            issue_ref,
            apply,
            write,
        } => quality_gate(workflow_path, issue_ref, apply, write),
        Command::ForgeDiscover { source } => {
            for (index, candidate) in discover_candidates(&source).iter().enumerate() {
                println!(
                    "{}. {:?}: {}",
                    index + 1,
                    candidate.classification,
                    candidate.title
                );
                println!("   {}", candidate.rationale);
            }
            Ok(())
        }
        Command::ForgeDiscuss { title, markdown } => {
            let report = validate_markdown(&title, &markdown);
            if let Some(question) = report.question {
                println!("question={}", question.question);
                println!("why={}", question.why_it_matters);
            } else {
                println!("question=none");
                println!("gate={:?}", report.decision.kind);
            }
            Ok(())
        }
        Command::ForgeDraft { title, goal } => {
            println!("{}", draft_from_template(&title, &goal));
            Ok(())
        }
        Command::ForgeValidate { title, markdown } => {
            let report = validate_markdown(&title, &markdown);
            print_forge_validation(&report);
            Ok(())
        }
        Command::ForgeRepair { title, markdown } => {
            let report = repair_markdown(&title, &markdown);
            print_forge_validation(&report.validation);
            println!("\n--- repaired draft ---\n");
            println!("{}", report.repaired_markdown);
            Ok(())
        }
        Command::ForgeCreate {
            workflow_path,
            title,
            markdown,
            add_to_project,
            project_fields,
            write,
        } => forge_create(
            workflow_path,
            title,
            markdown,
            add_to_project,
            project_fields,
            write,
        ),
        Command::ForgeInteractive { options } => forge_interactive(options),
        Command::ForgeReflect {
            context,
            skill,
            limit,
        } => forge_reflect(context, skill, limit),
        Command::Help => {
            println!("{}", usage());
            Ok(())
        }
    }
}

fn plan(workflow_path: PathBuf, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = build_plan_snapshot(&workflow_path)?;
    println!("{}", render_plan_snapshot(&snapshot, json)?);

    Ok(())
}

fn status_api(
    workflow_path: PathBuf,
    bind: SocketAddr,
    once: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !once {
        return Err("status-api currently requires --once".into());
    }
    if !bind.ip().is_loopback() {
        return Err("status-api bind address must be loopback for this first slice".into());
    }

    let snapshot = build_plan_snapshot(&workflow_path)?;
    println!("status_api=serving bind={bind} mode=once");
    let local_addr = serve_once(bind, &snapshot)?;
    println!("status_api=stopped bind={local_addr} mode=once");
    Ok(())
}

fn build_plan_snapshot(
    workflow_path: &Path,
) -> Result<jade_symphony::model::RuntimeSnapshot, Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let integration_gaps = adapter.integration_gaps();
    let issues = adapter.list_dispatchable_issues()?;
    let event_log_path = config
        .observability
        .logs_root
        .join("jade-symphony.jsonl")
        .display()
        .to_string();
    let orchestrator = Orchestrator::new(config);
    let mut plan = orchestrator.plan_dispatch(issues);
    plan.integration_gaps.extend(integration_gaps);
    plan.snapshot.integration_gaps = plan.integration_gaps.clone();
    plan.snapshot.event_log_path = Some(event_log_path);
    Ok(plan.snapshot)
}

fn render_plan_snapshot(
    snapshot: &jade_symphony::model::RuntimeSnapshot,
    json: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    if json {
        Ok(serde_json::to_string_pretty(snapshot)?)
    } else {
        Ok(render_snapshot(snapshot))
    }
}

fn quality_gate(
    workflow_path: PathBuf,
    issue_ref: String,
    apply: bool,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let decision = evaluate_issue_for_current_source(&config, &issue)?;

    println!(
        "gate={:?} dispatchable={}",
        decision.kind,
        decision.is_dispatchable()
    );
    if !decision.missing.is_empty() {
        println!("missing={}", decision.missing.join(", "));
    }
    if !decision.assumptions.is_empty() {
        println!("assumptions={}", decision.assumptions.join("; "));
    }

    if apply {
        require_write_intent(write)?;
        let workpad = gate_workpad(&issue, &decision);
        adapter.upsert_workpad(&issue_ref, &workpad)?;
        if !decision.is_dispatchable() {
            let target_state = gate_target_state(&decision);
            adapter.set_state(&issue_ref, target_state)?;
            println!("applied=true target_state={target_state}");
        } else {
            println!("applied=true target_state=unchanged");
        }
    }

    Ok(())
}

fn evaluate_issue_for_current_source(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<GateDecision, Box<dyn std::error::Error>> {
    let repo_root = std::env::current_dir()?;
    let expected_target = expected_target_repository(config);
    let deterministic =
        evaluate_issue_with_source_alignment(issue, &repo_root, expected_target.as_deref());
    if let Some(blocker) = live_missing_assignee_gate_blocker(config, issue) {
        return Ok(GateDecision {
            kind: GateDecisionKind::NeedToClarify,
            missing: vec![blocker],
            assumptions: deterministic.assumptions,
            notes: vec!["Live GitHub dispatch requires explicit issue ownership.".into()],
        });
    }
    Ok(evaluate_issue_with_llm_gate(
        issue,
        deterministic,
        &LlmGateOptions {
            mode: LlmGateMode::parse(&config.quality_gate.llm.mode),
            command: config.quality_gate.llm.command.clone(),
            timeout_ms: config.quality_gate.llm.timeout_ms,
        },
    ))
}

fn live_missing_assignee_gate_blocker(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Option<String> {
    (live_github_tracker(config)
        && !config.tracker.assignee_filter.allow_unassigned
        && issue.assignees.is_empty())
    .then(|| "live GitHub issue assignee".into())
}

fn expected_target_repository(config: &RuntimeConfig) -> Option<String> {
    Some(format!(
        "{}/{}",
        config.tracker.owner.as_ref()?,
        config.tracker.repo.as_ref()?
    ))
}

fn set_state(
    workflow_path: PathBuf,
    issue_ref: String,
    state: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    if !transition_allowed_for_main_agent(&normalize_state(&state)) {
        return Err("main implementation agent cannot set Human Review".into());
    }
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    adapter.set_state(&issue_ref, &state)?;
    println!("set_state=ok issue_ref={issue_ref} state={state}");
    Ok(())
}

fn upsert_workpad(
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let markdown = std::fs::read_to_string(&markdown_path)?;
    adapter.upsert_workpad(&issue_ref, &markdown)?;
    println!(
        "workpad=ok issue_ref={} source={}",
        issue_ref,
        markdown_path.display()
    );
    Ok(())
}

fn create_follow_up(
    workflow_path: PathBuf,
    title: String,
    body_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let body = std::fs::read_to_string(&body_path)?;
    let issue_id = adapter.create_follow_up_issue(FollowUpIssueInput {
        title,
        body,
        project_id: None,
        related_issue_ref: None,
        blocked_by_issue_ref: None,
    })?;
    println!("create_follow_up=ok issue_id={issue_id}");
    Ok(())
}

fn forge_create(
    workflow_path: PathBuf,
    title: String,
    markdown: String,
    add_to_project: bool,
    project_fields: Vec<ProjectFieldAssignment>,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    if !project_fields.is_empty() && !add_to_project {
        return Err("forge-create --project-field requires --add-to-project".into());
    }
    let config = load_config(&workflow_path)?;
    let report =
        validate_forge_create_contract(&title, &markdown, &config).inspect_err(|_message| {
            let report = validate_forge_create_report(&title, &markdown, &config)
                .unwrap_or_else(|_| validate_markdown(&title, &markdown));
            print_forge_validation(&report);
        })?;

    let adapter = adapter_from_config(&config);
    let issue_id = adapter.create_follow_up_issue(FollowUpIssueInput {
        title: report.title,
        body: markdown,
        project_id: None,
        related_issue_ref: None,
        blocked_by_issue_ref: None,
    })?;

    if add_to_project {
        adapter.add_issue_to_project(&issue_id)?;
        for assignment in &project_fields {
            adapter.set_project_field(&issue_id, assignment)?;
        }
    }

    println!(
        "forge_create=ok issue_id={issue_id} added_to_project={add_to_project} project_fields={}",
        project_fields.len()
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgeInteractiveOptions {
    workflow_path: Option<PathBuf>,
    title: String,
    intent: String,
    skill: Option<String>,
    context: Option<String>,
    add_to_project: bool,
    write: bool,
    confirm_create: bool,
}

fn forge_interactive(options: ForgeInteractiveOptions) -> Result<(), Box<dyn std::error::Error>> {
    let report = interactive_forge(InteractiveForgeInput {
        title: options.title.clone(),
        intent: options.intent,
        skill: options.skill,
        context: options.context,
    });
    print_interactive_forge_report(&report);

    if options.write {
        if !options.confirm_create {
            return Err("forge-interactive --write requires --confirm-create".into());
        }
        let workflow_path = options
            .workflow_path
            .ok_or("forge-interactive --write requires --workflow")?;
        forge_create(
            workflow_path,
            options.title,
            report.issue_markdown,
            options.add_to_project,
            Vec::new(),
            true,
        )?;
    }

    Ok(())
}

fn forge_reflect(
    context: String,
    skill: Option<String>,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if limit == 0 {
        return Err("forge-reflect --limit must be greater than 0".into());
    }

    let candidates = reflective_candidates_from_context(&context, skill.as_deref(), limit);
    println!("candidates={}", candidates.len());
    for (index, candidate) in candidates.iter().enumerate() {
        println!("{}. {}", index + 1, candidate.title);
        println!("skill={}", candidate.skill.key);
        println!("gate={:?}", candidate.validation.decision.kind);
        println!(
            "dispatchable={}",
            candidate.validation.decision.is_dispatchable()
        );
        println!("rationale={}", candidate.rationale);
        println!("--- issue draft ---");
        println!("{}", candidate.issue_markdown);
    }

    Ok(())
}

fn validate_forge_create_contract(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, String> {
    let report = validate_forge_create_report(title, markdown, config)
        .map_err(|error| format!("source alignment failed: {error}"))?;
    if report.decision.is_dispatchable() {
        Ok(report)
    } else {
        Err("issue forge validation failed; tracker issue was not created".into())
    }
}

fn validate_forge_create_report(
    title: &str,
    markdown: &str,
    config: &RuntimeConfig,
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, Box<dyn std::error::Error>> {
    let issue = TrackerIssue {
        tracker_kind: config.tracker.kind.clone(),
        id: "forge-draft".into(),
        item_id: None,
        identifier: "#draft".into(),
        title: title.into(),
        description: Some(markdown.into()),
        url: None,
        state: config.tracker.state_map.todo.clone(),
        labels: Vec::new(),
        assignees: Vec::new(),
        priority: None,
        branch_name: None,
        linked_pull_requests: Vec::new(),
        blocked_by: Vec::new(),
        project_fields: Default::default(),
        created_at: None,
        updated_at: None,
    };
    let decision = evaluate_issue_for_current_source(config, &issue)?;
    Ok(jade_symphony::issue_forge::ForgeValidationReport {
        title: title.to_string(),
        question: next_clarification_question(&decision),
        decision,
    })
}

fn add_to_project(
    workflow_path: PathBuf,
    issue_id: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    adapter.add_issue_to_project(&issue_id)?;
    println!("add_to_project=ok issue_id={issue_id}");
    Ok(())
}

fn review_fake(
    workflow_path: PathBuf,
    issue_ref: String,
    outcome: FakeReviewOutcome,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: format!(
            "Review {} {}\n\n{}",
            issue.identifier,
            issue.title,
            issue.description.as_deref().unwrap_or_default()
        ),
        workspace: config.workspace.root.clone(),
        artifact_root: config.observability.logs_root.join("reviews"),
    };
    let backend = FakeReviewBackend::new(outcome);
    let job = backend.poll(backend.start(request)?)?;
    apply_review_result(adapter.as_ref(), &issue_ref, &issue, &job)?;

    let decision = review_gate_decision(&job);
    println!(
        "review_fake=ok issue_ref={issue_ref} outcome={:?} target_state={:?}",
        decision.outcome, decision.target_state
    );
    println!("{}", decision.message);
    Ok(())
}

fn review_once(
    workflow_path: PathBuf,
    issue_ref: String,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issue = adapter
        .get_issue(&issue_ref)?
        .ok_or_else(|| format!("issue not found: {issue_ref}"))?;
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: format!(
            "Review {} {}\n\n{}",
            issue.identifier,
            issue.title,
            issue.description.as_deref().unwrap_or_default()
        ),
        workspace: config.workspace.root.clone(),
        artifact_root: config.observability.logs_root.join("reviews"),
    };
    let job = match config.review.backend.as_str() {
        "gemini-cli" => {
            let backend = GeminiCliReviewBackend::new(config.review.gemini_command.clone());
            match backend.start(request) {
                Ok(job) => backend.poll(job)?,
                Err(error) => ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    error.to_string(),
                ),
            }
        }
        _ => {
            let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
            backend.poll(backend.start(request)?)?
        }
    };
    apply_review_result(adapter.as_ref(), &issue_ref, &issue, &job)?;

    let decision = review_gate_decision(&job);
    println!(
        "review_once=ok issue_ref={issue_ref} backend={} outcome={:?} target_state={:?}",
        job.backend, decision.outcome, decision.target_state
    );
    println!("{}", decision.message);
    Ok(())
}

fn review_freshness(input: ReviewFreshnessInput) -> Result<(), Box<dyn std::error::Error>> {
    let report = classify_review_freshness(input);
    println!("review_freshness={:?}", report.decision.kind);
    println!(
        "prior_human_review_valid={}",
        report.decision.prior_human_review_valid
    );
    println!(
        "human_rereview_required={}",
        report.decision.human_rereview_required
    );
    println!(
        "main_agent_target_state={}",
        report.decision.main_agent_target_state
    );
    println!(
        "authorized_next_state={}",
        report
            .decision
            .authorized_next_state
            .as_deref()
            .unwrap_or("none")
    );
    println!("rationale={}", report.decision.rationale);
    println!("\n--- workpad evidence ---\n");
    println!("{}", render_review_freshness_workpad(&report));
    Ok(())
}

fn review_loop(options: ReviewLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let mut iterations = 0usize;

    loop {
        if let Some(max) = limit {
            if iterations >= max {
                println!("review_loop=stopped reason=max_iterations iterations={iterations}");
                break;
            }
        }

        iterations += 1;
        let config = load_config(&options.workflow_path)?;
        let adapter = adapter_from_config(&config);
        let issues = adapter
            .fetch_issues_by_states(std::slice::from_ref(&config.tracker.state_map.agent_review))?;

        if issues.is_empty() {
            println!("review_loop=stopped reason=no_agent_review_issue iterations={iterations}");
            break;
        };

        let backend_kind = review_backend_kind(&config, options.fake_outcome.as_ref());
        let selected = select_review_worker_issues(
            &issues,
            &config.tracker.state_map.agent_review,
            &backend_kind,
            options.worker_limit(&config),
        );

        if selected.is_empty() {
            for issue in issues {
                match review_run_eligibility(
                    &issue,
                    &config.tracker.state_map.agent_review,
                    &backend_kind,
                ) {
                    ReviewRunEligibility::AlreadyQueued { worker_key } => {
                        println!(
                            "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                            issue.identifier
                        );
                    }
                    ReviewRunEligibility::NotInAgentReview { current_state } => {
                        println!(
                            "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                            issue.identifier
                        );
                    }
                    ReviewRunEligibility::Eligible { .. } => {}
                }
            }
            continue;
        }

        for (slot, selected_issue) in selected.into_iter().enumerate() {
            let worker_slot = slot + 1;
            match review_run_eligibility(
                &selected_issue,
                &config.tracker.state_map.agent_review,
                &backend_kind,
            ) {
                ReviewRunEligibility::Eligible { worker_key } => {
                    println!(
                    "review_loop_iteration={iterations} worker_slot={worker_slot} issue={} worker_key={worker_key} mode={}",
                    selected_issue.identifier,
                    if options.write { "write" } else { "dry-run" }
                );
                    if !options.write {
                        println!(
                            "review_loop_dry_run action=start issue={} backend={backend_kind}",
                            selected_issue.identifier
                        );
                        println!(
                            "review_loop_dry_run action=workpad issue={} evidence=review_job",
                            selected_issue.identifier
                        );
                        println!(
                        "review_loop_dry_run action=reconcile issue={} actor=independent_review_agent",
                        selected_issue.identifier
                    );
                        continue;
                    }

                    let latest =
                        adapter
                            .get_issue(&selected_issue.identifier)?
                            .ok_or_else(|| {
                                format!(
                                    "issue disappeared before review: {}",
                                    selected_issue.identifier
                                )
                            })?;
                    match review_run_eligibility(
                        &latest,
                        &config.tracker.state_map.agent_review,
                        &backend_kind,
                    ) {
                        ReviewRunEligibility::Eligible { .. } => {
                            let mut job =
                                run_review_job(&config, &latest, options.fake_outcome.clone())?;
                            let ledger_path = write_review_job_ledger_record(
                                &config.observability.logs_root,
                                &latest,
                                &job,
                            )?;
                            job.ledger_path = Some(ledger_path.clone());
                            apply_review_result(
                                adapter.as_ref(),
                                &latest.identifier,
                                &latest,
                                &job,
                            )?;
                            let decision = review_gate_decision(&job);
                            println!(
                            "review_loop_action=reconciled issue={} backend={} outcome={:?} target_state={:?} ledger={}",
                            latest.identifier,
                            job.backend,
                            decision.outcome,
                            decision.target_state,
                            ledger_path.display()
                        );
                        }
                        ReviewRunEligibility::AlreadyQueued { worker_key } => {
                            println!(
                            "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                            latest.identifier
                        );
                        }
                        ReviewRunEligibility::NotInAgentReview { current_state } => {
                            println!(
                            "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                            latest.identifier
                        );
                        }
                    }
                }
                ReviewRunEligibility::AlreadyQueued { worker_key } => {
                    println!(
                    "review_loop_action=skip issue={} reason=review_worker_exists worker_key={worker_key}",
                    selected_issue.identifier
                );
                }
                ReviewRunEligibility::NotInAgentReview { current_state } => {
                    println!(
                    "review_loop_action=skip issue={} reason=state_changed current_state={current_state:?}",
                    selected_issue.identifier
                );
                }
            }
        }

        if !options.write && limit.is_none() {
            println!(
                "review_loop=stopped reason=dry_run_would_repeat_without_mutation iterations={iterations}"
            );
            break;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeOnceOutcome {
    NoMergingIssue,
    DryRun,
    Merged,
    Routed,
    Skipped,
}

fn merge_once(workflow_path: PathBuf, write: bool) -> Result<(), Box<dyn std::error::Error>> {
    merge_once_tick(workflow_path, write).map(|_| ())
}

fn merge_loop(options: MergeLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let max = options
        .iteration_limit()
        .ok_or("merge-loop requires --max-iterations or --once")?;
    let mut stopped = false;

    for iteration in 1..=max {
        println!(
            "merge_loop_iteration={} mode={}",
            iteration,
            if options.write { "write" } else { "dry-run" }
        );
        match merge_once_tick(options.workflow_path.clone(), options.write)? {
            MergeOnceOutcome::NoMergingIssue => {
                println!("merge_loop=stopped reason=no_merging_issue iterations={iteration}");
                stopped = true;
                break;
            }
            MergeOnceOutcome::DryRun if !options.write => {
                println!("merge_loop_action=dry_run_tick iterations={iteration}");
            }
            MergeOnceOutcome::Merged => {
                println!("merge_loop_action=merged iterations={iteration}");
            }
            MergeOnceOutcome::Routed => {
                println!("merge_loop_action=routed iterations={iteration}");
            }
            MergeOnceOutcome::Skipped => {
                println!("merge_loop_action=skipped iterations={iteration}");
            }
            MergeOnceOutcome::DryRun => {}
        }
    }

    if !stopped {
        println!("merge_loop=stopped reason=max_iterations iterations={max}");
    }

    Ok(())
}

fn merge_once_tick(
    workflow_path: PathBuf,
    write: bool,
) -> Result<MergeOnceOutcome, Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let merging_state = config.tracker.state_map.merging.clone();
    let mut issues = adapter.fetch_issues_by_states(std::slice::from_ref(&merging_state))?;
    if issues.is_empty() {
        println!("merge_once=stopped reason=no_merging_issue");
        return Ok(MergeOnceOutcome::NoMergingIssue);
    }

    issues.sort_by_key(|issue| issue.priority.unwrap_or(i64::MAX));
    let selected = issues.remove(0);
    let issue = adapter
        .get_issue(&selected.identifier)?
        .unwrap_or(selected.clone());
    let linked_pull_requests = adapter.list_linked_pull_requests(&issue.identifier)?;
    let runner = ProcessHandoffCommandRunner;
    let expected_base = expected_merge_base_branch(&config);
    let status = merge_preflight_status(&config, &issue, &linked_pull_requests, &runner)?;
    let decision = merge_lane_decision(
        &issue,
        &merging_state,
        expected_base,
        &linked_pull_requests,
        status.as_ref(),
    );

    println!(
        "merge_once issue={} decision={:?} target_state={} write={}",
        issue.identifier,
        decision.kind,
        decision.target_state.unwrap_or("none"),
        write
    );
    println!("reason={}", decision.reason);
    if let Some(pr_url) = decision.pr_url.as_deref() {
        println!("pull_request={pr_url}");
    }

    if !write {
        print_merge_dry_run_actions(&decision);
        return Ok(MergeOnceOutcome::DryRun);
    }

    if decision.kind.is_merge_ready() {
        let pr_ref = decision
            .pr_url
            .as_deref()
            .ok_or("merge-ready decision missing pull request URL")?;
        let output = merge_pull_request(pr_ref, &runner, &std::env::current_dir()?)?;
        let workpad = merge_lane_workpad(&issue, &decision, Some(&output));
        record_done_merge_lane_completion(adapter.as_ref(), &issue, &workpad)?;
        println!(
            "merge_once_action=merged issue={} target_state=done",
            issue.identifier
        );
        return Ok(MergeOnceOutcome::Merged);
    }

    let workpad = merge_lane_workpad(&issue, &decision, None);
    adapter.upsert_workpad(&issue.identifier, &workpad)?;
    if let Some(target_state) = decision.target_state {
        adapter.set_state(&issue.identifier, target_state)?;
        if decision.kind == MergeLaneDecisionKind::AlreadyMerged
            && normalize_state(target_state) == "done"
        {
            close_completed_issue(adapter.as_ref(), &issue.identifier)?;
        }
        println!(
            "merge_once_action=routed issue={} target_state={target_state}",
            issue.identifier
        );
        return Ok(MergeOnceOutcome::Routed);
    } else {
        println!("merge_once_action=skipped issue={}", issue.identifier);
    }

    Ok(MergeOnceOutcome::Skipped)
}

fn record_done_merge_lane_completion(
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    workpad: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    adapter.upsert_workpad(&issue.identifier, workpad)?;
    adapter.set_state(&issue.identifier, "done")?;
    close_completed_issue(adapter, &issue.identifier)?;
    Ok(())
}

fn close_completed_issue(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match adapter.close_issue(issue_ref) {
        Ok(()) => {
            println!("merge_once_action=closed_issue issue={issue_ref}");
            Ok(())
        }
        Err(TrackerError::NotImplemented(message)) => {
            eprintln!("merge_once_warning=issue_close_unavailable reason={message}");
            Ok(())
        }
        Err(TrackerError::IntegrationUnavailable(message)) => {
            eprintln!("merge_once_warning=issue_close_unavailable reason={message}");
            Ok(())
        }
        Err(error) => Err(Box::new(error)),
    }
}

fn merge_preflight_status(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    linked_pull_requests: &[jade_symphony::model::LinkedPullRequest],
    runner: &ProcessHandoffCommandRunner,
) -> Result<Option<jade_symphony::merge_lane::PullRequestMergeStatus>, Box<dyn std::error::Error>> {
    if linked_pull_requests.len() != 1 {
        return Ok(None);
    }

    let linked = &linked_pull_requests[0];
    let number_ref = linked.number.map(|number| number.to_string());
    let Some(pr_ref) = linked.url.as_deref().or(number_ref.as_deref()) else {
        return Ok(None);
    };

    if config.tracker.fixture_path.is_some() || issue.tracker_kind == "memory" {
        return Ok(pull_request_status_from_linked(linked));
    }

    match fetch_pull_request_status(pr_ref, runner, &std::env::current_dir()?) {
        Ok(status) => Ok(Some(status)),
        Err(error) => {
            eprintln!("merge_preflight_warning={error}");
            Ok(None)
        }
    }
}

fn print_merge_dry_run_actions(decision: &jade_symphony::merge_lane::MergeLaneDecision) {
    match decision.kind {
        MergeLaneDecisionKind::ReadyToMerge => {
            println!("merge_once_dry_run action=merge");
            println!("merge_once_dry_run action=workpad evidence=merge_result");
            println!("merge_once_dry_run action=set_state target_state=done");
            println!("merge_once_dry_run action=close_issue");
        }
        MergeLaneDecisionKind::AlreadyMerged => {
            println!("merge_once_dry_run action=workpad evidence=already_merged");
            println!("merge_once_dry_run action=set_state target_state=done");
            println!("merge_once_dry_run action=close_issue");
        }
        _ => {
            println!("merge_once_dry_run action=workpad evidence=preflight_blocker");
            if let Some(target_state) = decision.target_state {
                println!("merge_once_dry_run action=set_state target_state={target_state}");
            }
        }
    }
}

fn review_backend_kind(config: &RuntimeConfig, fake_outcome: Option<&FakeReviewOutcome>) -> String {
    if fake_outcome.is_some() {
        "fake-reviewer".into()
    } else if config.review.backend == "gemini-cli" {
        "gemini-cli".into()
    } else {
        "fake-reviewer".into()
    }
}

fn select_review_worker_issues(
    issues: &[TrackerIssue],
    agent_review_state: &str,
    backend_kind: &str,
    max_concurrent: usize,
) -> Vec<TrackerIssue> {
    issues
        .iter()
        .filter(|issue| {
            matches!(
                review_run_eligibility(issue, agent_review_state, backend_kind),
                ReviewRunEligibility::Eligible { .. }
            )
        })
        .take(max_concurrent.max(1))
        .cloned()
        .collect()
}

fn run_review_job(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    fake_outcome: Option<FakeReviewOutcome>,
) -> Result<ReviewJob, Box<dyn std::error::Error>> {
    let request = ReviewRequest {
        issue: issue.clone(),
        prompt: format!(
            "Review {} {}\n\n{}",
            issue.identifier,
            issue.title,
            issue.description.as_deref().unwrap_or_default()
        ),
        workspace: review_workspace_for_issue(config, issue),
        artifact_root: config.observability.logs_root.join("reviews"),
    };

    if let Some(outcome) = fake_outcome {
        let backend = FakeReviewBackend::new(outcome);
        return Ok(backend.poll(backend.start(request)?)?);
    }

    match config.review.backend.as_str() {
        "gemini-cli" => {
            let backend = GeminiCliReviewBackend::new(config.review.gemini_command.clone());
            match backend.start(request) {
                Ok(job) => Ok(backend.poll(job)?),
                Err(error) => Ok(ReviewJob::failed_unavailable(
                    issue.identifier.clone(),
                    "gemini-cli",
                    error.to_string(),
                )),
            }
        }
        _ => {
            let backend = FakeReviewBackend::new(FakeReviewOutcome::Pass);
            Ok(backend.poll(backend.start(request)?)?)
        }
    }
}

fn review_workspace_for_issue(config: &RuntimeConfig, issue: &TrackerIssue) -> PathBuf {
    run_loop_handoff_plan(config, issue)
        .map(|handoff| handoff.workspace_path)
        .unwrap_or_else(|_| config.workspace.root.clone())
}

fn apply_review_result(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    issue: &TrackerIssue,
    job: &jade_symphony::review::ReviewJob,
) -> Result<(), Box<dyn std::error::Error>> {
    let decision = review_gate_decision(job);
    if let Some(target_state) = decision.target_state {
        if !transition_allowed_for_review_agent(target_state, &decision) {
            return Err("review agent transition is not allowed for this review decision".into());
        }
        if rework_transition_expected(&decision) {
            let diagnostic = rework_diagnostic_from_review(issue, job, &decision);
            transition_issue_to_rework_with_diagnostic(adapter, issue, &diagnostic)?;
            return Ok(());
        }
    }

    let workpad = render_review_workpad(issue, job);
    adapter.upsert_workpad(issue_ref, &workpad)?;
    if let Some(target_state) = decision.target_state {
        adapter.set_state(issue_ref, target_state)?;
    }
    Ok(())
}

fn transition_issue_to_rework_with_diagnostic(
    adapter: &dyn TrackerAdapter,
    issue: &TrackerIssue,
    diagnostic: &ReworkDiagnostic,
) -> Result<(), Box<dyn std::error::Error>> {
    let workpad = render_rework_diagnostic_workpad(issue, diagnostic);
    adapter.upsert_workpad(&issue.identifier, &workpad)?;
    adapter.set_state(&issue.identifier, "rework")?;
    Ok(())
}

fn require_write_intent(write: bool) -> Result<(), Box<dyn std::error::Error>> {
    if write {
        Ok(())
    } else {
        Err("live write command requires explicit --write".into())
    }
}

fn load_config(workflow_path: &Path) -> Result<RuntimeConfig, Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, workflow_path)?;
    config.validate()?;
    Ok(config)
}

fn validate(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    println!("workflow={}", workflow_path.display());
    println!("tracker={}", config.tracker.kind);
    println!("backend={}", config.backend.kind);
    println!("workspace_root={}", config.workspace.root.display());
    println!("prompt_template_bytes={}", workflow.prompt_template.len());
    println!("status=valid");
    Ok(())
}

fn inspect(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.list_dispatchable_issues()?;

    println!("issues={}", issues.len());
    for issue in issues {
        let gate = evaluate_issue_for_current_source(&config, &issue)?;
        println!(
            "- {} {} state={} gate={:?}",
            issue.identifier, issue.title, issue.state, gate.kind
        );
        if !gate.missing.is_empty() {
            println!("  missing={}", gate.missing.join(", "));
        }
        if !gate.assumptions.is_empty() {
            println!("  assumptions={}", gate.assumptions.join("; "));
        }
    }

    for gap in adapter.integration_gaps() {
        println!("integration_gap={gap}");
    }

    Ok(())
}

fn doctor(options: DoctorOptions) -> Result<(), Box<dyn std::error::Error>> {
    let workflow_path = options.workflow_path;
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.fetch_issues_by_states(&all_mapped_tracker_states(&config))?;
    let mut report = audit_project_issues(&issues);
    report.integration_gaps = adapter.integration_gaps();

    if options.json {
        println!("{}", render_project_audit_report_json(&report)?);
    } else {
        println!("{}", render_project_audit_report(&report));
    }

    if options.strict && report.blocker_count() > 0 {
        return Err(format!(
            "project doctor strict mode found {} blocker violation(s)",
            report.blocker_count()
        )
        .into());
    }

    Ok(())
}

fn doctor_repair_human_review(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.list_dispatchable_issues()?;
    let report = audit_project_issues(&issues);
    let candidates = human_review_repair_candidates(&report);

    println!(
        "doctor_repair_human_review candidates={} write={write}",
        candidates.len()
    );
    for violation in candidates {
        println!(
            "doctor_repair_human_review action=move issue={} from={:?} to=agent_review",
            violation.issue_ref, violation.state
        );
        if write {
            let workpad = render_human_review_repair_workpad(violation);
            adapter.upsert_workpad(&violation.issue_ref, &workpad)?;
            adapter.set_state(&violation.issue_ref, "agent_review")?;
        } else {
            println!(
                "doctor_repair_human_review_dry_run action=workpad issue={} evidence=human_review_missing_review_evidence",
                violation.issue_ref
            );
            println!(
                "doctor_repair_human_review_dry_run action=set_state issue={} target_state=agent_review",
                violation.issue_ref
            );
        }
    }

    Ok(())
}

fn all_mapped_tracker_states(config: &RuntimeConfig) -> Vec<String> {
    let state_map = &config.tracker.state_map;
    vec![
        state_map.backlog.clone(),
        state_map.todo.clone(),
        state_map.need_to_clarify.clone(),
        state_map.in_progress.clone(),
        state_map.need_human_input.clone(),
        state_map.agent_review.clone(),
        state_map.human_review.clone(),
        state_map.rework.clone(),
        state_map.merging.clone(),
        state_map.done.clone(),
    ]
}

fn list_profiles(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let profiles = discover_execution_profiles(&config.profiles)?;
    let selected = selected_execution_profile(&config.profiles)?;

    println!("profiles={}", profiles.len());
    if let Some(profile) = selected {
        println!("selected_profile={}", profile.profile_id);
        println!("selected_instance={}", profile.instance_name);
    }
    for profile in profiles {
        println!(
            "- profile_id={} instance_name={} source={} workspace_namespace={} backend={}",
            profile.profile_id,
            profile.instance_name,
            profile.source,
            profile.workspace_namespace,
            profile.backend.as_deref().unwrap_or("configured")
        );
    }
    Ok(())
}

fn dogfood_smoke(workflow_path: PathBuf, write: bool) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let integration_gaps = adapter.integration_gaps();
    let issues = adapter.list_dispatchable_issues()?;
    let controlled_candidates: Vec<_> = issues
        .iter()
        .filter(|issue| is_controlled_dogfood_smoke_issue(issue))
        .collect();
    let executable_candidates = controlled_candidates
        .iter()
        .filter(|issue| {
            evaluate_issue_for_current_source(&config, issue)
                .map(|decision| decision.is_dispatchable())
                .unwrap_or(false)
        })
        .count();
    let fixture_mode = config.tracker.fixture_path.is_some();
    let write_ready =
        !fixture_mode && integration_gaps.is_empty() && executable_candidates == 1 && write;

    println!("dogfood_smoke=ok");
    println!("workflow={}", workflow_path.display());
    println!("tracker_kind={}", config.tracker.kind);
    println!("fixture_mode={fixture_mode}");
    println!("write_requested={write}");
    println!("controlled_candidates={}", controlled_candidates.len());
    println!("executable_candidates={executable_candidates}");
    println!(
        "runtime_state_path={}",
        runtime_state_path(&config).display()
    );
    println!(
        "event_log_root={}",
        config.observability.logs_root.join("events").display()
    );
    if integration_gaps.is_empty() {
        println!("integration_gaps=none");
    } else {
        for gap in &integration_gaps {
            println!("integration_gap={gap}");
        }
    }
    println!("write_ready={write_ready}");

    if !write {
        println!("dogfood_smoke_dry_run action=inspect_project");
        println!("dogfood_smoke_dry_run action=quality_gate_controlled_issue");
        println!("dogfood_smoke_dry_run action=report_run_loop_command");
        return Ok(());
    }

    if write_ready {
        println!(
            "dogfood_smoke_next_command=cargo run -- run-loop {} --max-iterations 1 --write",
            workflow_path.display()
        );
    } else {
        println!("dogfood_smoke_blocked=true");
        println!("dogfood_smoke_blocker=requires exactly one executable controlled smoke issue, non-fixture tracker mode, and no integration gaps");
    }

    Ok(())
}

fn cleanup_workspaces(
    workflow_path: PathBuf,
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(&workflow_path)?;
    let adapter = adapter_from_config(&config);
    let issues = adapter.fetch_issues_by_states(&config.tracker.terminal_states)?;
    let entries = workspace_cleanup_plan(&config, &issues)?;
    let eligible = entries
        .iter()
        .filter(|entry| matches!(entry.action, WorkspaceCleanupAction::Eligible))
        .count();

    println!(
        "workspace_cleanup mode={} terminal_issues={} eligible={eligible}",
        if write { "write" } else { "dry-run" },
        issues.len()
    );

    for entry in &entries {
        println!(
            "workspace_cleanup issue={} state={:?} action={} workspace_key={} path={}",
            entry.issue_ref,
            entry.state,
            entry.action.label(),
            entry.workspace_key,
            entry.workspace_path.display()
        );
        if let WorkspaceCleanupAction::Skipped { reason } = &entry.action {
            println!(
                "workspace_cleanup_skip issue={} reason={}",
                entry.issue_ref, reason
            );
        }
    }

    if write {
        for entry in entries
            .iter()
            .filter(|entry| matches!(entry.action, WorkspaceCleanupAction::Eligible))
        {
            remove_issue_workspace(&config.workspace.root, &entry.workspace_key, &config.hooks)?;
            println!(
                "workspace_cleanup_removed issue={} path={}",
                entry.issue_ref,
                entry.workspace_path.display()
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceCleanupEntry {
    issue_ref: String,
    state: String,
    workspace_key: String,
    workspace_path: PathBuf,
    action: WorkspaceCleanupAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspaceCleanupAction {
    Eligible,
    Skipped { reason: String },
}

impl WorkspaceCleanupAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::Skipped { .. } => "skipped",
        }
    }
}

fn workspace_cleanup_plan(
    config: &RuntimeConfig,
    issues: &[TrackerIssue],
) -> Result<Vec<WorkspaceCleanupEntry>, Box<dyn std::error::Error>> {
    let terminal_states = config.terminal_state_set();
    let profile = selected_execution_profile(&config.profiles)?;
    let profile_namespace = profile
        .as_ref()
        .map(|profile| profile.workspace_namespace.as_str());

    let mut entries = Vec::new();
    for issue in issues {
        if !terminal_states.contains(&issue.normalized_state()) {
            entries.push(WorkspaceCleanupEntry {
                issue_ref: issue.identifier.clone(),
                state: issue.state.clone(),
                workspace_key: "n/a".into(),
                workspace_path: config.workspace.root.clone(),
                action: WorkspaceCleanupAction::Skipped {
                    reason: "non_terminal_state".into(),
                },
            });
            continue;
        }

        let plan = match plan_issue_handoff_for_profile(
            &config.workspace.root,
            issue,
            DEFAULT_RUN_LOOP_BASE_BRANCH,
            profile_namespace,
        ) {
            Ok(plan) => plan,
            Err(error) => {
                entries.push(WorkspaceCleanupEntry {
                    issue_ref: issue.identifier.clone(),
                    state: issue.state.clone(),
                    workspace_key: "n/a".into(),
                    workspace_path: config.workspace.root.clone(),
                    action: WorkspaceCleanupAction::Skipped {
                        reason: format!("handoff_plan_failed:{error}"),
                    },
                });
                continue;
            }
        };

        let action = if plan.workspace_path.exists() {
            WorkspaceCleanupAction::Eligible
        } else {
            WorkspaceCleanupAction::Skipped {
                reason: "workspace_missing".into(),
            }
        };

        entries.push(WorkspaceCleanupEntry {
            issue_ref: issue.identifier.clone(),
            state: issue.state.clone(),
            workspace_key: plan.workspace_key,
            workspace_path: plan.workspace_path,
            action,
        });
    }

    Ok(entries)
}

fn is_controlled_dogfood_smoke_issue(issue: &TrackerIssue) -> bool {
    issue
        .labels_lowercase()
        .iter()
        .any(|label| label == "dogfood-smoke" || label == "smoke")
        || issue.title.to_ascii_lowercase().contains("[dogfood-smoke]")
}

fn run_once(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
    config.validate()?;

    let adapter = adapter_from_config(&config);
    let issues = adapter.list_dispatchable_issues()?;
    let orchestrator = Orchestrator::new(config.clone());
    let plan = orchestrator.plan_dispatch(issues);
    let Some(issue) = plan.selected.first() else {
        println!("{}", render_snapshot(&plan.snapshot));
        println!("run_once=skipped reason=no_dispatchable_issue");
        return Ok(());
    };

    let result = execute_issue_once(&workflow, &config, issue)?;

    println!("run_once=completed");
    println!("issue={} {}", issue.identifier, issue.title);
    println!("workspace={}", result.workspace_path.display());
    println!("backend={}", result.backend);
    println!("actor_role={}", result.actor_role);
    println!("actor_label={}", result.actor_label);
    println!(
        "git_author={}",
        result.git_author.as_deref().unwrap_or("n/a")
    );
    println!("git_identity={}", result.git_identity.summary());
    println!("success={}", result.success);
    println!(
        "event_log={}",
        config
            .observability
            .logs_root
            .join("jade-symphony.jsonl")
            .display()
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IssueExecutionResult {
    workspace_path: PathBuf,
    backend: String,
    profile_id: Option<String>,
    instance_name: Option<String>,
    success: bool,
    session_id: Option<String>,
    message: String,
    usage_limit_pause: Option<UsageLimitPause>,
    actor_role: String,
    actor_label: String,
    git_author: Option<String>,
    git_identity: GitIdentityApplyResult,
    live_handoff: Option<RunLoopLiveHandoff>,
    handoff_verification: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunLoopLiveHandoff {
    worktree: LiveWorktreeResult,
    publication: PullRequestPublication,
    verification: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HandoffVerification {
    success: bool,
    summary: String,
}

fn execute_issue_once(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let workspace_identifier = profile_scoped_identifier(
        profile
            .as_ref()
            .map(|profile| profile.workspace_namespace.as_str()),
        &issue.identifier,
    );
    execute_issue_once_with_workspace_key(workflow, config, issue, &workspace_identifier)
}

fn execute_issue_once_with_workspace_key(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
    workspace_key: &str,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    let workspace = prepare_workspace(&config.workspace.root, workspace_key, &config.hooks)?;
    let git_identity = apply_local_git_identity(&workspace.path, &config.identity.git)?;
    run_before_run(&workspace.path, &config.hooks)?;

    let prompt = render_prompt(&workflow.prompt_template, issue, None)?;
    std::fs::write(workspace.path.join("JADE_SYMPHONY_PROMPT.md"), &prompt)?;

    let backend = backend_from_config(config);
    let prepared = backend.prepare(workspace.path.clone(), prompt, config)?;
    let events = backend.run(prepared)?;
    let summary = backend.summarize(&events);
    let usage_limit_pause = usage_limit_pause_from_events(&events);
    run_after_run(&workspace.path, &config.hooks);

    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    for event in &events {
        log.append(&EventRecord {
            event: format!("{event:?}"),
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            session_id: summary.session_id.clone(),
            profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
            instance_name: profile
                .as_ref()
                .map(|profile| profile.instance_name.clone()),
            actor_role: Some(config.identity.actor_role.clone()),
            actor_label: Some(config.identity.actor_label.clone()),
            git_author: config.identity.git.author(),
            message: summary.message.clone(),
        })?;
    }

    Ok(IssueExecutionResult {
        workspace_path: workspace.path,
        backend: summary.backend,
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        success: summary.success,
        session_id: summary.session_id,
        message: summary.message,
        usage_limit_pause,
        actor_role: config.identity.actor_role.clone(),
        actor_label: config.identity.actor_label.clone(),
        git_author: config.identity.git.author(),
        git_identity,
        live_handoff: None,
        handoff_verification: None,
    })
}

fn run_loop(options: RunLoopOptions) -> Result<(), Box<dyn std::error::Error>> {
    let limit = options.iteration_limit();
    let mut iterations = 0usize;

    loop {
        if let Some(max) = limit {
            if iterations >= max {
                println!("run_loop=stopped reason=max_iterations iterations={iterations}");
                break;
            }
        }

        iterations += 1;
        let workflow = WorkflowDefinition::load(&options.workflow_path)?;
        let config = RuntimeConfig::from_workflow(&workflow, &options.workflow_path)?;
        config.validate()?;
        let adapter = adapter_from_config(&config);
        if options.write {
            let runtime_state = load_runtime_state(&config)?;
            match run_loop_resume_preflight(
                adapter.as_ref(),
                &config,
                runtime_state.as_ref(),
                current_time_ms(),
            )? {
                ResumePreflightAction::Continue => {}
                ResumePreflightAction::ClearCompleted { issue_identifier } => {
                    clear_runtime_state(&config)?;
                    println!(
                        "run_loop_resume_preflight action=clear issue={} reason=tracker_state_terminal",
                        issue_identifier
                    );
                }
                ResumePreflightAction::RetryLater {
                    issue_identifier,
                    retry,
                    due_in_ms,
                } => {
                    append_runtime_supervision_event(
                        &config,
                        runtime_state.as_ref(),
                        "RetryDeferred",
                        &format!(
                            "issue={issue_identifier} attempt={} due_in_ms={} error={}",
                            retry.attempt, due_in_ms, retry.error
                        ),
                    )?;
                    println!(
                        "run_loop=stopped reason=retry_backoff issue={} due_in_ms={} attempt={}",
                        issue_identifier, due_in_ms, retry.attempt
                    );
                    break;
                }
                ResumePreflightAction::Stalled {
                    issue_identifier,
                    stall,
                } => {
                    append_runtime_supervision_event(
                        &config,
                        runtime_state.as_ref(),
                        "RuntimeStalled",
                        &format!(
                            "issue={issue_identifier} stalled_for_ms={} reason={}",
                            stall.stalled_for_ms, stall.reason
                        ),
                    )?;
                    println!(
                        "run_loop=stopped reason=runtime_stalled issue={} stalled_for_ms={}",
                        issue_identifier, stall.stalled_for_ms
                    );
                    break;
                }
                ResumePreflightAction::Block { reason } => {
                    append_runtime_supervision_event(
                        &config,
                        runtime_state.as_ref(),
                        "ResumeBlocked",
                        &reason,
                    )?;
                    println!("run_loop=stopped reason=resume_preflight_blocked detail={reason}");
                    break;
                }
            }
        }
        let issues = adapter.list_dispatchable_issues()?;
        let orchestrator = Orchestrator::new(config.clone());
        let mut plan = orchestrator.plan_dispatch(issues);
        plan.integration_gaps.extend(adapter.integration_gaps());
        plan.snapshot.integration_gaps = plan.integration_gaps.clone();
        plan.snapshot.event_log_path = Some(
            config
                .observability
                .logs_root
                .join("jade-symphony.jsonl")
                .display()
                .to_string(),
        );

        let Some(issue) = plan.selected.first().cloned() else {
            println!("{}", render_snapshot(&plan.snapshot));
            match no_dispatch_action(&options, limit, config.polling.interval_ms) {
                NoDispatchAction::Stop { reason } => {
                    println!("run_loop=stopped reason={reason} iterations={iterations}");
                    break;
                }
                NoDispatchAction::SleepAndContinue { delay_ms } => {
                    println!(
                        "run_loop_idle action=sleep delay_ms={delay_ms} iterations={iterations}"
                    );
                    thread::sleep(Duration::from_millis(delay_ms));
                    continue;
                }
            }
        };

        let decision = evaluate_issue_for_current_source(&config, &issue)?;
        if !decision.is_dispatchable() {
            handle_run_loop_gate_failure(adapter.as_ref(), &issue, &decision, &options)?;
            continue;
        }

        println!(
            "run_loop_iteration={} issue={} title={:?} mode={}",
            iterations,
            issue.identifier,
            issue.title,
            if options.write { "write" } else { "dry-run" }
        );

        let handoff = match run_loop_handoff_plan(&config, &issue) {
            Ok(handoff) => handoff,
            Err(error) => {
                handle_run_loop_handoff_failure(adapter.as_ref(), &issue, &error, &options)?;
                continue;
            }
        };

        if !options.write {
            print_run_loop_dry_run_actions(&issue, &handoff, &config)?;
            if limit.is_none() {
                println!(
                    "run_loop=stopped reason=dry_run_would_repeat_without_mutation iterations={iterations}"
                );
                break;
            }
            continue;
        }

        let latest = adapter
            .get_issue(&issue.identifier)?
            .ok_or_else(|| format!("issue disappeared before claim: {}", issue.identifier))?;
        let latest_gate = evaluate_issue_for_current_source(&config, &latest)?;
        if !latest_gate.is_dispatchable() {
            handle_run_loop_gate_failure(adapter.as_ref(), &latest, &latest_gate, &options)?;
            continue;
        }

        let handoff = match run_loop_handoff_plan(&config, &latest) {
            Ok(handoff) => handoff,
            Err(error) => {
                handle_run_loop_handoff_failure(adapter.as_ref(), &latest, &error, &options)?;
                continue;
            }
        };

        let profile_login = selected_profile_github_login(&config)?;
        let active_login = if live_github_tracker(&config) && profile_login.is_none() {
            current_gh_login()?
        } else {
            None
        };
        match run_loop_assignee_ownership_decision(
            &latest,
            &config,
            active_login.as_deref(),
            profile_login.as_deref(),
        ) {
            AssigneeOwnershipDecision::Allowed => {}
            AssigneeOwnershipDecision::Block { reason } => {
                let workpad = run_loop_assignee_ownership_workpad(&latest, &reason);
                adapter.upsert_workpad(&latest.identifier, &workpad)?;
                println!(
                    "run_loop_action=skip issue={} reason=assignee_ownership detail={}",
                    latest.identifier, reason
                );
                continue;
            }
        }

        let existing_runtime_state = load_runtime_state(&config)?;
        if let Some(state) = &existing_runtime_state {
            if let Some(active_issue) = &state.active_issue {
                println!(
                    "run_loop_runtime_state action=loaded active_issue={} attempt={}",
                    active_issue.identifier, state.attempt_count
                );
            }
        }

        let ownership = run_loop_runtime_ownership(&latest, &config, &handoff)?;
        let claim_action = run_loop_claim_action(&latest, &config);
        if matches!(claim_action, RunLoopClaimAction::Resume) {
            if let RuntimeOwnershipDecision::Mismatched { reason, .. } =
                runtime_ownership_decision(latest.description.as_deref(), &ownership)
            {
                println!(
                    "run_loop_action=skip issue={} reason=ownership_mismatch detail={reason}",
                    latest.identifier
                );
                continue;
            }
        }

        let event = match claim_action {
            RunLoopClaimAction::Claim => {
                adapter.set_state(&latest.identifier, "in_progress")?;
                println!(
                    "run_loop_action=claim issue={} target_state=in_progress",
                    latest.identifier
                );
                "Claimed"
            }
            RunLoopClaimAction::Resume => {
                println!("run_loop_action=resume issue={}", latest.identifier);
                "Resumed"
            }
            RunLoopClaimAction::StopAndReplan { current_state } => {
                println!(
                    "run_loop_action=skip issue={} reason=external_state_change current_state={:?}",
                    latest.identifier, current_state
                );
                continue;
            }
        };
        let ownership_workpad = run_loop_ownership_workpad(&latest, &ownership, event);
        adapter.upsert_workpad(&latest.identifier, &ownership_workpad)?;
        println!(
            "run_loop_action=ownership issue={} profile={} branch={}",
            latest.identifier,
            ownership.profile_id.as_deref().unwrap_or("n/a"),
            ownership.branch_name
        );

        let mut runtime_state = run_loop_runtime_state_for_issue(
            existing_runtime_state.as_ref(),
            &latest,
            &config,
            event,
        );
        mark_runtime_state_updated(&mut runtime_state, current_time_ms());
        save_runtime_state(&config, &runtime_state)?;
        println!(
            "run_loop_runtime_state action=saved issue={} event={event}",
            latest.identifier
        );

        let live_worktree = if run_loop_live_handoff_enabled(&config) {
            let runner = ProcessHandoffCommandRunner;
            let repo_root = std::env::current_dir()?;
            let worktree = prepare_issue_worktree(&repo_root, &handoff, &runner)?;
            println!(
                "run_loop_action=worktree issue={} workspace={} branch={} created={}",
                latest.identifier,
                worktree.workspace_path.display(),
                worktree.branch_name,
                worktree.created
            );
            Some(worktree)
        } else {
            None
        };

        let mut result = execute_issue_once_with_workspace_key(
            &workflow,
            &config,
            &latest,
            &handoff.workspace_key,
        )?;
        if result.success {
            if let Some(worktree) = live_worktree {
                let runner = ProcessHandoffCommandRunner;
                let verification = run_handoff_verification(&handoff.workspace_path, &config);
                println!(
                    "run_loop_action=verify issue={} success={} summary={}",
                    latest.identifier, verification.success, verification.summary
                );
                result.handoff_verification = Some(verification.summary.clone());
                if verification.success {
                    match publish_issue_pull_request(&handoff, &runner) {
                        Ok(publication) => {
                            println!(
                                "run_loop_action=pr issue={} url={} created={}",
                                latest.identifier, publication.pr_url, publication.pr_created
                            );
                            result.live_handoff = Some(RunLoopLiveHandoff {
                                worktree,
                                publication,
                                verification: verification.summary,
                            });
                        }
                        Err(error) => {
                            result.success = false;
                            result.message = format!("handoff publication failed: {error}");
                        }
                    }
                } else {
                    result.success = false;
                    result.message =
                        format!("handoff verification failed: {}", verification.summary);
                }
            }
            if result.success {
                let linked =
                    apply_live_handoff_pr_link(adapter.as_ref(), &latest.identifier, &mut result);
                if linked {
                    println!(
                        "run_loop_action=link_pr issue={} evidence=live_handoff",
                        latest.identifier
                    );
                }
            }
        }
        runtime_state = run_loop_runtime_state_with_result(runtime_state, &result);
        mark_runtime_state_updated(&mut runtime_state, current_time_ms());
        save_runtime_state(&config, &runtime_state)?;
        println!(
            "run_loop_runtime_state action=updated issue={} event={}",
            latest.identifier,
            runtime_state.last_event.as_deref().unwrap_or("unknown")
        );

        let workpad = run_loop_handoff_workpad(&latest, &result, &handoff);
        adapter.upsert_workpad(&latest.identifier, &workpad)?;

        if result.success {
            if !transition_allowed_for_main_agent("agent_review") {
                return Err("main implementation agent cannot set requested review state".into());
            }
            let evidence = run_loop_agent_review_handoff_evidence(&latest, &result, &handoff);
            let handoff_report = evaluate_agent_review_handoff(&evidence);
            let handoff_workpad =
                render_agent_review_handoff_workpad(&latest, &evidence, &handoff_report);
            adapter.upsert_workpad(&latest.identifier, &handoff_workpad)?;
            if !handoff_report.is_ready() {
                runtime_state = run_loop_runtime_state_with_transition(
                    runtime_state,
                    Some(latest.state.clone()),
                    "need_human_input",
                    "agent review handoff invariant failed",
                );
                save_runtime_state(&config, &runtime_state)?;
                adapter.set_state(&latest.identifier, "need_human_input")?;
                clear_runtime_state(&config)?;
                println!(
                    "run_loop_action=blocked issue={} target_state=need_human_input reason=handoff_invariant_failed",
                    latest.identifier
                );
                continue;
            }
            runtime_state = run_loop_runtime_state_with_transition(
                runtime_state,
                Some(latest.state.clone()),
                "agent_review",
                "main agent completed",
            );
            mark_runtime_state_updated(&mut runtime_state, current_time_ms());
            save_runtime_state(&config, &runtime_state)?;
            adapter.set_state(&latest.identifier, "agent_review")?;
            clear_runtime_state(&config)?;
            println!(
                "run_loop_action=handoff issue={} target_state=agent_review",
                latest.identifier
            );
        } else {
            let retry_delay_ms = Orchestrator::new(config.clone())
                .retry_delay_ms(runtime_state.attempt_count, false);
            if let Some(pause) = &result.usage_limit_pause {
                record_runtime_retry(
                    &mut runtime_state,
                    current_time_ms(),
                    retry_delay_ms,
                    format!("usage-limit pause: {}", pause.evidence),
                );
                save_runtime_state(&config, &runtime_state)?;
                let pause_workpad =
                    run_loop_usage_limit_pause_workpad(&latest, &result, pause, retry_delay_ms);
                adapter.upsert_workpad(&latest.identifier, &pause_workpad)?;
                append_runtime_supervision_event(
                    &config,
                    Some(&runtime_state),
                    "UsageLimitPaused",
                    &format!(
                        "issue={} classifier={} due_in_ms={} evidence={}",
                        latest.identifier, pause.classifier, retry_delay_ms, pause.evidence
                    ),
                )?;
                println!(
                    "run_loop_action=usage_limit_paused issue={} classifier={} due_in_ms={}",
                    latest.identifier, pause.classifier, retry_delay_ms
                );
                break;
            }
            if runtime_state.attempt_count < config.agent.max_turns {
                record_runtime_retry(
                    &mut runtime_state,
                    current_time_ms(),
                    retry_delay_ms,
                    result.message.clone(),
                );
                save_runtime_state(&config, &runtime_state)?;
                append_runtime_supervision_event(
                    &config,
                    Some(&runtime_state),
                    "RetryScheduled",
                    &format!(
                        "issue={} attempt={} due_in_ms={} error={}",
                        latest.identifier,
                        runtime_state.attempt_count,
                        retry_delay_ms,
                        result.message
                    ),
                )?;
                println!(
                    "run_loop_action=retry_scheduled issue={} attempt={} due_in_ms={}",
                    latest.identifier, runtime_state.attempt_count, retry_delay_ms
                );
                break;
            } else {
                runtime_state = run_loop_runtime_state_with_transition(
                    runtime_state,
                    Some(latest.state.clone()),
                    "need_human_input",
                    "backend run failed after retry limit",
                );
                mark_runtime_state_updated(&mut runtime_state, current_time_ms());
                save_runtime_state(&config, &runtime_state)?;
                adapter.set_state(&latest.identifier, "need_human_input")?;
                clear_runtime_state(&config)?;
                println!(
                    "run_loop_action=blocked issue={} target_state=need_human_input",
                    latest.identifier
                );
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoDispatchAction {
    Stop { reason: &'static str },
    SleepAndContinue { delay_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunLoopClaimAction {
    Claim,
    Resume,
    StopAndReplan { current_state: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResumePreflightAction {
    Continue,
    ClearCompleted {
        issue_identifier: String,
    },
    RetryLater {
        issue_identifier: String,
        retry: RuntimeRetryState,
        due_in_ms: u64,
    },
    Stalled {
        issue_identifier: String,
        stall: RuntimeStallState,
    },
    Block {
        reason: String,
    },
}

fn run_loop_claim_action(issue: &TrackerIssue, config: &RuntimeConfig) -> RunLoopClaimAction {
    match claim_decision(issue, config) {
        ClaimDecision::Claimable => RunLoopClaimAction::Claim,
        ClaimDecision::AlreadyInProgress => RunLoopClaimAction::Resume,
        ClaimDecision::StopAndReplan { current_state } => {
            RunLoopClaimAction::StopAndReplan { current_state }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssigneeOwnershipDecision {
    Allowed,
    Block { reason: String },
}

fn run_loop_assignee_ownership_decision(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    active_login: Option<&str>,
    profile_login: Option<&str>,
) -> AssigneeOwnershipDecision {
    if !live_github_tracker(config) {
        return AssigneeOwnershipDecision::Allowed;
    }

    if issue.assignees.is_empty() {
        return if config.tracker.assignee_filter.allow_unassigned {
            AssigneeOwnershipDecision::Allowed
        } else {
            AssigneeOwnershipDecision::Block {
                reason: "live GitHub issue has no assignee".into(),
            }
        };
    }

    let identities = [profile_login, active_login]
        .into_iter()
        .flatten()
        .map(normalized_login)
        .filter(|login| !login.is_empty())
        .collect::<Vec<_>>();

    if identities.is_empty() {
        return AssigneeOwnershipDecision::Block {
            reason: "active GitHub identity unavailable for assignee ownership check".into(),
        };
    }

    let assigned = issue
        .assignees
        .iter()
        .map(|assignee| normalized_login(assignee))
        .collect::<Vec<_>>();

    if assigned
        .iter()
        .any(|assignee| identities.iter().any(|identity| identity == assignee))
    {
        AssigneeOwnershipDecision::Allowed
    } else {
        AssigneeOwnershipDecision::Block {
            reason: format!(
                "active identity {:?} does not match issue assignees {:?}",
                identities, issue.assignees
            ),
        }
    }
}

fn live_github_tracker(config: &RuntimeConfig) -> bool {
    config.tracker.kind == "github_project_v2" && config.tracker.fixture_path.is_none()
}

fn normalized_login(value: &str) -> String {
    value.trim().trim_start_matches('@').to_ascii_lowercase()
}

fn selected_profile_github_login(
    config: &RuntimeConfig,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    Ok(
        selected_execution_profile(&config.profiles)?.and_then(|profile| {
            profile
                .env
                .get("GITHUB_LOGIN")
                .cloned()
                .or_else(|| profile.env.get("GH_LOGIN").cloned())
                .or_else(|| profile.env.get("JADE_GITHUB_LOGIN").cloned())
        }),
    )
}

fn current_gh_login() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let output = ProcessCommand::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    if !output.status.success() {
        return Ok(None);
    }

    let login = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!login.is_empty()).then_some(login))
}

fn run_loop_resume_preflight(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    config: &RuntimeConfig,
    state: Option<&RuntimeState>,
    now_ms: u64,
) -> Result<ResumePreflightAction, Box<dyn std::error::Error>> {
    let Some(state) = state else {
        return Ok(ResumePreflightAction::Continue);
    };
    let Some(active_issue) = state.active_issue.as_ref() else {
        return Ok(ResumePreflightAction::Continue);
    };

    let Some(issue) = adapter.get_issue(&active_issue.identifier)? else {
        return Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references missing issue {}",
                active_issue.identifier
            ),
        });
    };
    let normalized_state = normalize_state(&issue.state);

    if config
        .terminal_state_set()
        .iter()
        .any(|state| state == &normalized_state)
        || matches!(normalized_state.as_str(), "agent review" | "human review")
    {
        return Ok(ResumePreflightAction::ClearCompleted {
            issue_identifier: active_issue.identifier.clone(),
        });
    }

    if normalized_state != "in progress" {
        return Ok(ResumePreflightAction::Block {
            reason: format!(
                "runtime state references {} but tracker state is {}",
                active_issue.identifier, issue.state
            ),
        });
    }

    if let Some(retry) = state.retry.clone() {
        let due_in_ms = retry.due_in_ms(now_ms);
        if due_in_ms > 0 {
            return Ok(ResumePreflightAction::RetryLater {
                issue_identifier: active_issue.identifier.clone(),
                retry,
                due_in_ms,
            });
        }
    }

    if let Some(stall) = detect_runtime_stall(state, now_ms, config.codex.stall_timeout_ms) {
        return Ok(ResumePreflightAction::Stalled {
            issue_identifier: active_issue.identifier.clone(),
            stall,
        });
    }

    Ok(ResumePreflightAction::Continue)
}

fn no_dispatch_action(
    options: &RunLoopOptions,
    limit: Option<usize>,
    poll_interval_ms: u64,
) -> NoDispatchAction {
    if !options.write || limit.is_some() {
        return NoDispatchAction::Stop {
            reason: "no_dispatchable_issue",
        };
    }

    NoDispatchAction::SleepAndContinue {
        delay_ms: poll_interval_ms,
    }
}

fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn append_runtime_supervision_event(
    config: &RuntimeConfig,
    state: Option<&RuntimeState>,
    event: &str,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    let active_issue = state.and_then(|state| state.active_issue.as_ref());
    log.append(&EventRecord {
        event: event.into(),
        issue_id: active_issue.map(|issue| issue.id.clone()),
        issue_identifier: active_issue.map(|issue| issue.identifier.clone()),
        session_id: state.and_then(|state| state.backend_session_id.clone()),
        profile_id: state.and_then(|state| state.profile_id.clone()),
        instance_name: state.and_then(|state| state.instance_name.clone()),
        actor_role: Some(config.identity.actor_role.clone()),
        actor_label: Some(config.identity.actor_label.clone()),
        git_author: config.identity.git.author(),
        message: message.into(),
    })?;
    Ok(())
}

fn run_loop_runtime_state_for_issue(
    existing: Option<&RuntimeState>,
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    event: &str,
) -> RuntimeState {
    let profile = selected_execution_profile(&config.profiles).ok().flatten();
    let mut state = RuntimeState::active(
        RuntimeIssueState {
            id: issue.id.clone(),
            identifier: issue.identifier.clone(),
        },
        &config.backend.kind,
    );
    state.attempt_count = next_runtime_attempt_count(existing, &issue.identifier);
    state.branch_name = issue.branch_name.clone();
    state.profile_id = profile.as_ref().map(|profile| profile.profile_id.clone());
    state.instance_name = profile
        .as_ref()
        .map(|profile| profile.instance_name.clone());
    state.actor_role = Some(config.identity.actor_role.clone());
    state.actor_label = Some(config.identity.actor_label.clone());
    state.git_author = config.identity.git.author();
    state.last_event = Some(event.into());
    state
}

fn next_runtime_attempt_count(existing: Option<&RuntimeState>, issue_identifier: &str) -> u32 {
    existing
        .and_then(|state| {
            state
                .active_issue
                .as_ref()
                .filter(|issue| issue.identifier == issue_identifier)
                .map(|_| state.attempt_count.saturating_add(1))
        })
        .unwrap_or(1)
}

fn run_loop_runtime_state_with_result(
    mut state: RuntimeState,
    result: &IssueExecutionResult,
) -> RuntimeState {
    state.workspace_path = Some(result.workspace_path.clone());
    state.backend = result.backend.clone();
    state.backend_session_id = result.session_id.clone();
    state.profile_id = result.profile_id.clone();
    state.instance_name = result.instance_name.clone();
    state.actor_role = Some(result.actor_role.clone());
    state.actor_label = Some(result.actor_label.clone());
    state.git_author = result.git_author.clone();
    state.last_event = Some(if result.success {
        "Completed".into()
    } else {
        "Failed".into()
    });
    state
}

fn run_loop_runtime_state_with_transition(
    mut state: RuntimeState,
    from: Option<String>,
    to: &str,
    reason: &str,
) -> RuntimeState {
    state.last_transition = Some(RuntimeTransition {
        from,
        to: to.into(),
        reason: reason.into(),
    });
    state
}

fn run_loop_handoff_plan(
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueHandoffPlan, HandoffError> {
    let profile = selected_execution_profile(&config.profiles)
        .ok()
        .flatten()
        .map(|profile| profile.workspace_namespace);
    plan_issue_handoff_for_profile(
        &config.workspace.root,
        issue,
        DEFAULT_RUN_LOOP_BASE_BRANCH,
        profile.as_deref(),
    )
}

fn run_loop_runtime_ownership(
    issue: &TrackerIssue,
    config: &RuntimeConfig,
    handoff: &IssueHandoffPlan,
) -> Result<RuntimeOwnershipMarker, Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    Ok(RuntimeOwnershipMarker {
        issue_ref: issue.identifier.clone(),
        actor_role: config.identity.actor_role.clone(),
        actor_label: config.identity.actor_label.clone(),
        profile_id: profile.as_ref().map(|profile| profile.profile_id.clone()),
        instance_name: profile
            .as_ref()
            .map(|profile| profile.instance_name.clone()),
        workspace_key: handoff.workspace_key.clone(),
        branch_name: handoff.branch_name.clone(),
    })
}

fn run_loop_ownership_workpad(
    issue: &TrackerIssue,
    ownership: &RuntimeOwnershipMarker,
    event: &str,
) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Runtime Ownership".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Event: `{event}`"),
        "- This marker is advisory tracker-visible ownership for active `In Progress` work.".into(),
        "- Another run-loop profile should not resume this issue when the marker differs.".into(),
        String::new(),
        render_runtime_ownership_marker(ownership),
    ]
    .join("\n")
}

fn run_loop_live_handoff_enabled(config: &RuntimeConfig) -> bool {
    config.tracker.kind == "github_project_v2" && config.tracker.fixture_path.is_none()
}

fn run_handoff_verification(workspace_path: &Path, config: &RuntimeConfig) -> HandoffVerification {
    if config.verification.commands.is_empty() {
        return HandoffVerification {
            success: true,
            summary: "skipped:not_configured".into(),
        };
    }

    for (index, command) in config.verification.commands.iter().enumerate() {
        let label = format!("verification:{}", index + 1);
        if let Err(error) = run_workspace_command(
            &label,
            command,
            workspace_path,
            config.verification.timeout_ms,
        ) {
            return HandoffVerification {
                success: false,
                summary: format!(
                    "failed command={} index={} error={}",
                    shell_summary(command),
                    index + 1,
                    compact_evidence(&error.to_string())
                ),
            };
        }
    }

    HandoffVerification {
        success: true,
        summary: format!("passed:{} command(s)", config.verification.commands.len()),
    }
}

fn shell_summary(command: &str) -> String {
    let compact = compact_evidence(command);
    format!("`{compact}`")
}

fn compact_evidence(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 240;
    let truncated = compact.chars().take(LIMIT).collect::<String>();
    if truncated.len() < compact.len() {
        format!("{truncated}...")
    } else {
        compact
    }
}

fn handle_run_loop_gate_failure(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    issue: &TrackerIssue,
    decision: &GateDecision,
    options: &RunLoopOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "run_loop_gate=failed issue={} decision={:?}",
        issue.identifier, decision.kind
    );
    if options.write {
        adapter.upsert_workpad(&issue.identifier, &gate_workpad(issue, decision))?;
        adapter.set_state(&issue.identifier, gate_target_state(decision))?;
    } else {
        println!(
            "run_loop_dry_run action=workpad issue={} reason=quality_gate_failed",
            issue.identifier
        );
        println!(
            "run_loop_dry_run action=set_state issue={} target_state={}",
            issue.identifier,
            gate_target_state(decision)
        );
    }
    Ok(())
}

fn handle_run_loop_handoff_failure(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    issue: &TrackerIssue,
    error: &HandoffError,
    options: &RunLoopOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "run_loop_handoff=failed issue={} error={}",
        issue.identifier, error
    );
    let workpad = run_loop_handoff_failure_workpad(issue, error);
    if options.write {
        adapter.upsert_workpad(&issue.identifier, &workpad)?;
        adapter.set_state(&issue.identifier, "need_human_input")?;
    } else {
        println!(
            "run_loop_dry_run action=workpad issue={} reason=handoff_plan_failed",
            issue.identifier
        );
        println!(
            "run_loop_dry_run action=set_state issue={} target_state=need_human_input",
            issue.identifier
        );
    }
    Ok(())
}

fn print_run_loop_dry_run_actions(
    issue: &TrackerIssue,
    handoff: &IssueHandoffPlan,
    config: &RuntimeConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let profile = selected_execution_profile(&config.profiles)?;
    if normalize_state(&issue.state) != "in progress" {
        println!(
            "run_loop_dry_run action=claim issue={} target_state=in_progress",
            issue.identifier
        );
    } else {
        println!("run_loop_dry_run action=resume issue={}", issue.identifier);
    }
    println!(
        "run_loop_dry_run action=handoff_plan issue={} workspace_key={} workspace_path={} branch={} pr_title={:?}",
        issue.identifier,
        handoff.workspace_key,
        handoff.workspace_path.display(),
        handoff.branch_name,
        handoff.pull_request.title
    );
    println!(
        "run_loop_dry_run action=identity issue={} actor_role={} actor_label={:?} git_author={:?}",
        issue.identifier,
        config.identity.actor_role,
        config.identity.actor_label,
        config.identity.git.author()
    );
    println!(
        "run_loop_dry_run action=run issue={} backend=configured",
        issue.identifier
    );
    if let Some(profile) = profile {
        println!(
            "run_loop_dry_run profile_id={} instance_name={}",
            profile.profile_id, profile.instance_name
        );
    }
    println!(
        "run_loop_dry_run action=worktree issue={} workspace={} branch={}",
        issue.identifier,
        handoff.workspace_path.display(),
        handoff.branch_name
    );
    let verification_summary = if config.verification.commands.is_empty() {
        "skipped:not_configured".to_string()
    } else {
        format!(
            "configured:{} command(s)",
            config.verification.commands.len()
        )
    };
    println!(
        "run_loop_dry_run action=verify issue={} summary={}",
        issue.identifier, verification_summary
    );
    println!(
        "run_loop_dry_run action=pr issue={} head={} base={}",
        issue.identifier, handoff.branch_name, handoff.pull_request.base_branch
    );
    println!(
        "run_loop_dry_run action=workpad issue={} evidence=run_summary",
        issue.identifier
    );
    println!(
        "run_loop_dry_run action=handoff issue={} target_state=agent_review",
        issue.identifier
    );
    Ok(())
}

fn run_loop_handoff_workpad(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    handoff: &IssueHandoffPlan,
) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `jade-symphony run-loop`".to_string(),
        String::new(),
        "### Run Evidence".to_string(),
        format!("- Workspace: `{}`", result.workspace_path.display()),
        format!("- Backend: `{}`", result.backend),
        format!(
            "- Profile: `{}`",
            result.profile_id.as_deref().unwrap_or("n/a")
        ),
        format!(
            "- Instance: `{}`",
            result.instance_name.as_deref().unwrap_or("n/a")
        ),
        format!("- Actor role: `{}`", result.actor_role),
        format!("- Actor label: `{}`", result.actor_label),
        format!(
            "- Git author: `{}`",
            result.git_author.as_deref().unwrap_or("n/a")
        ),
        format!("- Git identity: `{}`", result.git_identity.summary()),
        format!("- Success: `{}`", result.success),
        format!(
            "- Session: `{}`",
            result.session_id.as_deref().unwrap_or("n/a")
        ),
        format!("- Message: {}", result.message),
        String::new(),
        "### Planned Handoff".to_string(),
        format!("- Workspace key: `{}`", handoff.workspace_key),
        format!("- Workspace path: `{}`", handoff.workspace_path.display()),
        format!("- Branch: `{}`", handoff.branch_name),
        format!("- PR title: `{}`", handoff.pull_request.title),
        format!("- PR base branch: `{}`", handoff.pull_request.base_branch),
        rework_continuation_workpad_line(handoff),
        handoff_verification_workpad_line(result),
        live_handoff_workpad_line(result),
        String::new(),
        "### Main-Agent Boundary".to_string(),
        "- Locally complete main-agent work stops at `Agent Review`.".to_string(),
        "- `Human Review` is reserved for independent Review Agent pass evidence.".to_string(),
    ]
    .join("\n")
}

fn rework_continuation_workpad_line(handoff: &IssueHandoffPlan) -> String {
    match &handoff.continuation {
        Some(continuation) => format!(
            "- Rework continuation: `{}` from `{}` ({})",
            continuation.pull_request_url, continuation.source, continuation.pull_request_state
        ),
        None => "- Rework continuation: `not-used`".to_string(),
    }
}

fn handoff_verification_workpad_line(result: &IssueExecutionResult) -> String {
    format!(
        "- Handoff verification: `{}`",
        result
            .handoff_verification
            .as_deref()
            .unwrap_or("skipped:not_run")
    )
}

fn live_handoff_workpad_line(result: &IssueExecutionResult) -> String {
    match &result.live_handoff {
        Some(handoff) => format!(
            "- Live PR: `{}` (created: `{}`, branch pushed: `{}`, verification: `{}`)",
            handoff.publication.pr_url,
            handoff.publication.pr_created,
            handoff.publication.branch_pushed,
            handoff.verification
        ),
        None => "- Live PR: `not-created`".to_string(),
    }
}

fn record_live_handoff_pr_link(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    result: &IssueExecutionResult,
) -> Result<(), String> {
    let Some(handoff) = &result.live_handoff else {
        return Ok(());
    };

    adapter
        .link_pull_request(issue_ref, &handoff.publication.pr_url)
        .map_err(|error| format!("handoff PR link failed: {error}"))
}

fn apply_live_handoff_pr_link(
    adapter: &dyn TrackerAdapter,
    issue_ref: &str,
    result: &mut IssueExecutionResult,
) -> bool {
    if result.live_handoff.is_none() {
        return false;
    }

    match record_live_handoff_pr_link(adapter, issue_ref, result) {
        Ok(()) => true,
        Err(error) => {
            result.success = false;
            result.message = error;
            false
        }
    }
}

fn run_loop_agent_review_handoff_evidence(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    handoff: &IssueHandoffPlan,
) -> AgentReviewHandoffEvidence {
    let mut evidence = AgentReviewHandoffEvidence::from_plan(
        handoff,
        format!(
            "backend={} success={} session={} message={}",
            result.backend,
            result.success,
            result.session_id.as_deref().unwrap_or("n/a"),
            result.message
        ),
        "main agent completed local run",
    );
    evidence.pull_request_url = result
        .live_handoff
        .as_ref()
        .map(|handoff| handoff.publication.pr_url.clone())
        .or_else(|| {
            issue
                .linked_pull_requests
                .iter()
                .find_map(|pr| pr.url.clone())
        });
    if evidence.pull_request_url.is_none() {
        evidence.no_pr_blocker = Some(
            "No pull request URL was present in tracker data at handoff time; keeping issue out of Agent Review until PR evidence is durable.".into(),
        );
    }
    evidence
}

fn run_loop_handoff_failure_workpad(issue: &TrackerIssue, error: &HandoffError) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `jade-symphony run-loop`".to_string(),
        String::new(),
        "### Handoff Planning Blocker".to_string(),
        format!("- Error: `{}`", error),
        "- Backend execution was skipped before claim/run to avoid mixing issue scope.".to_string(),
        String::new(),
        "### Required Human Decision".to_string(),
        "- Confirm the correct branch/workspace ownership before retrying.".to_string(),
    ]
    .join("\n")
}

fn run_loop_assignee_ownership_workpad(issue: &TrackerIssue, reason: &str) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Assignee Ownership Blocker".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Reason: {reason}"),
        format!("- Issue assignees: `{}`", issue.assignees.join(", ")),
        String::new(),
        "### Boundary".to_string(),
        "- Jade Symphony did not claim this issue or move it to `In Progress`.".to_string(),
        "- Assign the issue to the active GitHub identity or selected execution profile before retrying.".to_string(),
    ]
    .join("\n")
}

fn run_loop_usage_limit_pause_workpad(
    issue: &TrackerIssue,
    result: &IssueExecutionResult,
    pause: &UsageLimitPause,
    retry_delay_ms: u64,
) -> String {
    [
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Usage-Limit Pause".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        "- Source: `jade-symphony run-loop`".to_string(),
        format!("- Backend: `{}`", result.backend),
        format!("- Classifier: `{}`", pause.classifier),
        format!("- Evidence: {}", pause.evidence),
        format!("- Retry backoff: `{retry_delay_ms}ms`"),
        String::new(),
        "### State Safety".to_string(),
        "- Tracker state was not advanced to `Agent Review`.".to_string(),
        "- Runtime state keeps the active issue and next retry time.".to_string(),
        "- The run-loop will skip this issue until retry backoff expires or an operator intervenes."
            .to_string(),
    ]
    .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Plan {
        workflow_path: PathBuf,
        json: bool,
    },
    StatusApi {
        workflow_path: PathBuf,
        bind: SocketAddr,
        once: bool,
    },
    Validate {
        workflow_path: PathBuf,
    },
    Inspect {
        workflow_path: PathBuf,
    },
    Doctor {
        options: DoctorOptions,
    },
    DoctorRepairHumanReview {
        workflow_path: PathBuf,
        write: bool,
    },
    Profiles {
        workflow_path: PathBuf,
    },
    DogfoodSmoke {
        workflow_path: PathBuf,
        write: bool,
    },
    RunOnce {
        workflow_path: PathBuf,
    },
    RunLoop {
        options: RunLoopOptions,
    },
    CleanupWorkspaces {
        workflow_path: PathBuf,
        write: bool,
    },
    MergeOnce {
        workflow_path: PathBuf,
        write: bool,
    },
    SetState {
        workflow_path: PathBuf,
        issue_ref: String,
        state: String,
        write: bool,
    },
    Workpad {
        workflow_path: PathBuf,
        issue_ref: String,
        markdown_path: PathBuf,
        write: bool,
    },
    CreateFollowUp {
        workflow_path: PathBuf,
        title: String,
        body_path: PathBuf,
        write: bool,
    },
    AddToProject {
        workflow_path: PathBuf,
        issue_id: String,
        write: bool,
    },
    ReviewFake {
        workflow_path: PathBuf,
        issue_ref: String,
        outcome: FakeReviewOutcome,
        write: bool,
    },
    ReviewOnce {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    ReviewFreshness {
        input: ReviewFreshnessInput,
    },
    ReviewLoop {
        options: ReviewLoopOptions,
    },
    MergeLoop {
        options: MergeLoopOptions,
    },
    Gate {
        workflow_path: PathBuf,
        issue_ref: String,
        apply: bool,
        write: bool,
    },
    ForgeDraft {
        title: String,
        goal: String,
    },
    ForgeDiscover {
        source: String,
    },
    ForgeDiscuss {
        title: String,
        markdown: String,
    },
    ForgeValidate {
        title: String,
        markdown: String,
    },
    ForgeRepair {
        title: String,
        markdown: String,
    },
    ForgeCreate {
        workflow_path: PathBuf,
        title: String,
        markdown: String,
        add_to_project: bool,
        project_fields: Vec<ProjectFieldAssignment>,
        write: bool,
    },
    ForgeInteractive {
        options: ForgeInteractiveOptions,
    },
    ForgeReflect {
        context: String,
        skill: Option<String>,
        limit: usize,
    },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunLoopOptions {
    workflow_path: PathBuf,
    max_iterations: Option<usize>,
    once: bool,
    write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReviewLoopOptions {
    workflow_path: PathBuf,
    max_iterations: Option<usize>,
    once: bool,
    write: bool,
    fake_outcome: Option<FakeReviewOutcome>,
    max_concurrent: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorOptions {
    workflow_path: PathBuf,
    json: bool,
    strict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MergeLoopOptions {
    workflow_path: PathBuf,
    max_iterations: Option<usize>,
    once: bool,
    write: bool,
}

impl ReviewLoopOptions {
    fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }

    fn worker_limit(&self, config: &RuntimeConfig) -> usize {
        self.max_concurrent
            .unwrap_or(config.review.max_concurrent_workers)
            .max(1)
    }
}

impl MergeLoopOptions {
    fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }
}

impl RunLoopOptions {
    fn iteration_limit(&self) -> Option<usize> {
        if self.once {
            Some(1)
        } else {
            self.max_iterations
        }
    }
}

impl Command {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        if matches!(args.first().map(String::as_str), Some("help")) {
            return Ok(Self::Help);
        }

        let argv = std::iter::once("jade-symphony".to_string())
            .chain(args)
            .collect::<Vec<_>>();
        match Cli::try_parse_from(argv) {
            Ok(cli) => Command::try_from(cli),
            Err(error) if error.kind() == ErrorKind::DisplayHelp => Ok(Self::Help),
            Err(error) => Err(error.to_string()),
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "jade-symphony",
    about = "OpenAI Symphony-style orchestration harness with Jade extensions",
    disable_help_subcommand = true,
    arg_required_else_help = false
)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    #[command(alias = "plan-dispatch", alias = "dry-run", alias = "status")]
    Plan(WorkflowPathArgs),
    #[command(name = "status-api")]
    StatusApi(StatusApiArgs),
    #[command(alias = "validate-workflow")]
    Validate(WorkflowPathArgs),
    Inspect(WorkflowPathArgs),
    #[command(alias = "audit-project")]
    Doctor(DoctorArgs),
    #[command(name = "doctor-repair-human-review")]
    DoctorRepairHumanReview(DoctorRepairArgs),
    Profiles(WorkflowPathArgs),
    #[command(name = "dogfood-smoke")]
    DogfoodSmoke(DogfoodSmokeArgs),
    #[command(name = "run-once")]
    RunOnce(WorkflowPathArgs),
    #[command(name = "run-loop")]
    RunLoop(RunLoopArgs),
    #[command(name = "cleanup-workspaces", alias = "workspace-cleanup")]
    CleanupWorkspaces(CleanupWorkspacesArgs),
    #[command(name = "merge-once", alias = "land")]
    MergeOnce(MergeOnceArgs),
    #[command(name = "merge-loop")]
    MergeLoop(MergeLoopArgs),
    #[command(name = "set-state")]
    SetState(SetStateArgs),
    Workpad(WorkpadArgs),
    #[command(name = "create-follow-up")]
    CreateFollowUp(CreateFollowUpArgs),
    #[command(name = "add-to-project")]
    AddToProject(AddToProjectArgs),
    #[command(name = "review-fake")]
    ReviewFake(ReviewFakeArgs),
    #[command(name = "review-once")]
    ReviewOnce(ReviewOnceArgs),
    #[command(name = "review-freshness")]
    ReviewFreshness(ReviewFreshnessArgs),
    #[command(name = "review-loop")]
    ReviewLoop(ReviewLoopArgs),
    Gate(GateArgs),
    #[command(name = "gate-apply")]
    GateApply(GateArgs),
    #[command(name = "forge-discover")]
    ForgeDiscover(ForgeDiscoverArgs),
    #[command(name = "forge-discuss")]
    ForgeDiscuss(ForgeMarkdownArgs),
    #[command(name = "forge-draft")]
    ForgeDraft(ForgeDraftArgs),
    #[command(name = "forge-validate")]
    ForgeValidate(ForgeMarkdownArgs),
    #[command(name = "forge-repair")]
    ForgeRepair(ForgeMarkdownArgs),
    #[command(name = "forge-create")]
    ForgeCreate(ForgeCreateArgs),
    #[command(name = "forge-interactive")]
    ForgeInteractive(ForgeInteractiveArgs),
    #[command(name = "forge-reflect")]
    ForgeReflect(ForgeReflectArgs),
}

#[derive(Debug, Args)]
struct WorkflowPathArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct DoctorRepairArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    strict: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct StatusApiArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: SocketAddr,
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Args)]
struct RunLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    max_iterations: Option<usize>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct DogfoodSmokeArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct CleanupWorkspacesArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct MergeOnceArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct MergeLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    max_iterations: Option<usize>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct GateArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct SetStateArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    state: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct WorkpadArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    markdown_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct CreateFollowUpArgs {
    #[arg(long)]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[arg(long = "body-file")]
    body_file: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct AddToProjectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_id: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewFakeArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long, value_enum, default_value = "pass")]
    outcome: CliFakeReviewOutcome,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewOnceArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewFreshnessArgs {
    #[arg(long = "issue")]
    issue_ref: String,
    #[arg(long = "prior-head")]
    prior_head_sha: String,
    #[arg(long = "current-head")]
    current_head_sha: String,
    #[arg(long = "prior-base")]
    prior_base_sha: String,
    #[arg(long = "current-base")]
    current_base_sha: String,
    #[arg(long = "changed-file")]
    changed_files: Vec<String>,
    #[arg(long = "stale-reason", value_enum)]
    stale_reason: CliReviewStaleReason,
    #[arg(long = "rework-class", value_enum)]
    rework_class: CliReviewReworkClass,
    #[arg(long = "patch-summary")]
    patch_summary: Option<String>,
}

#[derive(Debug, Args)]
struct ReviewLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    max_iterations: Option<usize>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    write: bool,
    #[arg(long = "max-concurrent")]
    max_concurrent: Option<usize>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "fake-outcome", value_enum)]
    fake_outcome: Option<CliFakeReviewOutcome>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliReviewStaleReason {
    MergeConflict,
    BaseBranchUpdated,
    ReviewOutdated,
    Unknown,
}

impl From<CliReviewStaleReason> for ReviewStaleReason {
    fn from(value: CliReviewStaleReason) -> Self {
        match value {
            CliReviewStaleReason::MergeConflict => ReviewStaleReason::MergeConflict,
            CliReviewStaleReason::BaseBranchUpdated => ReviewStaleReason::BaseBranchUpdated,
            CliReviewStaleReason::ReviewOutdated => ReviewStaleReason::ReviewOutdated,
            CliReviewStaleReason::Unknown => ReviewStaleReason::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliReviewReworkClass {
    MechanicalConflictResolution,
    BaseRefresh,
    SemanticChange,
    Unknown,
}

impl From<CliReviewReworkClass> for ReviewReworkClass {
    fn from(value: CliReviewReworkClass) -> Self {
        match value {
            CliReviewReworkClass::MechanicalConflictResolution => {
                ReviewReworkClass::MechanicalConflictResolution
            }
            CliReviewReworkClass::BaseRefresh => ReviewReworkClass::BaseRefresh,
            CliReviewReworkClass::SemanticChange => ReviewReworkClass::SemanticChange,
            CliReviewReworkClass::Unknown => ReviewReworkClass::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliFakeReviewOutcome {
    Pass,
    Confirmed,
    Failed,
}

impl From<CliFakeReviewOutcome> for FakeReviewOutcome {
    fn from(value: CliFakeReviewOutcome) -> Self {
        match value {
            CliFakeReviewOutcome::Pass => FakeReviewOutcome::Pass,
            CliFakeReviewOutcome::Confirmed => FakeReviewOutcome::ConfirmedFinding,
            CliFakeReviewOutcome::Failed => FakeReviewOutcome::Failed,
        }
    }
}

#[derive(Debug, Args)]
struct ForgeDraftArgs {
    #[arg(long)]
    title: String,
    #[arg(long)]
    goal: String,
}

#[derive(Debug, Args)]
struct ForgeDiscoverArgs {
    #[arg(long)]
    intent: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ForgeMarkdownArgs {
    #[arg(long)]
    title: String,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    body: Option<String>,
}

#[derive(Debug, Args)]
struct ForgeCreateArgs {
    #[arg(long)]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    body: Option<String>,
    #[arg(long = "add-to-project")]
    add_to_project: bool,
    #[arg(long = "project-field")]
    project_fields: Vec<String>,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ForgeInteractiveArgs {
    #[arg(long)]
    workflow: Option<PathBuf>,
    #[arg(long)]
    title: String,
    #[arg(long)]
    intent: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    #[arg(long)]
    skill: Option<String>,
    #[arg(long = "context-file")]
    context_file: Option<PathBuf>,
    #[arg(long = "add-to-project")]
    add_to_project: bool,
    #[arg(long)]
    write: bool,
    #[arg(long = "confirm-create")]
    confirm_create: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ForgeReflectArgs {
    #[arg(long = "context-file")]
    context_file: PathBuf,
    #[arg(long)]
    skill: Option<String>,
    #[arg(long, default_value_t = 3)]
    limit: usize,
}

impl TryFrom<Cli> for Command {
    type Error = String;

    fn try_from(cli: Cli) -> Result<Self, Self::Error> {
        let default_workflow = || PathBuf::from("WORKFLOW.md");
        match cli.command {
            None => Ok(Self::Plan {
                workflow_path: cli.workflow_path.unwrap_or_else(default_workflow),
                json: false,
            }),
            Some(command) => {
                if cli.workflow_path.is_some() {
                    return Err(usage());
                }

                match command {
                    CliCommand::Plan(args) => Ok(Self::Plan {
                        workflow_path: args.workflow_path,
                        json: args.json,
                    }),
                    CliCommand::StatusApi(args) => Ok(Self::StatusApi {
                        workflow_path: args.workflow_path,
                        bind: args.bind,
                        once: args.once,
                    }),
                    CliCommand::Validate(args) => Ok(Self::Validate {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Inspect(args) => Ok(Self::Inspect {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Doctor(args) => Ok(Self::Doctor {
                        options: DoctorOptions {
                            workflow_path: args.workflow_path,
                            json: args.json,
                            strict: args.strict,
                        },
                    }),
                    CliCommand::DoctorRepairHumanReview(args) => {
                        Ok(Self::DoctorRepairHumanReview {
                            workflow_path: args.workflow_path,
                            write: args.write,
                        })
                    }
                    CliCommand::Profiles(args) => Ok(Self::Profiles {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::DogfoodSmoke(args) => Ok(Self::DogfoodSmoke {
                        workflow_path: args.workflow_path,
                        write: args.write,
                    }),
                    CliCommand::RunOnce(args) => Ok(Self::RunOnce {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::RunLoop(args) => {
                        if args.max_iterations == Some(0) {
                            return Err(usage());
                        }
                        Ok(Self::RunLoop {
                            options: RunLoopOptions {
                                workflow_path: args.workflow_path,
                                max_iterations: args.max_iterations,
                                once: args.once,
                                write: args.write,
                            },
                        })
                    }
                    CliCommand::CleanupWorkspaces(args) => Ok(Self::CleanupWorkspaces {
                        workflow_path: args.workflow_path,
                        write: args.write,
                    }),
                    CliCommand::MergeOnce(args) => Ok(Self::MergeOnce {
                        workflow_path: args.workflow_path,
                        write: args.write,
                    }),
                    CliCommand::MergeLoop(args) => {
                        if args.max_iterations == Some(0)
                            || (!args.once && args.max_iterations.is_none())
                        {
                            return Err(usage());
                        }
                        Ok(Self::MergeLoop {
                            options: MergeLoopOptions {
                                workflow_path: args.workflow_path,
                                max_iterations: args.max_iterations,
                                once: args.once,
                                write: args.write,
                            },
                        })
                    }
                    CliCommand::SetState(args) => Ok(Self::SetState {
                        workflow_path: args.workflow_path,
                        issue_ref: args.issue_ref,
                        state: args.state,
                        write: args.write,
                    }),
                    CliCommand::Workpad(args) => Ok(Self::Workpad {
                        workflow_path: args.workflow_path,
                        issue_ref: args.issue_ref,
                        markdown_path: args.markdown_path,
                        write: args.write,
                    }),
                    CliCommand::CreateFollowUp(args) => Ok(Self::CreateFollowUp {
                        workflow_path: args.workflow,
                        title: args.title,
                        body_path: args.body_file,
                        write: args.write,
                    }),
                    CliCommand::AddToProject(args) => Ok(Self::AddToProject {
                        workflow_path: args.workflow_path,
                        issue_id: args.issue_id,
                        write: args.write,
                    }),
                    CliCommand::ReviewFake(args) => Ok(Self::ReviewFake {
                        workflow_path: args.workflow_path,
                        issue_ref: args.issue_ref,
                        outcome: args.outcome.into(),
                        write: args.write,
                    }),
                    CliCommand::ReviewOnce(args) => Ok(Self::ReviewOnce {
                        workflow_path: args.workflow_path,
                        issue_ref: args.issue_ref,
                        write: args.write,
                    }),
                    CliCommand::ReviewFreshness(args) => Ok(Self::ReviewFreshness {
                        input: ReviewFreshnessInput {
                            issue_ref: args.issue_ref,
                            prior_head_sha: args.prior_head_sha,
                            current_head_sha: args.current_head_sha,
                            prior_base_sha: args.prior_base_sha,
                            current_base_sha: args.current_base_sha,
                            changed_files: args.changed_files,
                            stale_reason: args.stale_reason.into(),
                            rework_class: args.rework_class.into(),
                            patch_summary: args.patch_summary,
                        },
                    }),
                    CliCommand::ReviewLoop(args) => {
                        if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
                            return Err(usage());
                        }
                        Ok(Self::ReviewLoop {
                            options: ReviewLoopOptions {
                                workflow_path: args.workflow_path,
                                max_iterations: args.max_iterations,
                                once: args.once,
                                write: args.write,
                                fake_outcome: args.fake_outcome.map(Into::into),
                                max_concurrent: args.max_concurrent,
                            },
                        })
                    }
                    CliCommand::Gate(args) => Ok(Self::Gate {
                        workflow_path: args.workflow_path,
                        issue_ref: args.issue_ref,
                        apply: false,
                        write: args.write,
                    }),
                    CliCommand::GateApply(args) => Ok(Self::Gate {
                        workflow_path: args.workflow_path,
                        issue_ref: args.issue_ref,
                        apply: true,
                        write: args.write,
                    }),
                    CliCommand::ForgeDiscover(args) => Ok(Self::ForgeDiscover {
                        source: read_source_arg(args.intent, args.file)?,
                    }),
                    CliCommand::ForgeDiscuss(args) => Ok(Self::ForgeDiscuss {
                        title: args.title,
                        markdown: read_source_arg(args.body, args.file)?,
                    }),
                    CliCommand::ForgeDraft(args) => Ok(Self::ForgeDraft {
                        title: args.title,
                        goal: args.goal,
                    }),
                    CliCommand::ForgeValidate(args) => Ok(Self::ForgeValidate {
                        title: args.title,
                        markdown: read_source_arg(args.body, args.file)?,
                    }),
                    CliCommand::ForgeRepair(args) => Ok(Self::ForgeRepair {
                        title: args.title,
                        markdown: read_source_arg(args.body, args.file)?,
                    }),
                    CliCommand::ForgeCreate(args) => Ok(Self::ForgeCreate {
                        workflow_path: args.workflow,
                        title: args.title,
                        markdown: read_source_arg(args.body, args.file)?,
                        add_to_project: args.add_to_project,
                        project_fields: parse_project_field_assignments(args.project_fields)?,
                        write: args.write,
                    }),
                    CliCommand::ForgeInteractive(args) => Ok(Self::ForgeInteractive {
                        options: ForgeInteractiveOptions {
                            workflow_path: args.workflow,
                            title: args.title,
                            intent: read_source_arg(args.intent, args.file)?,
                            skill: validate_optional_forge_skill(args.skill)?,
                            context: read_optional_file(args.context_file)?,
                            add_to_project: args.add_to_project,
                            write: args.write,
                            confirm_create: args.confirm_create,
                        },
                    }),
                    CliCommand::ForgeReflect(args) => Ok(Self::ForgeReflect {
                        context: read_required_file(args.context_file)?,
                        skill: validate_optional_forge_skill(args.skill)?,
                        limit: args.limit,
                    }),
                }
            }
        }
    }
}

fn gate_workpad(issue: &TrackerIssue, decision: &GateDecision) -> String {
    let mut lines = vec![
        "## Jade Symphony Workpad".to_string(),
        String::new(),
        "### Context".to_string(),
        format!("- Issue: {} {}", issue.identifier, issue.title),
        format!("- Current state: {}", issue.state),
        String::new(),
        "### Decisions / Assumptions".to_string(),
    ];

    if decision.assumptions.is_empty() {
        lines.push("- None recorded.".into());
    } else {
        lines.extend(decision.assumptions.iter().map(|item| format!("- {item}")));
    }

    lines.extend([
        String::new(),
        "### Quality Gate".to_string(),
        format!("- Decision: {:?}", decision.kind),
    ]);

    if !decision.missing.is_empty() {
        lines.push(format!("- Missing: {}", decision.missing.join(", ")));
    }
    if !decision.notes.is_empty() {
        lines.extend(decision.notes.iter().map(|item| format!("- Note: {item}")));
    }

    lines.extend([
        String::new(),
        "### Plan".to_string(),
        "- [ ] Resolve quality-gate findings before dispatch.".to_string(),
        String::new(),
        "### Validation".to_string(),
        "- [ ] Re-run `jade-symphony gate` after issue updates.".to_string(),
    ]);

    lines.join("\n")
}

fn gate_target_state(decision: &GateDecision) -> &'static str {
    match decision.kind {
        GateDecisionKind::NeedToClarify | GateDecisionKind::TooBroad => "need_to_clarify",
        GateDecisionKind::Blocked => "need_human_input",
        GateDecisionKind::DuplicateAlreadyCovered => "done",
        GateDecisionKind::Ready | GateDecisionKind::ReadyWithAssumptions => "todo",
    }
}

fn read_source_arg(inline: Option<String>, file: Option<PathBuf>) -> Result<String, String> {
    match (inline, file) {
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display())),
        _ => Err(usage()),
    }
}

fn read_optional_file(file: Option<PathBuf>) -> Result<Option<String>, String> {
    file.map(read_required_file).transpose()
}

fn read_required_file(path: PathBuf) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn parse_project_field_assignments(
    values: Vec<String>,
) -> Result<Vec<ProjectFieldAssignment>, String> {
    values
        .into_iter()
        .map(|value| ProjectFieldAssignment::parse(&value).map_err(|error| error.to_string()))
        .collect()
}

fn validate_optional_forge_skill(skill: Option<String>) -> Result<Option<String>, String> {
    if let Some(key) = skill.as_deref() {
        if find_issue_skill(key).is_none() {
            return Err(format!("unknown Issue Forge skill: {key}"));
        }
    }
    Ok(skill)
}

fn print_forge_validation(report: &jade_symphony::issue_forge::ForgeValidationReport) {
    println!("title={}", report.title);
    println!("gate={:?}", report.decision.kind);
    println!("dispatchable={}", report.decision.is_dispatchable());
    if !report.decision.missing.is_empty() {
        println!("missing={}", report.decision.missing.join(", "));
    }
    if !report.decision.assumptions.is_empty() {
        println!("assumptions={}", report.decision.assumptions.join("; "));
    }
    if let Some(question) = &report.question {
        println!("question={}", question.question);
        println!("why={}", question.why_it_matters);
    }
}

fn print_interactive_forge_report(report: &jade_symphony::issue_forge::InteractiveForgeReport) {
    println!("skill={}", report.selected_skill.key);
    print_forge_validation(&report.validation);
    if let Some(question) = &report.question {
        println!("clarification_question={}", question.question);
        println!("clarification_why={}", question.why_it_matters);
    } else {
        println!("clarification_question=none");
    }
    println!("\n--- issue draft ---\n");
    println!("{}", report.issue_markdown);
}

fn usage() -> String {
    let mut command = Cli::command();
    command.render_long_help().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jade_symphony::tracker::MemoryTracker;
    use std::cell::RefCell;

    fn forge_contract() -> String {
        [
            "## Issue Goal",
            "Create a validated tracker issue.",
            "## Why Now",
            "Now.",
            "## Issue Context",
            "Context.",
            "## Non-Negotiable Guardrails",
            "- Guard.",
            "## Scope",
            "Scope.",
            "## Canonical References",
            "### Target Repository / Package",
            "- Alive24/jade-symphony",
            "## Verification",
            "### Completion Criteria",
            "- Pass.",
            "### Functional Verification",
            "- `cargo test`",
        ]
        .join("\n")
    }

    fn parse(args: &[&str]) -> Command {
        Command::parse(args.iter().map(|arg| arg.to_string()).collect()).unwrap()
    }

    fn test_config() -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: memory\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn live_github_config(allow_unassigned: bool) -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            &format!(
                "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  assignee_filter:\n    allow_unassigned: {}\n---\nPrompt",
                allow_unassigned
            ),
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn fixture_github_config() -> RuntimeConfig {
        let workflow = WorkflowDefinition::parse(
            "/tmp/WORKFLOW.md",
            "---\ntracker:\n  kind: github_project_v2\n  owner: Alive24\n  repo: jade-symphony\n  project_owner: Alive24\n  project_number: 9\n  fixture_path: fixtures/dry-run-issues.json\n---\nPrompt",
        )
        .unwrap();
        RuntimeConfig::from_workflow(&workflow, Path::new("/tmp/WORKFLOW.md")).unwrap()
    }

    fn tracker_issue(state: &str) -> TrackerIssue {
        TrackerIssue {
            tracker_kind: "memory".into(),
            id: "ISSUE_29".into(),
            item_id: None,
            identifier: "#29".into(),
            title: "Wire runtime state persistence into run-loop".into(),
            description: None,
            url: None,
            state: state.into(),
            labels: Vec::new(),
            assignees: Vec::new(),
            priority: None,
            branch_name: Some("feature/issue-29-runtime-state-run-loop".into()),
            linked_pull_requests: Vec::new(),
            blocked_by: Vec::new(),
            project_fields: Default::default(),
            created_at: None,
            updated_at: None,
        }
    }

    fn tracker_issue_with_ref(identifier: &str, title: &str, state: &str) -> TrackerIssue {
        let mut issue = tracker_issue(state);
        issue.identifier = identifier.into();
        issue.title = title.into();
        issue.branch_name = None;
        issue
    }

    #[derive(Default)]
    struct RecordingAdapter {
        operations: RefCell<Vec<String>>,
        fail_workpad: bool,
        fail_link_pr: bool,
    }

    impl RecordingAdapter {
        fn operations(&self) -> Vec<String> {
            self.operations.borrow().clone()
        }
    }

    impl TrackerAdapter for RecordingAdapter {
        fn kind(&self) -> &'static str {
            "recording"
        }

        fn list_dispatchable_issues(
            &self,
        ) -> Result<Vec<TrackerIssue>, jade_symphony::tracker::TrackerError> {
            Ok(Vec::new())
        }

        fn get_issue(
            &self,
            _issue_ref: &str,
        ) -> Result<Option<TrackerIssue>, jade_symphony::tracker::TrackerError> {
            Ok(None)
        }

        fn fetch_issues_by_states(
            &self,
            _states: &[String],
        ) -> Result<Vec<TrackerIssue>, jade_symphony::tracker::TrackerError> {
            Ok(Vec::new())
        }

        fn set_state(
            &self,
            issue_ref: &str,
            normalized_state: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            self.operations
                .borrow_mut()
                .push(format!("set_state:{issue_ref}:{normalized_state}"));
            Ok(())
        }

        fn upsert_workpad(
            &self,
            issue_ref: &str,
            markdown: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if self.fail_workpad {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "workpad failed".into(),
                    ),
                );
            }
            assert!(
                markdown.contains("## Rework Diagnostic")
                    || markdown.contains("### Merge Lane Handoff")
            );
            self.operations
                .borrow_mut()
                .push(format!("workpad:{issue_ref}"));
            Ok(())
        }

        fn create_follow_up_issue(
            &self,
            _input: FollowUpIssueInput,
        ) -> Result<String, jade_symphony::tracker::TrackerError> {
            Ok("dry-run:follow-up".into())
        }

        fn add_issue_to_project(
            &self,
            _issue_id: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            Ok(())
        }

        fn link_pull_request(
            &self,
            issue_ref: &str,
            pr_ref: &str,
        ) -> Result<(), jade_symphony::tracker::TrackerError> {
            if self.fail_link_pr {
                return Err(
                    jade_symphony::tracker::TrackerError::IntegrationUnavailable(
                        "link failed".into(),
                    ),
                );
            }
            self.operations
                .borrow_mut()
                .push(format!("link_pr:{issue_ref}:{pr_ref}"));
            Ok(())
        }

        fn list_linked_pull_requests(
            &self,
            _issue_ref: &str,
        ) -> Result<
            Vec<jade_symphony::model::LinkedPullRequest>,
            jade_symphony::tracker::TrackerError,
        > {
            Ok(Vec::new())
        }

        fn close_issue(&self, issue_ref: &str) -> Result<(), jade_symphony::tracker::TrackerError> {
            self.operations
                .borrow_mut()
                .push(format!("close_issue:{issue_ref}"));
            Ok(())
        }
    }

    fn active_runtime_state(identifier: &str) -> RuntimeState {
        let mut state = RuntimeState::active(
            RuntimeIssueState {
                id: "ISSUE_29".into(),
                identifier: identifier.into(),
            },
            "dry-run",
        );
        state.updated_at_ms = Some(1_000);
        state
    }

    #[test]
    fn clap_parser_preserves_default_plan_compatibility() {
        assert_eq!(
            parse(&[]),
            Command::Plan {
                workflow_path: PathBuf::from("WORKFLOW.md"),
                json: false,
            }
        );
        assert_eq!(
            parse(&["examples/dry-run-workflow.md"]),
            Command::Plan {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                json: false,
            }
        );
    }

    #[test]
    fn clap_parser_keeps_operator_command_aliases() {
        assert_eq!(
            parse(&["status", "examples/dry-run-workflow.md"]),
            Command::Plan {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                json: false,
            }
        );
        assert_eq!(
            parse(&["validate-workflow", "examples/dry-run-workflow.md"]),
            Command::Validate {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md")
            }
        );
        assert_eq!(
            parse(&["audit-project", "examples/dry-run-workflow.md"]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                    json: false,
                    strict: false,
                }
            }
        );
        assert_eq!(
            parse(&["profiles", "examples/dry-run-workflow.md"]),
            Command::Profiles {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md")
            }
        );
    }

    #[test]
    fn parses_status_json_flag() {
        assert_eq!(
            parse(&["status", "examples/dry-run-workflow.md", "--json"]),
            Command::Plan {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                json: true,
            }
        );
    }

    #[test]
    fn parses_doctor_repair_human_review_command() {
        assert_eq!(
            parse(&[
                "doctor-repair-human-review",
                "examples/github-project-workflow.md",
                "--dry-run"
            ]),
            Command::DoctorRepairHumanReview {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: false
            }
        );
        assert_eq!(
            parse(&[
                "doctor-repair-human-review",
                "examples/github-project-workflow.md",
                "--write"
            ]),
            Command::DoctorRepairHumanReview {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: true
            }
        );
    }

    #[test]
    fn renders_plan_snapshot_as_json_when_requested() {
        let snapshot = jade_symphony::model::RuntimeSnapshot {
            event_log_path: Some("/tmp/jade-symphony.jsonl".into()),
            integration_gaps: vec!["gap".into()],
            ..Default::default()
        };

        let rendered = render_plan_snapshot(&snapshot, true).unwrap();
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(
            value
                .get("event_log_path")
                .and_then(serde_json::Value::as_str),
            Some("/tmp/jade-symphony.jsonl")
        );
        assert_eq!(
            value
                .pointer("/integration_gaps/0")
                .and_then(serde_json::Value::as_str),
            Some("gap")
        );
    }

    #[test]
    fn parses_doctor_json_and_strict_flags() {
        assert_eq!(
            parse(&[
                "doctor",
                "examples/github-project-workflow.md",
                "--json",
                "--strict"
            ]),
            Command::Doctor {
                options: DoctorOptions {
                    workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                    json: true,
                    strict: true,
                }
            }
        );
    }

    #[test]
    fn parses_status_api_command() {
        assert_eq!(
            parse(&[
                "status-api",
                "examples/dry-run-workflow.md",
                "--bind",
                "127.0.0.1:0",
                "--once"
            ]),
            Command::StatusApi {
                workflow_path: PathBuf::from("examples/dry-run-workflow.md"),
                bind: "127.0.0.1:0".parse().unwrap(),
                once: true,
            }
        );
    }

    #[test]
    fn parses_dogfood_smoke_command() {
        assert_eq!(
            parse(&[
                "dogfood-smoke",
                "examples/github-project-workflow.md",
                "--dry-run"
            ]),
            Command::DogfoodSmoke {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: false
            }
        );
        assert_eq!(
            parse(&[
                "dogfood-smoke",
                "examples/github-project-workflow.md",
                "--write"
            ]),
            Command::DogfoodSmoke {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: true
            }
        );
    }

    #[test]
    fn parses_cleanup_workspaces_command() {
        assert_eq!(
            parse(&[
                "cleanup-workspaces",
                "examples/github-project-workflow.md",
                "--write"
            ]),
            Command::CleanupWorkspaces {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: true,
            }
        );
        assert_eq!(
            parse(&["workspace-cleanup", "examples/github-project-workflow.md"]),
            Command::CleanupWorkspaces {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: false,
            }
        );
    }

    #[test]
    fn workspace_cleanup_plan_marks_terminal_existing_workspace_eligible() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.workspace.root = temp.path().join("workspaces");
        let issue = tracker_issue("Done");
        let handoff =
            plan_issue_handoff_for_profile(&config.workspace.root, &issue, "main", None).unwrap();
        std::fs::create_dir_all(&handoff.workspace_path).unwrap();

        let entries = workspace_cleanup_plan(&config, &[issue]).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].issue_ref, "#29");
        assert_eq!(entries[0].workspace_key, handoff.workspace_key);
        assert_eq!(entries[0].action, WorkspaceCleanupAction::Eligible);
    }

    #[test]
    fn workspace_cleanup_plan_skips_non_terminal_and_missing_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = test_config();
        config.workspace.root = temp.path().join("workspaces");
        let mut active = tracker_issue("In Progress");
        active.identifier = "#30".into();
        active.title = "Active workspace".into();
        active.branch_name = None;
        let missing_terminal = tracker_issue("Done");

        let entries = workspace_cleanup_plan(&config, &[active, missing_terminal]).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].action,
            WorkspaceCleanupAction::Skipped {
                reason: "non_terminal_state".into()
            }
        );
        assert_eq!(
            entries[1].action,
            WorkspaceCleanupAction::Skipped {
                reason: "workspace_missing".into()
            }
        );
    }

    #[test]
    fn all_mapped_tracker_states_includes_merging_for_doctor() {
        let config = test_config();
        let states = all_mapped_tracker_states(&config);

        assert!(states.contains(&"Merging".to_string()));
        assert!(states.contains(&"Rework".to_string()));
        assert!(states.contains(&"Done".to_string()));
    }

    #[test]
    fn controlled_smoke_issue_requires_marker_label_or_title() {
        let mut issue = tracker_issue("Todo");
        assert!(!is_controlled_dogfood_smoke_issue(&issue));

        issue.labels = vec!["dogfood-smoke".into()];
        assert!(is_controlled_dogfood_smoke_issue(&issue));

        issue.labels.clear();
        issue.title = "[dogfood-smoke] controlled run".into();
        assert!(is_controlled_dogfood_smoke_issue(&issue));
    }

    #[test]
    fn clap_parser_treats_help_flags_as_successful_help() {
        assert_eq!(parse(&["--help"]), Command::Help);
        assert_eq!(parse(&["-h"]), Command::Help);
    }

    #[test]
    fn clap_parser_preserves_write_intent_for_mutating_commands() {
        assert_eq!(
            parse(&[
                "set-state",
                "examples/github-project-workflow.md",
                "#4",
                "agent_review",
                "--write"
            ]),
            Command::SetState {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#4".into(),
                state: "agent_review".into(),
                write: true
            }
        );
    }

    #[test]
    fn clap_parser_preserves_review_outcome_mapping() {
        assert_eq!(
            parse(&[
                "review-fake",
                "examples/github-project-workflow.md",
                "#4",
                "--outcome",
                "confirmed",
                "--write"
            ]),
            Command::ReviewFake {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                issue_ref: "#4".into(),
                outcome: FakeReviewOutcome::ConfirmedFinding,
                write: true
            }
        );
    }

    #[test]
    fn parses_review_freshness_command() {
        let command = Command::parse(vec![
            "review-freshness".into(),
            "--issue".into(),
            "#33".into(),
            "--prior-head".into(),
            "old-head".into(),
            "--current-head".into(),
            "new-head".into(),
            "--prior-base".into(),
            "old-base".into(),
            "--current-base".into(),
            "new-base".into(),
            "--changed-file".into(),
            "docs/dogfood-readiness.md".into(),
            "--stale-reason".into(),
            "merge-conflict".into(),
            "--rework-class".into(),
            "mechanical-conflict-resolution".into(),
            "--patch-summary".into(),
            "Resolved conflict without semantic changes.".into(),
        ])
        .unwrap();

        let Command::ReviewFreshness { input } = command else {
            panic!("expected review-freshness command");
        };

        assert_eq!(input.issue_ref, "#33");
        assert_eq!(input.changed_files, vec!["docs/dogfood-readiness.md"]);
        assert_eq!(input.stale_reason, ReviewStaleReason::MergeConflict);
        assert_eq!(
            input.rework_class,
            ReviewReworkClass::MechanicalConflictResolution
        );
        assert!(input.patch_summary.unwrap().contains("Resolved conflict"));
    }

    #[test]
    fn parses_review_loop_flags() {
        let command = Command::parse(vec![
            "review-loop".into(),
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
            panic!("expected review-loop command");
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
            "review-loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "4".into(),
            "--once".into(),
        ])
        .unwrap();

        let Command::ReviewLoop { options } = command else {
            panic!("expected review-loop command");
        };

        assert_eq!(options.iteration_limit(), Some(1));
    }

    #[test]
    fn parses_merge_loop_flags() {
        let command = Command::parse(vec![
            "merge-loop".into(),
            "examples/github-project-workflow.md".into(),
            "--max-iterations".into(),
            "3".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::MergeLoop { options } = command else {
            panic!("expected merge-loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/github-project-workflow.md")
        );
        assert_eq!(options.max_iterations, Some(3));
        assert!(options.write);
    }

    #[test]
    fn merge_loop_once_overrides_max_iterations() {
        let command = Command::parse(vec![
            "merge-loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "4".into(),
            "--once".into(),
        ])
        .unwrap();

        let Command::MergeLoop { options } = command else {
            panic!("expected merge-loop command");
        };

        assert_eq!(options.iteration_limit(), Some(1));
    }

    #[test]
    fn rejects_unbounded_merge_loop_for_now() {
        assert!(Command::parse(vec!["merge-loop".into(), "WORKFLOW.md".into()]).is_err());
    }

    #[test]
    fn rejects_zero_merge_loop_iterations() {
        assert!(Command::parse(vec![
            "merge-loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "0".into(),
        ])
        .is_err());
    }

    #[test]
    fn review_worker_selection_respects_concurrency_limit() {
        let selected = select_review_worker_issues(
            &[
                tracker_issue_with_ref("#67", "First review", "Agent Review"),
                tracker_issue_with_ref("#68", "Second review", "Agent Review"),
                tracker_issue_with_ref("#69", "Third review", "Agent Review"),
            ],
            "Agent Review",
            "fake-reviewer",
            2,
        );

        assert_eq!(
            selected
                .iter()
                .map(|issue| issue.identifier.as_str())
                .collect::<Vec<_>>(),
            vec!["#67", "#68"]
        );
    }

    #[test]
    fn review_worker_selection_skips_existing_worker_marker() {
        let mut queued = tracker_issue_with_ref("#67", "Queued review", "Agent Review");
        queued.project_fields.insert(
            "Review Worker".into(),
            serde_json::Value::String("queued review:#67:fake-reviewer".into()),
        );
        let ready = tracker_issue_with_ref("#68", "Ready review", "Agent Review");

        let selected =
            select_review_worker_issues(&[queued, ready], "Agent Review", "fake-reviewer", 2);

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identifier, "#68");
    }

    #[test]
    fn review_workspace_uses_issue_handoff_workspace() {
        let config = test_config();
        let issue =
            tracker_issue_with_ref("#67", "Add parallel review worker pool", "Agent Review");

        let workspace = review_workspace_for_issue(&config, &issue);

        assert!(workspace.ends_with("issue-67-add-parallel-review-worker-pool"));
    }

    #[test]
    fn parses_run_loop_flags() {
        let command = Command::parse(vec![
            "run-loop".into(),
            "examples/dry-run-workflow.md".into(),
            "--max-iterations".into(),
            "3".into(),
            "--dry-run".into(),
        ])
        .unwrap();

        let Command::RunLoop { options } = command else {
            panic!("expected run-loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("examples/dry-run-workflow.md")
        );
        assert_eq!(options.max_iterations, Some(3));
        assert!(!options.once);
        assert!(!options.write);
    }

    #[test]
    fn run_loop_once_overrides_max_iterations() {
        let command = Command::parse(vec![
            "run-loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "9".into(),
            "--once".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::RunLoop { options } = command else {
            panic!("expected run-loop command");
        };

        assert_eq!(options.iteration_limit(), Some(1));
        assert!(options.write);
    }

    #[test]
    fn parses_merge_once_command() {
        let command = Command::parse(vec![
            "merge-once".into(),
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

        let command = Command::parse(vec![
            "land".into(),
            "examples/github-project-workflow.md".into(),
            "--write".into(),
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::MergeOnce {
                workflow_path: PathBuf::from("examples/github-project-workflow.md"),
                write: true
            }
        );
    }

    #[test]
    fn rejects_zero_run_loop_iterations() {
        let error = Command::parse(vec![
            "run-loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "0".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn rejects_zero_review_loop_iterations() {
        let error = Command::parse(vec![
            "review-loop".into(),
            "WORKFLOW.md".into(),
            "--max-iterations".into(),
            "0".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn run_loop_claim_action_uses_tracker_claim_decision() {
        let config = test_config();

        assert_eq!(
            run_loop_claim_action(&tracker_issue("Todo"), &config),
            RunLoopClaimAction::Claim
        );
        assert_eq!(
            run_loop_claim_action(&tracker_issue("Rework"), &config),
            RunLoopClaimAction::Claim
        );
        assert_eq!(
            run_loop_claim_action(&tracker_issue("In Progress"), &config),
            RunLoopClaimAction::Resume
        );
        assert_eq!(
            run_loop_claim_action(&tracker_issue("Agent Review"), &config),
            RunLoopClaimAction::StopAndReplan {
                current_state: "Agent Review".into()
            }
        );
    }

    #[test]
    fn live_gate_blocks_missing_assignee_without_override() {
        let config = live_github_config(false);
        let issue = tracker_issue("Todo");

        assert_eq!(
            live_missing_assignee_gate_blocker(&config, &issue).as_deref(),
            Some("live GitHub issue assignee")
        );
    }

    #[test]
    fn fixture_mode_does_not_require_live_assignee() {
        let config = fixture_github_config();
        let issue = tracker_issue("Todo");

        assert_eq!(live_missing_assignee_gate_blocker(&config, &issue), None);
        assert_eq!(
            run_loop_assignee_ownership_decision(&issue, &config, None, None),
            AssigneeOwnershipDecision::Allowed
        );
    }

    #[test]
    fn assignee_ownership_allows_matching_active_login() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["CodexUser".into()];

        assert_eq!(
            run_loop_assignee_ownership_decision(&issue, &config, Some("codexuser"), None),
            AssigneeOwnershipDecision::Allowed
        );
    }

    #[test]
    fn assignee_ownership_blocks_mismatched_active_login() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["owner-a".into()];

        let decision = run_loop_assignee_ownership_decision(&issue, &config, Some("owner-b"), None);

        assert!(matches!(decision, AssigneeOwnershipDecision::Block { .. }));
    }

    #[test]
    fn assignee_ownership_allows_matching_profile_login() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["profile-owner".into()];

        assert_eq!(
            run_loop_assignee_ownership_decision(
                &issue,
                &config,
                Some("different-gh-user"),
                Some("profile-owner"),
            ),
            AssigneeOwnershipDecision::Allowed
        );
    }

    #[test]
    fn assignee_ownership_blocks_missing_active_identity() {
        let config = live_github_config(false);
        let mut issue = tracker_issue("Todo");
        issue.assignees = vec!["owner-a".into()];

        let decision = run_loop_assignee_ownership_decision(&issue, &config, None, None);

        assert_eq!(
            decision,
            AssigneeOwnershipDecision::Block {
                reason: "active GitHub identity unavailable for assignee ownership check".into(),
            }
        );
    }

    #[test]
    fn run_loop_runtime_ownership_workpad_records_matching_marker() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let ownership = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();

        let workpad = run_loop_ownership_workpad(&issue, &ownership, "Resumed");

        assert!(workpad.contains("jade-symphony-runtime-ownership"));
        assert_eq!(
            runtime_ownership_decision(Some(&workpad), &ownership),
            RuntimeOwnershipDecision::Matches
        );
    }

    #[test]
    fn run_loop_runtime_ownership_detects_different_active_branch() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let expected = run_loop_runtime_ownership(&issue, &config, &handoff).unwrap();
        let mut existing = expected.clone();
        existing.branch_name = "feature/issue-100-other-work".into();
        let workpad = render_runtime_ownership_marker(&existing);

        assert!(matches!(
            runtime_ownership_decision(Some(&workpad), &expected),
            RuntimeOwnershipDecision::Mismatched { .. }
        ));
    }

    #[test]
    fn resume_preflight_continues_active_in_progress_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert_eq!(action, ResumePreflightAction::Continue);
    }

    #[test]
    fn resume_preflight_blocks_conflicting_tracker_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("Todo")]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert!(matches!(action, ResumePreflightAction::Block { .. }));
    }

    #[test]
    fn resume_preflight_defers_until_retry_is_due() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        record_runtime_retry(&mut state, 1_000, 5_000, "rate limited");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert!(matches!(
            action,
            ResumePreflightAction::RetryLater {
                due_in_ms: 4_000,
                ..
            }
        ));
    }

    #[test]
    fn resume_preflight_detects_stalled_active_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("In Progress")]);
        let mut state = active_runtime_state("#29");
        state.updated_at_ms = Some(1_000);

        let action = run_loop_resume_preflight(
            &tracker,
            &config,
            Some(&state),
            config.codex.stall_timeout_ms + 2_000,
        )
        .unwrap();

        assert!(matches!(action, ResumePreflightAction::Stalled { .. }));
    }

    #[test]
    fn resume_preflight_clears_completed_tracker_state() {
        let config = test_config();
        let tracker = MemoryTracker::new(vec![tracker_issue("Agent Review")]);
        let state = active_runtime_state("#29");

        let action = run_loop_resume_preflight(&tracker, &config, Some(&state), 2_000).unwrap();

        assert_eq!(
            action,
            ResumePreflightAction::ClearCompleted {
                issue_identifier: "#29".into()
            }
        );
    }

    #[test]
    fn run_loop_runtime_state_increments_same_issue_attempts() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let existing = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed");

        let state = run_loop_runtime_state_for_issue(Some(&existing), &issue, &config, "Resumed");

        assert_eq!(state.attempt_count, 2);
        assert_eq!(
            state
                .active_issue
                .as_ref()
                .map(|issue| issue.identifier.as_str()),
            Some("#29")
        );
        assert_eq!(state.branch_name, issue.branch_name);
        assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
        assert_eq!(state.actor_label.as_deref(), Some("Jade Symphony Agent"));
        assert_eq!(state.last_event.as_deref(), Some("Resumed"));
    }

    #[test]
    fn run_loop_runtime_state_records_result_and_transition() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let state = run_loop_runtime_state_for_issue(None, &issue, &config, "Claimed");
        let result = IssueExecutionResult {
            workspace_path: PathBuf::from("/tmp/jade/issue-29"),
            backend: "dry-run".into(),
            profile_id: Some("codex-alpha".into()),
            instance_name: Some("Codex Alpha".into()),
            success: true,
            session_id: Some("session-29".into()),
            message: "ok".into(),
            usage_limit_pause: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let state = run_loop_runtime_state_with_result(state, &result);
        assert_eq!(state.workspace_path, Some(result.workspace_path));
        assert_eq!(state.backend_session_id.as_deref(), Some("session-29"));
        assert_eq!(state.profile_id.as_deref(), Some("codex-alpha"));
        assert_eq!(state.actor_role.as_deref(), Some("implementation_agent"));
        assert_eq!(
            state.git_author.as_deref(),
            Some("Jade Symphony Agent <jade@example.invalid>")
        );
        assert_eq!(state.last_event.as_deref(), Some("Completed"));

        let state = run_loop_runtime_state_with_transition(
            state,
            Some("In Progress".into()),
            "agent_review",
            "main agent completed",
        );
        assert_eq!(
            state.last_transition,
            Some(RuntimeTransition {
                from: Some("In Progress".into()),
                to: "agent_review".into(),
                reason: "main agent completed".into(),
            })
        );
    }

    #[test]
    fn run_loop_handoff_plan_uses_issue_workspace_and_branch_plan() {
        let config = test_config();
        let issue = tracker_issue("In Progress");

        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();

        assert_eq!(
            handoff.workspace_key,
            "issue-29-wire-runtime-state-persistence-into-run-loop"
        );
        assert!(handoff
            .workspace_path
            .ends_with("issue-29-wire-runtime-state-persistence-into-run-loop"));
        assert_eq!(
            handoff.branch_name,
            "feature/issue-29-wire-runtime-state-persistence-into-run-loop"
        );
        assert_eq!(
            handoff.pull_request.title,
            "#29: Wire runtime state persistence into run-loop"
        );
        assert_eq!(handoff.pull_request.base_branch, "main");
    }

    #[test]
    fn run_loop_handoff_plan_rejects_branch_for_different_issue() {
        let config = test_config();
        let mut issue = tracker_issue("In Progress");
        issue.branch_name = Some("feature/issue-99-other-work".into());

        let error = run_loop_handoff_plan(&config, &issue).unwrap_err();

        assert!(matches!(
            error,
            HandoffError::BranchIssueMismatch {
                expected_issue,
                found_issue,
                ..
            } if expected_issue == "29" && found_issue == "99"
        ));
    }

    #[test]
    fn run_loop_handoff_workpad_records_planned_pr_evidence() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            session_id: Some("session-33".into()),
            message: "ok".into(),
            usage_limit_pause: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: Some(RunLoopLiveHandoff {
                worktree: LiveWorktreeResult {
                    workspace_path: handoff.workspace_path.clone(),
                    branch_name: handoff.branch_name.clone(),
                    created: true,
                },
                publication: PullRequestPublication {
                    branch_pushed: true,
                    pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                    pr_created: true,
                },
                verification: "skipped:not_configured".into(),
            }),
            handoff_verification: Some("skipped:not_configured".into()),
        };

        let workpad = run_loop_handoff_workpad(&issue, &result, &handoff);

        assert!(workpad.contains("### Planned Handoff"));
        assert!(workpad.contains("Actor role: `implementation_agent`"));
        assert!(
            workpad.contains("Git identity: `applied:Jade Symphony Agent <jade@example.invalid>`")
        );
        assert!(workpad
            .contains("Workspace key: `issue-29-wire-runtime-state-persistence-into-run-loop`"));
        assert!(workpad
            .contains("Branch: `feature/issue-29-wire-runtime-state-persistence-into-run-loop`"));
        assert!(workpad.contains("PR title: `#29: Wire runtime state persistence into run-loop`"));
        assert!(workpad.contains("Handoff verification: `skipped:not_configured`"));
        assert!(workpad.contains("Live PR: `https://github.com/Alive24/jade-symphony/pull/45`"));
    }

    #[test]
    fn live_run_loop_handoff_records_pr_link_through_tracker() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut result = successful_live_handoff_result(&handoff);
        let adapter = RecordingAdapter::default();

        assert!(apply_live_handoff_pr_link(
            &adapter,
            &issue.identifier,
            &mut result
        ));

        assert!(result.success);
        assert_eq!(
            adapter.operations(),
            vec!["link_pr:#29:https://github.com/Alive24/jade-symphony/pull/45"]
        );
    }

    #[test]
    fn handoff_verification_skips_when_not_configured() {
        let config = test_config();
        let temp = tempfile::tempdir().unwrap();

        let verification = run_handoff_verification(temp.path(), &config);

        assert!(verification.success);
        assert_eq!(verification.summary, "skipped:not_configured");
    }

    #[test]
    fn handoff_verification_runs_configured_commands() {
        let mut config = test_config();
        config.verification.commands = vec!["printf verified > verification.txt".into()];
        config.verification.timeout_ms = 5_000;
        let temp = tempfile::tempdir().unwrap();

        let verification = run_handoff_verification(temp.path(), &config);

        assert!(verification.success);
        assert_eq!(verification.summary, "passed:1 command(s)");
        assert_eq!(
            std::fs::read_to_string(temp.path().join("verification.txt")).unwrap(),
            "verified"
        );
    }

    #[test]
    fn live_run_loop_handoff_link_failure_blocks_agent_review() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let mut result = successful_live_handoff_result(&handoff);
        let adapter = RecordingAdapter {
            operations: RefCell::new(Vec::new()),
            fail_workpad: false,
            fail_link_pr: true,
        };

        assert!(!apply_live_handoff_pr_link(
            &adapter,
            &issue.identifier,
            &mut result
        ));

        assert!(!result.success);
        assert!(result.message.contains("handoff PR link failed"));
    }

    fn successful_live_handoff_result(handoff: &IssueHandoffPlan) -> IssueExecutionResult {
        IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            session_id: Some("session-33".into()),
            message: "ok".into(),
            usage_limit_pause: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: Some(RunLoopLiveHandoff {
                worktree: LiveWorktreeResult {
                    workspace_path: handoff.workspace_path.clone(),
                    branch_name: handoff.branch_name.clone(),
                    created: true,
                },
                publication: PullRequestPublication {
                    branch_pushed: true,
                    pr_url: "https://github.com/Alive24/jade-symphony/pull/45".into(),
                    pr_created: true,
                },
                verification: "skipped:not_configured".into(),
            }),
            handoff_verification: Some("skipped:not_configured".into()),
        }
    }

    #[test]
    fn handoff_verification_failure_blocks_success() {
        let mut config = test_config();
        config.verification.commands = vec!["echo nope >&2; exit 7".into()];
        config.verification.timeout_ms = 5_000;
        let temp = tempfile::tempdir().unwrap();

        let verification = run_handoff_verification(temp.path(), &config);

        assert!(!verification.success);
        assert!(verification.summary.contains("failed command=`echo nope"));
        assert!(verification.summary.contains("status 7"));
    }

    #[test]
    fn usage_limit_pause_workpad_preserves_tracker_state_boundary() {
        let issue = tracker_issue("In Progress");
        let result = IssueExecutionResult {
            workspace_path: PathBuf::from("/tmp/jade/issue-63"),
            backend: "codex".into(),
            profile_id: None,
            instance_name: None,
            success: false,
            session_id: Some("session-63".into()),
            message: "Codex subprocess exited with status 1".into(),
            usage_limit_pause: Some(UsageLimitPause {
                classifier: "usage_limit".into(),
                evidence: "usage limit reached".into(),
            }),
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::NotGitRepository,
                author: None,
                applied_keys: Vec::new(),
            },
            live_handoff: None,
            handoff_verification: None,
        };
        let pause = result.usage_limit_pause.as_ref().unwrap();
        let workpad = run_loop_usage_limit_pause_workpad(&issue, &result, pause, 20_000);

        assert!(workpad.contains("### Usage-Limit Pause"));
        assert!(workpad.contains("Classifier: `usage_limit`"));
        assert!(workpad.contains("Tracker state was not advanced to `Agent Review`"));
        assert!(workpad.contains("Retry backoff: `20000ms`"));
    }

    #[test]
    fn rework_transition_writes_diagnostic_before_state_change() {
        let adapter = RecordingAdapter::default();
        let issue = tracker_issue("Agent Review");
        let diagnostic = ReworkDiagnostic::validation_failure(
            issue.identifier.clone(),
            "cargo test",
            "failing test output",
        );

        transition_issue_to_rework_with_diagnostic(&adapter, &issue, &diagnostic).unwrap();

        assert_eq!(
            adapter.operations(),
            vec![
                "workpad:#29".to_string(),
                "set_state:#29:rework".to_string()
            ]
        );
    }

    #[test]
    fn rework_transition_does_not_set_state_when_workpad_write_fails() {
        let adapter = RecordingAdapter {
            operations: RefCell::new(Vec::new()),
            fail_workpad: true,
            fail_link_pr: false,
        };
        let issue = tracker_issue("Agent Review");
        let diagnostic = ReworkDiagnostic::validation_failure(
            issue.identifier.clone(),
            "cargo test",
            "failing test output",
        );

        assert!(transition_issue_to_rework_with_diagnostic(&adapter, &issue, &diagnostic).is_err());
        assert!(adapter.operations().is_empty());
    }

    #[test]
    fn merge_completion_closes_issue_after_workpad_and_done_state() {
        let adapter = RecordingAdapter::default();
        let issue = tracker_issue("Merging");
        let workpad = "## Jade Symphony Workpad\n\n### Merge Lane Handoff\n";

        record_done_merge_lane_completion(&adapter, &issue, workpad).unwrap();

        assert_eq!(
            adapter.operations(),
            vec![
                "workpad:#29".to_string(),
                "set_state:#29:done".to_string(),
                "close_issue:#29".to_string()
            ]
        );
    }

    #[test]
    fn run_loop_agent_review_handoff_blocks_missing_pr_url() {
        let config = test_config();
        let issue = tracker_issue("In Progress");
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            session_id: Some("session-57".into()),
            message: "ok".into(),
            usage_limit_pause: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let evidence = run_loop_agent_review_handoff_evidence(&issue, &result, &handoff);
        let report = evaluate_agent_review_handoff(&evidence);

        assert!(!report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("need_human_input"));
        assert!(evidence
            .no_pr_blocker
            .unwrap()
            .contains("No pull request URL"));
    }

    #[test]
    fn run_loop_agent_review_handoff_passes_with_pr_url() {
        let config = test_config();
        let mut issue = tracker_issue("In Progress");
        issue
            .linked_pull_requests
            .push(jade_symphony::model::LinkedPullRequest {
                id: Some("PR_57".into()),
                number: Some(57),
                url: Some("https://github.com/Alive24/jade-symphony/pull/57".into()),
                state: Some("OPEN".into()),
            });
        let handoff = run_loop_handoff_plan(&config, &issue).unwrap();
        let result = IssueExecutionResult {
            workspace_path: handoff.workspace_path.clone(),
            backend: "dry-run".into(),
            profile_id: None,
            instance_name: None,
            success: true,
            session_id: Some("session-57".into()),
            message: "ok".into(),
            usage_limit_pause: None,
            actor_role: "implementation_agent".into(),
            actor_label: "Jade Symphony Agent".into(),
            git_author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
            git_identity: GitIdentityApplyResult {
                status: jade_symphony::workspace::GitIdentityApplyStatus::Applied,
                author: Some("Jade Symphony Agent <jade@example.invalid>".into()),
                applied_keys: vec!["user.name".into(), "user.email".into()],
            },
            live_handoff: None,
            handoff_verification: None,
        };

        let evidence = run_loop_agent_review_handoff_evidence(&issue, &result, &handoff);
        let report = evaluate_agent_review_handoff(&evidence);

        assert!(report.is_ready());
        assert_eq!(report.target_state.as_deref(), Some("agent_review"));
        assert_eq!(
            evidence.pull_request_url.as_deref(),
            Some("https://github.com/Alive24/jade-symphony/pull/57")
        );
    }

    #[test]
    fn parses_forge_create_flags() {
        let command = Command::parse(vec![
            "forge-create".into(),
            "--workflow".into(),
            "examples/dry-run-workflow.md".into(),
            "--title".into(),
            "Create issue".into(),
            "--body".into(),
            forge_contract(),
            "--add-to-project".into(),
            "--project-field".into(),
            "Capability=CLI".into(),
            "--write".into(),
        ])
        .unwrap();

        let Command::ForgeCreate {
            workflow_path,
            title,
            markdown,
            add_to_project,
            project_fields,
            write,
        } = command
        else {
            panic!("expected forge-create command");
        };

        assert_eq!(workflow_path, PathBuf::from("examples/dry-run-workflow.md"));
        assert_eq!(title, "Create issue");
        assert!(markdown.contains("## Issue Goal"));
        assert!(add_to_project);
        assert_eq!(
            project_fields,
            vec![ProjectFieldAssignment {
                name: "Capability".into(),
                value: "CLI".into()
            }]
        );
        assert!(write);
    }

    #[test]
    fn forge_create_project_fields_require_project_add() {
        let error = forge_create(
            PathBuf::from("missing-workflow.md"),
            "Create issue".into(),
            forge_contract(),
            false,
            vec![ProjectFieldAssignment {
                name: "Capability".into(),
                value: "CLI".into(),
            }],
            true,
        )
        .unwrap_err()
        .to_string();

        assert_eq!(
            error,
            "forge-create --project-field requires --add-to-project"
        );
    }

    #[test]
    fn parses_forge_interactive_flags() {
        let command = Command::parse(vec![
            "forge-interactive".into(),
            "--workflow".into(),
            "examples/github-project-workflow.md".into(),
            "--title".into(),
            "Add resume preflight".into(),
            "--intent".into(),
            "run-loop should inspect runtime state before claiming new work".into(),
            "--skill".into(),
            "runtime".into(),
            "--add-to-project".into(),
            "--write".into(),
            "--confirm-create".into(),
        ])
        .unwrap();

        let Command::ForgeInteractive { options } = command else {
            panic!("expected forge-interactive command");
        };

        assert_eq!(
            options.workflow_path,
            Some(PathBuf::from("examples/github-project-workflow.md"))
        );
        assert_eq!(options.title, "Add resume preflight");
        assert!(options.intent.contains("runtime state"));
        assert_eq!(options.skill.as_deref(), Some("runtime"));
        assert!(options.add_to_project);
        assert!(options.write);
        assert!(options.confirm_create);
    }

    #[test]
    fn rejects_unknown_forge_skill() {
        let error = Command::parse(vec![
            "forge-interactive".into(),
            "--title".into(),
            "Add a thing".into(),
            "--intent".into(),
            "make the runtime loop safer".into(),
            "--skill".into(),
            "product-roadmap".into(),
        ])
        .unwrap_err();

        assert!(error.contains("unknown Issue Forge skill"));
    }

    #[test]
    fn rejects_forge_create_with_both_body_and_file() {
        let error = Command::parse(vec![
            "forge-create".into(),
            "--workflow".into(),
            "WORKFLOW.md".into(),
            "--title".into(),
            "Create issue".into(),
            "--body".into(),
            forge_contract(),
            "--file".into(),
            "issue.md".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn validates_forge_create_contract_before_tracker_write() {
        let config = test_config();
        assert!(validate_forge_create_contract("Create issue", &forge_contract(), &config).is_ok());

        let error =
            validate_forge_create_contract("Thin issue", "make it better", &config).unwrap_err();
        assert!(error.contains("tracker issue was not created"));
    }

    #[test]
    fn forge_create_can_use_memory_tracker_adapter() {
        let temp = tempfile::tempdir().unwrap();
        let workflow_path = temp.path().join("WORKFLOW.md");
        std::fs::write(
            &workflow_path,
            "---\ntracker:\n  kind: memory\nobservability:\n  logs_root: log\n---\nPrompt",
        )
        .unwrap();

        forge_create(
            workflow_path,
            "Create issue".into(),
            forge_contract(),
            true,
            Vec::new(),
            true,
        )
        .unwrap();
    }

    #[test]
    fn no_dispatch_stops_for_dry_run_even_without_limit() {
        let options = RunLoopOptions {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            max_iterations: None,
            once: false,
            write: false,
        };

        assert_eq!(
            no_dispatch_action(&options, options.iteration_limit(), 250),
            NoDispatchAction::Stop {
                reason: "no_dispatchable_issue"
            }
        );
    }

    #[test]
    fn no_dispatch_stops_for_bounded_write_loop() {
        let options = RunLoopOptions {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            max_iterations: Some(2),
            once: false,
            write: true,
        };

        assert_eq!(
            no_dispatch_action(&options, options.iteration_limit(), 250),
            NoDispatchAction::Stop {
                reason: "no_dispatchable_issue"
            }
        );
    }

    #[test]
    fn no_dispatch_sleeps_for_unbounded_write_loop() {
        let options = RunLoopOptions {
            workflow_path: PathBuf::from("WORKFLOW.md"),
            max_iterations: None,
            once: false,
            write: true,
        };

        assert_eq!(
            no_dispatch_action(&options, options.iteration_limit(), 250),
            NoDispatchAction::SleepAndContinue { delay_ms: 250 }
        );
    }
}
