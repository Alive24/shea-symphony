use std::path::{Path, PathBuf};

use jade_symphony::agent::backend_from_config;
use jade_symphony::config::RuntimeConfig;
use jade_symphony::event_log::{EventLog, EventRecord};
use jade_symphony::issue_forge::{
    discover_candidates, draft_from_template, repair_markdown, validate_markdown,
};
use jade_symphony::model::{normalize_state, GateDecision, GateDecisionKind, TrackerIssue};
use jade_symphony::orchestrator::Orchestrator;
use jade_symphony::prompt::render_prompt;
use jade_symphony::quality_gate::evaluate_issue;
use jade_symphony::review::{
    render_review_workpad, review_gate_decision, transition_allowed_for_main_agent,
    transition_allowed_for_review_agent, FakeReviewBackend, FakeReviewOutcome,
    GeminiCliReviewBackend, ReviewBackend, ReviewJob, ReviewRequest,
};
use jade_symphony::status_surface::render_snapshot;
use jade_symphony::tracker::{adapter_from_config, FollowUpIssueInput};
use jade_symphony::workflow::WorkflowDefinition;
use jade_symphony::workspace::{prepare_workspace, run_after_run, run_before_run};

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
        Command::Plan { workflow_path } => plan(workflow_path),
        Command::Validate { workflow_path } => validate(workflow_path),
        Command::Inspect { workflow_path } => inspect(workflow_path),
        Command::RunOnce { workflow_path } => run_once(workflow_path),
        Command::RunLoop { options } => run_loop(options),
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
            write,
        } => forge_create(workflow_path, title, markdown, add_to_project, write),
        Command::Help => {
            println!("{}", usage());
            Ok(())
        }
    }
}

fn plan(workflow_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let workflow = WorkflowDefinition::load(&workflow_path)?;
    let config = RuntimeConfig::from_workflow(&workflow, &workflow_path)?;
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

    println!("{}", render_snapshot(&plan.snapshot));

    Ok(())
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
    let decision = evaluate_issue(&issue);

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
    write: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    require_write_intent(write)?;
    let report = validate_forge_create_contract(&title, &markdown).inspect_err(|_message| {
        let report = validate_markdown(&title, &markdown);
        print_forge_validation(&report);
    })?;

    let config = load_config(&workflow_path)?;
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
    }

    println!("forge_create=ok issue_id={issue_id} added_to_project={add_to_project}");
    Ok(())
}

fn validate_forge_create_contract(
    title: &str,
    markdown: &str,
) -> Result<jade_symphony::issue_forge::ForgeValidationReport, String> {
    let report = validate_markdown(title, markdown);
    if report.decision.is_dispatchable() {
        Ok(report)
    } else {
        Err("issue forge validation failed; tracker issue was not created".into())
    }
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

fn apply_review_result(
    adapter: &dyn jade_symphony::tracker::TrackerAdapter,
    issue_ref: &str,
    issue: &TrackerIssue,
    job: &jade_symphony::review::ReviewJob,
) -> Result<(), Box<dyn std::error::Error>> {
    let decision = review_gate_decision(job);
    let workpad = render_review_workpad(issue, job);

    adapter.upsert_workpad(issue_ref, &workpad)?;
    if let Some(target_state) = decision.target_state {
        if !transition_allowed_for_review_agent(target_state, &decision) {
            return Err("review agent transition is not allowed for this review decision".into());
        }
        adapter.set_state(issue_ref, target_state)?;
    }
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
        let gate = evaluate_issue(&issue);
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
    success: bool,
    session_id: Option<String>,
    message: String,
}

fn execute_issue_once(
    workflow: &WorkflowDefinition,
    config: &RuntimeConfig,
    issue: &TrackerIssue,
) -> Result<IssueExecutionResult, Box<dyn std::error::Error>> {
    let workspace = prepare_workspace(&config.workspace.root, &issue.identifier, &config.hooks)?;
    run_before_run(&workspace.path, &config.hooks)?;

    let prompt = render_prompt(&workflow.prompt_template, issue, None)?;
    std::fs::write(workspace.path.join("JADE_SYMPHONY_PROMPT.md"), &prompt)?;

    let backend = backend_from_config(config);
    let prepared = backend.prepare(workspace.path.clone(), prompt, config)?;
    let events = backend.run(prepared)?;
    let summary = backend.summarize(&events);
    run_after_run(&workspace.path, &config.hooks);

    let log = EventLog::new(config.observability.logs_root.join("jade-symphony.jsonl"));
    for event in &events {
        log.append(&EventRecord {
            event: format!("{event:?}"),
            issue_id: Some(issue.id.clone()),
            issue_identifier: Some(issue.identifier.clone()),
            session_id: summary.session_id.clone(),
            message: summary.message.clone(),
        })?;
    }

    Ok(IssueExecutionResult {
        workspace_path: workspace.path,
        backend: summary.backend,
        success: summary.success,
        session_id: summary.session_id,
        message: summary.message,
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
            println!("run_loop=stopped reason=no_dispatchable_issue iterations={iterations}");
            break;
        };

        let decision = evaluate_issue(&issue);
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

        if !options.write {
            print_run_loop_dry_run_actions(&issue);
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
        let latest_gate = evaluate_issue(&latest);
        if !latest_gate.is_dispatchable() {
            handle_run_loop_gate_failure(adapter.as_ref(), &latest, &latest_gate, &options)?;
            continue;
        }

        if normalize_state(&latest.state) != "in progress" {
            adapter.set_state(&latest.identifier, "in_progress")?;
            println!(
                "run_loop_action=claim issue={} target_state=in_progress",
                latest.identifier
            );
        } else {
            println!("run_loop_action=resume issue={}", latest.identifier);
        }

        let result = execute_issue_once(&workflow, &config, &latest)?;
        let workpad = run_loop_handoff_workpad(&latest, &result);
        adapter.upsert_workpad(&latest.identifier, &workpad)?;

        if result.success {
            if !transition_allowed_for_main_agent("agent_review") {
                return Err("main implementation agent cannot set requested review state".into());
            }
            adapter.set_state(&latest.identifier, "agent_review")?;
            println!(
                "run_loop_action=handoff issue={} target_state=agent_review",
                latest.identifier
            );
        } else {
            adapter.set_state(&latest.identifier, "need_human_input")?;
            println!(
                "run_loop_action=blocked issue={} target_state=need_human_input",
                latest.identifier
            );
        }
    }

    Ok(())
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

fn print_run_loop_dry_run_actions(issue: &TrackerIssue) {
    if normalize_state(&issue.state) != "in progress" {
        println!(
            "run_loop_dry_run action=claim issue={} target_state=in_progress",
            issue.identifier
        );
    } else {
        println!("run_loop_dry_run action=resume issue={}", issue.identifier);
    }
    println!(
        "run_loop_dry_run action=run issue={} backend=configured",
        issue.identifier
    );
    println!(
        "run_loop_dry_run action=workpad issue={} evidence=run_summary",
        issue.identifier
    );
    println!(
        "run_loop_dry_run action=handoff issue={} target_state=agent_review",
        issue.identifier
    );
}

fn run_loop_handoff_workpad(issue: &TrackerIssue, result: &IssueExecutionResult) -> String {
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
        format!("- Success: `{}`", result.success),
        format!(
            "- Session: `{}`",
            result.session_id.as_deref().unwrap_or("n/a")
        ),
        format!("- Message: {}", result.message),
        String::new(),
        "### Main-Agent Boundary".to_string(),
        "- Locally complete main-agent work stops at `Agent Review`.".to_string(),
        "- `Human Review` is reserved for independent Review Agent pass evidence.".to_string(),
    ]
    .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Plan {
        workflow_path: PathBuf,
    },
    Validate {
        workflow_path: PathBuf,
    },
    Inspect {
        workflow_path: PathBuf,
    },
    RunOnce {
        workflow_path: PathBuf,
    },
    RunLoop {
        options: RunLoopOptions,
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
        write: bool,
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
        if args.is_empty() {
            return Ok(Self::Plan {
                workflow_path: PathBuf::from("WORKFLOW.md"),
            });
        }

        match args[0].as_str() {
            "-h" | "--help" | "help" => Ok(Self::Help),
            "plan" | "plan-dispatch" | "dry-run" | "status" => Ok(Self::Plan {
                workflow_path: workflow_arg(&args[1..]),
            }),
            "validate" | "validate-workflow" => Ok(Self::Validate {
                workflow_path: workflow_arg(&args[1..]),
            }),
            "inspect" => Ok(Self::Inspect {
                workflow_path: workflow_arg(&args[1..]),
            }),
            "run-once" => Ok(Self::RunOnce {
                workflow_path: workflow_arg(&args[1..]),
            }),
            "run-loop" => parse_run_loop(&args[1..]),
            "set-state" => parse_set_state(&args[1..]),
            "workpad" => parse_workpad(&args[1..]),
            "create-follow-up" => parse_create_follow_up(&args[1..]),
            "add-to-project" => parse_add_to_project(&args[1..]),
            "review-fake" => parse_review_fake(&args[1..]),
            "review-once" => parse_review_once(&args[1..]),
            "gate" => parse_gate(&args[1..], false),
            "gate-apply" => parse_gate(&args[1..], true),
            "forge-discover" => parse_forge_discover(&args[1..]),
            "forge-discuss" => parse_forge_markdown_command(&args[1..], ForgeCommandKind::Discuss),
            "forge-draft" => parse_forge_draft(&args[1..]),
            "forge-validate" => {
                parse_forge_markdown_command(&args[1..], ForgeCommandKind::Validate)
            }
            "forge-repair" => parse_forge_markdown_command(&args[1..], ForgeCommandKind::Repair),
            "forge-create" => parse_forge_create(&args[1..]),
            command if command.starts_with('-') => Err(usage()),
            workflow_path => Ok(Self::Plan {
                workflow_path: PathBuf::from(workflow_path),
            }),
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

fn parse_gate(args: &[String], apply: bool) -> Result<Command, String> {
    let (args, write) = strip_mode_flags(args);
    if args.len() != 2 {
        return Err(usage());
    }
    Ok(Command::Gate {
        workflow_path: PathBuf::from(&args[0]),
        issue_ref: args[1].clone(),
        apply,
        write,
    })
}

fn parse_run_loop(args: &[String]) -> Result<Command, String> {
    let mut workflow_path = None;
    let mut max_iterations = None;
    let mut once = false;
    let mut write = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--write" => {
                write = true;
                index += 1;
            }
            "--dry-run" => {
                index += 1;
            }
            "--once" => {
                once = true;
                index += 1;
            }
            "--max-iterations" if index + 1 < args.len() => {
                let value = args[index + 1].parse::<usize>().map_err(|_| usage())?;
                if value == 0 {
                    return Err(usage());
                }
                max_iterations = Some(value);
                index += 2;
            }
            value if value.starts_with('-') => return Err(usage()),
            value => {
                if workflow_path.is_some() {
                    return Err(usage());
                }
                workflow_path = Some(PathBuf::from(value));
                index += 1;
            }
        }
    }

    Ok(Command::RunLoop {
        options: RunLoopOptions {
            workflow_path: workflow_path.unwrap_or_else(|| PathBuf::from("WORKFLOW.md")),
            max_iterations,
            once,
            write,
        },
    })
}

fn parse_set_state(args: &[String]) -> Result<Command, String> {
    let (args, write) = strip_mode_flags(args);
    if args.len() != 3 {
        return Err(usage());
    }
    Ok(Command::SetState {
        workflow_path: PathBuf::from(&args[0]),
        issue_ref: args[1].clone(),
        state: args[2].clone(),
        write,
    })
}

fn parse_workpad(args: &[String]) -> Result<Command, String> {
    let (args, write) = strip_mode_flags(args);
    if args.len() != 3 {
        return Err(usage());
    }
    Ok(Command::Workpad {
        workflow_path: PathBuf::from(&args[0]),
        issue_ref: args[1].clone(),
        markdown_path: PathBuf::from(&args[2]),
        write,
    })
}

fn parse_create_follow_up(args: &[String]) -> Result<Command, String> {
    let mut title = None;
    let mut body_path = None;
    let mut workflow_path = None;
    let mut write = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--write" => {
                write = true;
                index += 1;
            }
            "--dry-run" => {
                index += 1;
            }
            "--workflow" if index + 1 < args.len() => {
                workflow_path = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            "--title" if index + 1 < args.len() => {
                title = Some(args[index + 1].clone());
                index += 2;
            }
            "--body-file" if index + 1 < args.len() => {
                body_path = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            _ => return Err(usage()),
        }
    }

    Ok(Command::CreateFollowUp {
        workflow_path: workflow_path.ok_or_else(usage)?,
        title: title.ok_or_else(usage)?,
        body_path: body_path.ok_or_else(usage)?,
        write,
    })
}

fn parse_add_to_project(args: &[String]) -> Result<Command, String> {
    let (args, write) = strip_mode_flags(args);
    if args.len() != 2 {
        return Err(usage());
    }
    Ok(Command::AddToProject {
        workflow_path: PathBuf::from(&args[0]),
        issue_id: args[1].clone(),
        write,
    })
}

fn parse_forge_create(args: &[String]) -> Result<Command, String> {
    let mut workflow_path = None;
    let mut title = None;
    let mut body = None;
    let mut file = None;
    let mut add_to_project = false;
    let mut write = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--write" => {
                write = true;
                index += 1;
            }
            "--dry-run" => {
                index += 1;
            }
            "--add-to-project" => {
                add_to_project = true;
                index += 1;
            }
            "--workflow" if index + 1 < args.len() => {
                workflow_path = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            "--title" if index + 1 < args.len() => {
                title = Some(args[index + 1].clone());
                index += 2;
            }
            "--body" if index + 1 < args.len() => {
                body = Some(args[index + 1].clone());
                index += 2;
            }
            "--file" if index + 1 < args.len() => {
                file = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            _ => return Err(usage()),
        }
    }

    Ok(Command::ForgeCreate {
        workflow_path: workflow_path.ok_or_else(usage)?,
        title: title.ok_or_else(usage)?,
        markdown: read_source_arg(body, file)?,
        add_to_project,
        write,
    })
}

fn parse_review_fake(args: &[String]) -> Result<Command, String> {
    let (args, write) = strip_mode_flags(args);
    if args.len() != 2 && args.len() != 4 {
        return Err(usage());
    }
    let outcome = if args.len() == 4 {
        if args[2] != "--outcome" {
            return Err(usage());
        }
        parse_fake_review_outcome(&args[3])?
    } else {
        FakeReviewOutcome::Pass
    };

    Ok(Command::ReviewFake {
        workflow_path: PathBuf::from(&args[0]),
        issue_ref: args[1].clone(),
        outcome,
        write,
    })
}

fn parse_review_once(args: &[String]) -> Result<Command, String> {
    let (args, write) = strip_mode_flags(args);
    if args.len() != 2 {
        return Err(usage());
    }
    Ok(Command::ReviewOnce {
        workflow_path: PathBuf::from(&args[0]),
        issue_ref: args[1].clone(),
        write,
    })
}

fn parse_fake_review_outcome(value: &str) -> Result<FakeReviewOutcome, String> {
    match value {
        "pass" => Ok(FakeReviewOutcome::Pass),
        "confirmed" => Ok(FakeReviewOutcome::ConfirmedFinding),
        "failed" => Ok(FakeReviewOutcome::Failed),
        _ => Err(usage()),
    }
}

fn strip_mode_flags(args: &[String]) -> (Vec<String>, bool) {
    let mut write = false;
    let filtered = args
        .iter()
        .filter_map(|arg| match arg.as_str() {
            "--write" => {
                write = true;
                None
            }
            "--dry-run" => None,
            _ => Some(arg.clone()),
        })
        .collect();
    (filtered, write)
}

fn workflow_arg(args: &[String]) -> PathBuf {
    let args = args
        .iter()
        .filter(|arg| arg.as_str() != "--dry-run" && arg.as_str() != "--write")
        .cloned()
        .collect::<Vec<_>>();
    args.first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("WORKFLOW.md"))
}

fn parse_forge_draft(args: &[String]) -> Result<Command, String> {
    let mut title = None;
    let mut goal = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--title" if index + 1 < args.len() => {
                title = Some(args[index + 1].clone());
                index += 2;
            }
            "--goal" if index + 1 < args.len() => {
                goal = Some(args[index + 1].clone());
                index += 2;
            }
            _ => return Err(usage()),
        }
    }

    Ok(Command::ForgeDraft {
        title: title.ok_or_else(usage)?,
        goal: goal.ok_or_else(usage)?,
    })
}

#[derive(Debug, Clone, Copy)]
enum ForgeCommandKind {
    Discuss,
    Validate,
    Repair,
}

fn parse_forge_discover(args: &[String]) -> Result<Command, String> {
    let mut intent = None;
    let mut file = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--intent" if index + 1 < args.len() => {
                intent = Some(args[index + 1].clone());
                index += 2;
            }
            "--file" if index + 1 < args.len() => {
                file = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            _ => return Err(usage()),
        }
    }

    Ok(Command::ForgeDiscover {
        source: read_source_arg(intent, file)?,
    })
}

fn parse_forge_markdown_command(
    args: &[String],
    kind: ForgeCommandKind,
) -> Result<Command, String> {
    let mut title = None;
    let mut file = None;
    let mut body = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--title" if index + 1 < args.len() => {
                title = Some(args[index + 1].clone());
                index += 2;
            }
            "--file" if index + 1 < args.len() => {
                file = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            "--body" if index + 1 < args.len() => {
                body = Some(args[index + 1].clone());
                index += 2;
            }
            _ => return Err(usage()),
        }
    }

    let title = title.ok_or_else(usage)?;
    let markdown = read_source_arg(body, file)?;
    Ok(match kind {
        ForgeCommandKind::Discuss => Command::ForgeDiscuss { title, markdown },
        ForgeCommandKind::Validate => Command::ForgeValidate { title, markdown },
        ForgeCommandKind::Repair => Command::ForgeRepair { title, markdown },
    })
}

fn read_source_arg(inline: Option<String>, file: Option<PathBuf>) -> Result<String, String> {
    match (inline, file) {
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display())),
        _ => Err(usage()),
    }
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

fn usage() -> String {
    [
        "Usage:",
        "  jade-symphony plan [path-to-WORKFLOW.md]",
        "  jade-symphony plan-dispatch [path-to-WORKFLOW.md]",
        "  jade-symphony dry-run [path-to-WORKFLOW.md]",
        "  jade-symphony status [path-to-WORKFLOW.md]",
        "  jade-symphony validate [path-to-WORKFLOW.md]",
        "  jade-symphony validate-workflow [path-to-WORKFLOW.md]",
        "  jade-symphony inspect [path-to-WORKFLOW.md]",
        "  jade-symphony run-once [path-to-WORKFLOW.md]",
        "  jade-symphony run-loop [path-to-WORKFLOW.md] [--max-iterations <n> | --once] [--dry-run | --write]",
        "  jade-symphony set-state <path-to-WORKFLOW.md> <issue-ref> <normalized-state> --write",
        "  jade-symphony workpad <path-to-WORKFLOW.md> <issue-ref> <markdown-file> --write",
        "  jade-symphony create-follow-up --workflow <path-to-WORKFLOW.md> --title <title> --body-file <markdown-file> --write",
        "  jade-symphony add-to-project <path-to-WORKFLOW.md> <issue-node-id> --write",
        "  jade-symphony review-once <path-to-WORKFLOW.md> <issue-ref> --write",
        "  jade-symphony review-fake <path-to-WORKFLOW.md> <issue-ref> [--outcome pass|confirmed|failed] --write",
        "  jade-symphony gate <path-to-WORKFLOW.md> <issue-ref>",
        "  jade-symphony gate-apply <path-to-WORKFLOW.md> <issue-ref> --write",
        "  jade-symphony forge-discover --intent <intent> | --file <markdown-file>",
        "  jade-symphony forge-discuss --title <title> (--file <markdown-file> | --body <markdown>)",
        "  jade-symphony forge-draft --title <title> --goal <goal>",
        "  jade-symphony forge-validate --title <title> (--file <markdown-file> | --body <markdown>)",
        "  jade-symphony forge-repair --title <title> (--file <markdown-file> | --body <markdown>)",
        "  jade-symphony forge-create --workflow <path-to-WORKFLOW.md> --title <title> (--file <markdown-file> | --body <markdown>) [--add-to-project] --write",
        "",
        "Compatibility: `jade-symphony <path-to-WORKFLOW.md>` is treated as `plan`.",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ]
        .join("\n")
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
            "--write".into(),
        ])
        .unwrap();

        let Command::ForgeCreate {
            workflow_path,
            title,
            markdown,
            add_to_project,
            write,
        } = command
        else {
            panic!("expected forge-create command");
        };

        assert_eq!(workflow_path, PathBuf::from("examples/dry-run-workflow.md"));
        assert_eq!(title, "Create issue");
        assert!(markdown.contains("## Issue Goal"));
        assert!(add_to_project);
        assert!(write);
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
        assert!(validate_forge_create_contract("Create issue", &forge_contract()).is_ok());

        let error = validate_forge_create_contract("Thin issue", "make it better").unwrap_err();
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
            true,
        )
        .unwrap();
    }
}
