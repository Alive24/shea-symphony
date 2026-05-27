use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{error::ErrorKind, Args, Parser, Subcommand, ValueEnum};
use shea_symphony::lane_claim::LaneClaimSource;
use shea_symphony::review::{
    FakeReviewOutcome, ReviewFreshnessInput, ReviewReworkClass, ReviewStaleReason,
};
use shea_symphony::review_status::DEFAULT_RECENT_REVIEW_JOBS;
use shea_symphony::skill_status::SkillStatusInput;
use shea_symphony::tracker::ProjectFieldAssignment;

use crate::commands::autopilot::AutopilotLoopOptions;
use crate::commands::doctor::{DoctorAction, DoctorOptions, DoctorRepairIssueOptions};
use crate::commands::forge::{ForgeRelationshipPlan, ForgeReworkOptions, PromotionNoteInput};
use crate::commands::project::ProjectStateOptions;
use crate::commands::session::AgentSessionLaneArg;
use crate::lanes::main_loop::RunLoopOptions;
use crate::lanes::merge::MergeLoopOptions;
use crate::lanes::review::{ReviewLoopOptions, ReviewStatusCliOptions};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Plan {
        workflow_path: PathBuf,
        json: bool,
    },
    AutopilotPlan {
        workflow_path: PathBuf,
        json: bool,
    },
    AutopilotLoop {
        options: AutopilotLoopOptions,
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
        states: Vec<String>,
    },
    ProjectState {
        options: ProjectStateOptions,
    },
    ProjectIssue {
        workflow_path: PathBuf,
        issue_ref: String,
        json: bool,
    },
    ProjectInspect {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: Option<AgentSessionLaneArg>,
    },
    ProjectRelationshipList {
        workflow_path: PathBuf,
        issue_ref: String,
    },
    ProjectRelationshipVerify {
        workflow_path: PathBuf,
        issue_ref: String,
        blocked_by: Vec<String>,
        subissue: Vec<String>,
    },
    ProjectRelationshipAddBlockedBy {
        workflow_path: PathBuf,
        issue_ref: String,
        blocker_ref: String,
        write: bool,
        dry_run: bool,
    },
    ProjectRelationshipAddSubissue {
        workflow_path: PathBuf,
        parent_ref: String,
        subissue_ref: String,
        write: bool,
        dry_run: bool,
    },
    Doctor {
        options: DoctorOptions,
    },
    DoctorRepairHumanReview {
        workflow_path: PathBuf,
        write: bool,
    },
    SkillsStatus {
        input: SkillStatusInput,
        json: bool,
    },
    Profiles {
        workflow_path: PathBuf,
    },
    Debug {
        workflow_path: PathBuf,
    },
    CleanupPlan {
        workflow_path: PathBuf,
    },
    CleanPlan {
        workflow_path: PathBuf,
    },
    CleanAudit {
        workflow_path: PathBuf,
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
    WorkspaceList {
        workflow_path: PathBuf,
    },
    WorkspaceShow {
        workflow_path: PathBuf,
        issue_ref: String,
    },
    WorkspaceAdopt {
        workflow_path: PathBuf,
        issue_ref: String,
        path: PathBuf,
        write: bool,
    },
    WorkspaceEnsure {
        workflow_path: PathBuf,
        issue_ref: String,
        pr_ref: Option<String>,
        branch: Option<String>,
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
    TimelineComment {
        workflow_path: PathBuf,
        issue_ref: String,
        markdown_path: PathBuf,
        write: bool,
    },
    LinkPr {
        workflow_path: PathBuf,
        issue_ref: String,
        pr_ref: String,
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
    ReviewClaim {
        workflow_path: PathBuf,
        issue_ref: String,
        worker: String,
        write: bool,
    },
    LaneClaim {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: AgentSessionLaneArg,
        worker: String,
        source: CliLaneClaimSource,
        write: bool,
    },
    ReviewClearClaim {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    ReviewPass {
        workflow_path: PathBuf,
        issue_ref: String,
        evidence: String,
        write: bool,
    },
    ReviewReject {
        workflow_path: PathBuf,
        issue_ref: String,
        evidence: String,
        target_state: String,
        write: bool,
    },
    ReviewSession {
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
    ReviewStatus {
        options: ReviewStatusCliOptions,
    },
    MergeSession {
        workflow_path: PathBuf,
        issue_ref: String,
        write: bool,
    },
    AgentSessionStart {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: AgentSessionLaneArg,
        run_id: Option<String>,
        write: bool,
    },
    SessionStart {
        workflow_path: PathBuf,
        issue_ref: String,
        lane: AgentSessionLaneArg,
        run_id: String,
        write: bool,
    },
    SessionList {
        workflow_path: PathBuf,
    },
    SessionAttach {
        workflow_path: PathBuf,
        session: String,
        exec: bool,
    },
    AgentSessionList {
        workflow_path: PathBuf,
    },
    AgentSessionAttach {
        workflow_path: PathBuf,
        session: String,
        exec: bool,
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
    ForgeValidate {
        workflow_path: PathBuf,
        status: Option<ForgeStatusArg>,
        title: String,
        markdown: String,
        issue_ref: Option<String>,
    },
    ForgeCreate {
        workflow_path: PathBuf,
        title: String,
        markdown: String,
        status: ForgeStatusArg,
        project: Option<String>,
        project_fields: Vec<ProjectFieldAssignment>,
        assignees: Vec<String>,
        relationships: ForgeRelationshipPlan,
        write: bool,
        dry_run: bool,
    },
    ForgePromote {
        workflow_path: PathBuf,
        issue_ref: String,
        title: String,
        markdown: String,
        promotion_note: PromotionNoteInput,
        relationships: ForgeRelationshipPlan,
        write: bool,
        dry_run: bool,
    },
    ForgeRework {
        options: ForgeReworkOptions,
    },
    Help(String),
}

impl Command {
    pub(crate) fn parse(args: Vec<String>) -> Result<Self, String> {
        if matches!(
            args.first().map(String::as_str),
            Some("help" | "--help" | "-h")
        ) {
            return Ok(Self::Help(usage()));
        }

        let argv = std::iter::once("shea-symphony".to_string())
            .chain(args)
            .collect::<Vec<_>>();
        match Cli::try_parse_from(argv) {
            Ok(cli) => Command::try_from(cli),
            Err(error) if error.kind() == ErrorKind::DisplayHelp => {
                Ok(Self::Help(error.to_string()))
            }
            Err(error) => Err(error.to_string()),
        }
    }
}

fn lane_command(lane: AgentSessionLaneArg, args: LaneCommandArgs) -> Result<Command, String> {
    match args.command {
        MainCommandArgs::Claim(claim) => Ok(Command::LaneClaim {
            workflow_path: claim.workflow_path,
            issue_ref: claim.issue_ref,
            lane,
            worker: claim.worker,
            source: claim.source,
            write: claim.write,
        }),
        MainCommandArgs::Once(args) if lane == AgentSessionLaneArg::Main => Ok(Command::RunOnce {
            workflow_path: args.workflow_path,
        }),
        MainCommandArgs::Loop(args) if lane == AgentSessionLaneArg::Main => run_loop_command(args),
        MainCommandArgs::Once(_) | MainCommandArgs::Loop(_) => {
            Err("only the main lane supports once/loop through this command group".into())
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "shea-symphony",
    about = "OpenAI Symphony-style orchestration harness with Shea Symphony extensions",
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
    #[command(
        next_help_heading = "Human / Operator operations",
        alias = "plan-dispatch",
        alias = "dry-run"
    )]
    Plan(WorkflowPathArgs),
    #[command(alias = "validate-workflow")]
    Validate(WorkflowPathArgs),
    #[command(alias = "audit-project")]
    Doctor(DoctorArgs),
    #[command(name = "doctor-repair-human-review")]
    DoctorRepairHumanReview(DoctorRepairArgs),
    #[command(next_help_heading = "Human / Operator operations")]
    Skills(SkillsArgs),
    Profiles(WorkflowPathArgs),
    Debug(WorkflowPathArgs),
    #[command(
        next_help_heading = "Lane orchestration",
        name = "autopilot",
        about = "Read-only planning and bounded all-lane loop"
    )]
    Autopilot(AutopilotArgs),
    Status(StatusArgs),
    Clean(CleanArgs),
    #[command(
        next_help_heading = "Project / Agent internals",
        about = "Discover and record per-issue git worktrees",
        long_about = "Discover and record per-issue git worktrees.\n\n`workspace` is the safe local-worktree coordination surface for Main, Review, and Merge lanes. It discovers existing issue worktrees from the session registry, Main Workpad/timeline evidence, linked PR/branch hints, and `git worktree list`. It can ensure missing Review/Merge inspection worktrees under the configured workspace root, but it never runs `gh pr checkout`, switches branches, or changes the canonical repository checkout.\n\nUse `workspace show` before local Review or Merge inspection. Use `workspace adopt` only when an operator has selected an existing worktree that should become the canonical workspace evidence for the issue. Use `workspace ensure` only when no suitable candidate exists and local inspection is required."
    )]
    Workspace(WorkspaceArgs),
    #[command(name = "session")]
    Session(SessionArgs),
    Project(ProjectArgs),
    #[command(next_help_heading = "Lane orchestration", name = "main")]
    Main(LaneCommandArgs),
    #[command(name = "merge")]
    Merge(MergeArgs),
    Review(ReviewArgs),
    #[command(name = "create-follow-up")]
    CreateFollowUp(CreateFollowUpArgs),
    #[command(next_help_heading = "Issue Forge")]
    Forge(ForgeArgs),
    #[command(
        next_help_heading = "Reserved lifecycle topology",
        about = "Reserved for future all-lane automatic orchestration"
    )]
    Run,
    #[command(about = "Reserved for future Shea Symphony binary and skill upgrades")]
    Upgrade,
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
struct AutopilotArgs {
    #[command(subcommand)]
    command: AutopilotCommandArgs,
}

#[derive(Debug, Subcommand)]
enum AutopilotCommandArgs {
    #[command(
        about = "Plan Main, Review, and Merge lanes without mutating tracker or runtime state"
    )]
    Plan(AutopilotPlanArgs),
    #[command(
        about = "Run foreground Main, Review, and Merge lane ticks",
        long_about = "`autopilot loop` is a foreground CLI supervisor, not a daemon, background service, or app-server. It composes Main, Review, and Merge lane ticks in order, prints status and parked queues, and runs with --once, --max-iterations, or --continuous. Mutations require --write; dry-run remains the default preview boundary."
    )]
    Loop(AutopilotLoopArgs),
}

#[derive(Debug, Args)]
struct AutopilotPlanArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AutopilotLoopArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(
        long,
        help = "Bounded number of foreground autopilot iterations to run"
    )]
    max_iterations: Option<usize>,
    #[arg(
        long,
        conflicts_with = "max_iterations",
        help = "Run exactly one bounded iteration"
    )]
    once: bool,
    #[arg(
        long,
        conflicts_with_all = ["max_iterations", "once"],
        help = "Run foreground autopilot until cancelled"
    )]
    continuous: bool,
    #[arg(
        long,
        conflicts_with = "dry_run",
        help = "Allow lane ticks to mutate tracker, runtime, worktrees, and PR state"
    )]
    write: bool,
    #[arg(
        long = "dry-run",
        conflicts_with = "write",
        help = "Preview the bounded all-lane tick without mutation"
    )]
    dry_run: bool,
    #[arg(
        long = "no-recover",
        help = "Disable default recover-first handling in write mode"
    )]
    no_recover: bool,
    #[arg(
        long,
        help = "Delay between bounded iterations when the loop waits or retries"
    )]
    poll_interval_ms: Option<u64>,
    #[arg(long, help = "Maximum Main-lane worker slots per iteration")]
    main_max_concurrent: Option<usize>,
    #[arg(long, help = "Maximum Review-lane worker slots per iteration")]
    review_max_concurrent: Option<usize>,
    #[arg(long, help = "Maximum Merge-lane worker slots per iteration")]
    merge_max_concurrent: Option<usize>,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long, help = "Print structured JSON status snapshots")]
    json: bool,
    #[arg(long, help = "Emit newline-delimited JSON work-signal events")]
    event_json: bool,
}

#[derive(Debug, Args)]
struct ProjectStateArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct ProjectIssueArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    json: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long = "state")]
    states: Vec<String>,
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
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    strict: bool,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long)]
    interactive: bool,
    #[arg(long = "auto-fix")]
    auto_fix: bool,
    #[arg(long = "stale-after-ms", default_value_t = 10_800_000)]
    stale_after_ms: u64,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    write: bool,
    #[command(subcommand)]
    action: Option<DoctorSubcommandArgs>,
}

#[derive(Debug, Args)]
struct SkillsArgs {
    #[command(subcommand)]
    command: SkillsCommandArgs,
}

#[derive(Debug, Subcommand)]
enum SkillsCommandArgs {
    #[command(about = "Report per-repo Shea Symphony skill readiness")]
    Status(SkillsStatusArgs),
}

#[derive(Debug, Args)]
struct SkillsStatusArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long = "suite-path")]
    suite_path: Option<PathBuf>,
    #[arg(long = "codex-dir")]
    codex_dir: Option<PathBuf>,
    #[arg(long = "gemini-dir")]
    gemini_dir: Option<PathBuf>,
    #[arg(long = "require-gemini")]
    require_gemini: bool,
    #[arg(long = "session-skills")]
    session_skills: Vec<String>,
    #[arg(long = "session-skills-file")]
    session_skills_file: Option<PathBuf>,
    #[arg(long)]
    json: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[command(subcommand)]
    command: StatusCommandArgs,
}

#[derive(Debug, Subcommand)]
enum StatusCommandArgs {
    #[command(about = "Render the current runtime snapshot")]
    Show(WorkflowPathArgs),
    #[command(about = "Serve the current runtime snapshot once over loopback HTTP")]
    Serve(StatusApiArgs),
}

#[derive(Debug, Subcommand)]
enum DoctorSubcommandArgs {
    Repair(DoctorRepairIssueArgs),
}

#[derive(Debug, Args)]
struct DoctorRepairIssueArgs {
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "move-need-human-input")]
    move_need_human_input: bool,
    #[arg(long = "mark-pr-ready")]
    mark_pr_ready: bool,
    #[arg(long = "confirm-handoff-ready")]
    confirm_handoff_ready: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
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
    #[arg(
        long,
        conflicts_with = "no_recover",
        help = "Enable recover-first handling for interrupted Main runtime sessions (default in --write mode)"
    )]
    recover: bool,
    #[arg(
        long = "no-recover",
        conflicts_with = "recover",
        help = "Disable default recover-first handling in --write mode"
    )]
    no_recover: bool,
    #[arg(long = "max-concurrent")]
    max_concurrent: Option<usize>,
    #[arg(long, value_enum, default_value_t = CliDisplayMode::Plain)]
    display: CliDisplayMode,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    Plain,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliDisplayMode {
    Plain,
    Tui,
}

impl From<CliDisplayMode> for DisplayMode {
    fn from(value: CliDisplayMode) -> Self {
        match value {
            CliDisplayMode::Plain => Self::Plain,
            CliDisplayMode::Tui => Self::Tui,
        }
    }
}

#[derive(Debug, Args)]
struct CleanArgs {
    #[command(subcommand)]
    command: CleanCommand,
}

#[derive(Debug, Subcommand)]
enum CleanCommand {
    Plan(WorkflowPathArgs),
    Audit(WorkflowPathArgs),
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
struct WorkspaceArgs {
    #[command(subcommand)]
    command: WorkspaceCommandArgs,
}

#[derive(Debug, Subcommand)]
enum WorkspaceCommandArgs {
    #[command(
        about = "List discovered issue worktrees and orphan hints",
        long_about = "List discovered issue worktrees and orphan hints.\n\nThis is a read-only Project-wide inventory. It scans tracker issues, session registry records, Main Workpad/timeline evidence, linked PR/branch hints, and local `git worktree list` output. It reports candidates per issue and orphan-looking worktrees whose branch/path implies an issue not currently present in the fetched Project state."
    )]
    List(WorkspaceListArgs),
    #[command(
        about = "Show candidate worktrees for one issue",
        long_about = "Show candidate worktrees for one issue.\n\nThis is the read-only preflight for Review and Merge agents before touching local files. It prints candidate worktrees, their branch/head metadata, evidence sources, warnings, and the canonical candidate when one can be chosen safely. Multiple strong candidates require operator choice through `workspace adopt` before local inspection should rely on a path."
    )]
    Show(WorkspaceShowArgs),
    #[command(
        about = "Record an operator-selected existing worktree",
        long_about = "Record an operator-selected existing worktree as canonical workspace evidence for one issue.\n\n`workspace adopt` validates that the path is an existing git worktree for this repository and that its branch matches the issue/PR evidence. With `--write`, it writes a tracker workpad entry so later Main, Review, and Merge sessions can reuse the same workspace. It does not create a worktree, checkout a PR, switch branches, or mutate files in the selected worktree."
    )]
    Adopt(WorkspaceAdoptArgs),
    #[command(
        about = "Ensure a safe Review/Merge inspection worktree",
        long_about = "Ensure a safe Review/Merge inspection worktree for one issue.\n\n`workspace ensure` first runs the same discovery as `workspace show` and reuses one suitable existing issue worktree when it can be chosen safely. If no suitable worktree exists, it prepares a git worktree only under the workflow-configured workspace root, using the linked PR branch or an explicit `--pr` / `--branch` argument. It never runs `gh pr checkout`, never switches the canonical checkout, refuses ambiguous candidates, and with `--write` records durable Workspace Evidence in the canonical issue workpad."
    )]
    Ensure(WorkspaceEnsureArgs),
}

#[derive(Debug, Args)]
struct WorkspaceListArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
}

#[derive(Debug, Args)]
struct WorkspaceShowArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier to inspect, for example #253")]
    issue_ref: String,
}

#[derive(Debug, Args)]
struct WorkspaceAdoptArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier whose canonical workspace evidence should be updated")]
    issue_ref: String,
    #[arg(help = "Existing local git worktree path selected by the operator")]
    path: PathBuf,
    #[arg(
        long,
        help = "Write workspace adoption evidence to the tracker workpad"
    )]
    write: bool,
    #[arg(
        long = "dry-run",
        help = "Preview adoption validation without writing tracker evidence"
    )]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct WorkspaceEnsureArgs {
    #[arg(
        value_name = "path-to-WORKFLOW.md",
        help = "Workflow config that defines the tracker, artifact roots, and workspace root"
    )]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier whose Review/Merge inspection workspace should be ensured")]
    issue_ref: String,
    #[arg(
        long = "pr",
        help = "Optional PR number, URL, or ref to fetch when tracker-linked PR evidence is missing or ambiguous"
    )]
    pr_ref: Option<String>,
    #[arg(
        long,
        help = "Optional branch/ref to use instead of the linked PR head branch"
    )]
    branch: Option<String>,
    #[arg(
        long,
        help = "Create or reuse the worktree and write Workspace Evidence"
    )]
    write: bool,
    #[arg(
        long = "dry-run",
        help = "Preview the reuse/create plan without creating worktrees or writing tracker evidence"
    )]
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
    #[arg(
        long,
        conflicts_with = "no_recover",
        help = "Enable recover-first handling for interrupted Merge loop claims (default in --write mode)"
    )]
    recover: bool,
    #[arg(
        long = "no-recover",
        conflicts_with = "recover",
        help = "Disable default recover-first handling in --write mode"
    )]
    no_recover: bool,
    #[arg(long = "max-concurrent")]
    max_concurrent: Option<usize>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct LaneCommandArgs {
    #[command(subcommand)]
    command: MainCommandArgs,
}

#[derive(Debug, Subcommand)]
enum MainCommandArgs {
    Claim(LaneClaimArgs),
    Once(WorkflowPathArgs),
    Loop(RunLoopArgs),
}

#[derive(Debug, Args)]
struct LaneClaimArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    worker: String,
    #[arg(long, value_enum, default_value_t = CliLaneClaimSource::Manual)]
    source: CliLaneClaimSource,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum CliLaneClaimSource {
    Manual,
    Loop,
    Goal,
}

impl From<CliLaneClaimSource> for LaneClaimSource {
    fn from(value: CliLaneClaimSource) -> Self {
        match value {
            CliLaneClaimSource::Manual => Self::Manual,
            CliLaneClaimSource::Loop => Self::Loop,
            CliLaneClaimSource::Goal => Self::Goal,
        }
    }
}

#[derive(Debug, Args)]
struct AgentSessionArgs {
    #[command(subcommand)]
    command: AgentSessionCommand,
}

#[derive(Debug, Subcommand)]
enum AgentSessionCommand {
    Start(AgentSessionStartArgs),
    List(AgentSessionListArgs),
    Attach(AgentSessionAttachArgs),
}

#[derive(Debug, Args)]
struct AgentSessionStartArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long, value_enum, default_value = "main")]
    lane: AgentSessionLaneArg,
    #[arg(long = "run")]
    run_id: Option<String>,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct SessionArgs {
    #[command(subcommand)]
    command: SessionCommand,
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ProjectCommandArgs {
    #[command(about = "Read tracker state and Project health")]
    State(ProjectStateArgs),
    #[command(about = "Read one Project issue and linked PR evidence")]
    Issue(ProjectIssueArgs),
    #[command(about = "Inspect live issue readiness without mutating tracker state")]
    Inspect(ProjectInspectArgs),
    #[command(about = "List, add, and verify GitHub issue relationships")]
    Relationship(ProjectRelationshipArgs),
    #[command(name = "set-state", about = "Set one issue Project status")]
    SetState(SetStateArgs),
    #[command(name = "link-pr", about = "Record pull request evidence for one issue")]
    LinkPr(LinkPrArgs),
    #[command(name = "add", about = "Add one GitHub issue to the configured Project")]
    Add(AddToProjectArgs),
    #[command(about = "Upsert the canonical issue workpad")]
    Workpad(WorkpadArgs),
    #[command(
        name = "timeline-comment",
        about = "Append a standalone issue timeline comment"
    )]
    TimelineComment(TimelineCommentArgs),
}

#[derive(Debug, Args)]
struct ProjectRelationshipArgs {
    #[command(subcommand)]
    command: ProjectRelationshipCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ProjectRelationshipCommandArgs {
    #[command(about = "List structured blocked-by and native subissue relationships")]
    List(ProjectRelationshipListArgs),
    #[command(about = "Verify expected structured issue relationships")]
    Verify(ProjectRelationshipVerifyArgs),
    #[command(name = "add-blocked-by", about = "Add a blocked-by relationship")]
    AddBlockedBy(ProjectRelationshipAddBlockedByArgs),
    #[command(
        name = "add-subissue",
        about = "Add a native subissue to a parent issue"
    )]
    AddSubissue(ProjectRelationshipAddSubissueArgs),
}

#[derive(Debug, Args)]
struct ProjectRelationshipListArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct ProjectRelationshipVerifyArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long = "blocked-by")]
    blocked_by: Vec<String>,
    #[arg(long = "subissue")]
    subissue: Vec<String>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct ProjectRelationshipAddBlockedByArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    blocker_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ProjectRelationshipAddSubissueArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    parent_ref: String,
    subissue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ProjectInspectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(help = "Issue identifier to inspect, for example #284")]
    issue_ref: String,
    #[arg(long, value_enum, help = "Optional lane context for readiness output")]
    lane: Option<AgentSessionLaneArg>,
    #[arg(long = "dry-run")]
    _dry_run: bool,
    #[arg(long = "write")]
    _write: bool,
}

#[derive(Debug, Args)]
struct TimelineCommentArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(value_name = "MARKDOWN_PATH")]
    markdown_path: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct MergeArgs {
    #[command(subcommand)]
    command: MergeCommandArgs,
}

#[derive(Debug, Subcommand)]
enum MergeCommandArgs {
    Claim(LaneClaimArgs),
    Once(MergeOnceArgs),
    Loop(MergeLoopArgs),
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Start(SessionStartArgs),
    List(AgentSessionListArgs),
    Attach(AgentSessionAttachArgs),
}

#[derive(Debug, Args)]
struct SessionStartArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long, value_enum)]
    lane: AgentSessionLaneArg,
    #[arg(long = "run")]
    run_id: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct AgentSessionListArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
}

#[derive(Debug, Args)]
struct AgentSessionAttachArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    session: String,
    #[arg(long)]
    exec: bool,
}

#[derive(Debug, Args)]
struct LaneSessionAliasArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
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
struct LinkPrArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    pr_ref: String,
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
struct ReviewClaimArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    worker: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewClearClaimArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewEvidenceArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long = "evidence-file")]
    evidence_file: PathBuf,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    _dry_run: bool,
}

#[derive(Debug, Args)]
struct ReviewRejectArgs {
    #[arg(value_name = "path-to-WORKFLOW.md")]
    workflow_path: PathBuf,
    issue_ref: String,
    #[arg(long = "evidence-file")]
    evidence_file: PathBuf,
    #[arg(long = "target-state", default_value = "agent_review")]
    target_state: String,
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

#[derive(Debug, Args)]
struct ReviewStatusArgs {
    #[arg(value_name = "path-to-WORKFLOW.md", default_value = "WORKFLOW.md")]
    workflow_path: PathBuf,
    #[arg(long = "issue", help = "Filter status to one issue, for example #313")]
    issue_filter: Option<String>,
    #[arg(
        long = "recent",
        default_value_t = DEFAULT_RECENT_REVIEW_JOBS,
        help = "Number of recent completed or failed review jobs to show"
    )]
    recent_limit: usize,
    #[arg(long, help = "Show more paths and anomaly details")]
    verbose: bool,
    #[arg(long, help = "Print the complete structured review status payload")]
    json: bool,
}

#[derive(Debug, Args)]
struct ReviewArgs {
    #[command(subcommand)]
    command: ReviewCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ReviewCommandArgs {
    Fake(ReviewFakeArgs),
    Once(ReviewOnceArgs),
    Claim(LaneClaimArgs),
    Pass(ReviewEvidenceArgs),
    Reject(ReviewRejectArgs),
    Session(LaneSessionAliasArgs),
    Freshness(ReviewFreshnessArgs),
    Loop(ReviewLoopArgs),
    Status(ReviewStatusArgs),
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
struct ForgeMarkdownArgs {
    #[arg(long)]
    body: Option<String>,
    #[arg(long = "body-file", alias = "file")]
    body_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ForgeArgs {
    #[command(subcommand)]
    command: ForgeCommandArgs,
}

#[derive(Debug, Subcommand)]
enum ForgeCommandArgs {
    Create(ForgeCreateArgs),
    Promote(ForgePromoteArgs),
    Rework(ForgeReworkArgs),
    Validate(ForgeValidateArgs),
}

#[derive(Debug, Args)]
struct ForgeCreateArgs {
    #[arg(long, default_value = "workflows/shea-symphony.md")]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[arg(long, value_enum, ignore_case = true, default_value_t = ForgeStatusArg::Todo)]
    status: ForgeStatusArg,
    #[arg(long)]
    project: Option<String>,
    #[arg(long = "project-field")]
    project_fields: Vec<String>,
    #[arg(long = "assignee")]
    assignees: Vec<String>,
    #[arg(long = "blocked-by")]
    blocked_by: Vec<String>,
    #[arg(long = "parent")]
    parent: Option<String>,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ForgePromoteArgs {
    issue_ref: String,
    #[arg(long, default_value = "workflows/shea-symphony.md")]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[command(flatten)]
    promotion_note: PromotionNoteArgs,
    #[arg(long = "blocked-by")]
    blocked_by: Vec<String>,
    #[arg(long = "parent")]
    parent: Option<String>,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct ForgeReworkArgs {
    issue_ref: String,
    #[arg(long, default_value = "workflows/shea-symphony.md")]
    workflow: PathBuf,
    #[arg(long)]
    title: String,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[arg(long = "evidence-file")]
    evidence_file: PathBuf,
    #[arg(long = "operator-confirmation")]
    operator_confirmation: String,
    #[arg(long)]
    write: bool,
    #[arg(long = "dry-run")]
    dry_run: bool,
}

#[derive(Debug, Args)]
struct PromotionNoteArgs {
    #[arg(long = "operator-confirmation")]
    operator_confirmation: String,
    #[arg(long = "decision", required = true)]
    decisions: Vec<String>,
    #[arg(long = "scope-change", required = true)]
    scope_changes: Vec<String>,
    #[arg(long = "dependency-context", required = true)]
    dependencies_context: Vec<String>,
    #[arg(long = "readback-summary")]
    readback_summaries: Vec<String>,
}

#[derive(Debug, Args)]
struct ForgeValidateArgs {
    #[arg(long, default_value = "workflows/shea-symphony.md")]
    workflow: PathBuf,
    #[arg(long, value_enum, ignore_case = true)]
    status: Option<ForgeStatusArg>,
    #[arg(long)]
    title: Option<String>,
    #[command(flatten)]
    markdown: ForgeMarkdownArgs,
    #[arg(long = "issue")]
    issue_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ForgeStatusArg {
    Backlog,
    Todo,
}

impl ForgeStatusArg {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "Backlog",
            Self::Todo => "Todo",
        }
    }

    pub(crate) fn normalized_state(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "todo",
        }
    }
}

fn run_loop_command(args: RunLoopArgs) -> Result<Command, String> {
    if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
        return Err(usage());
    }
    Ok(Command::RunLoop {
        options: RunLoopOptions {
            workflow_path: args.workflow_path,
            max_iterations: args.max_iterations,
            once: args.once,
            write: args.write,
            recover: loop_recover_enabled(args.write, args.recover, args.no_recover),
            max_concurrent: args.max_concurrent,
            display: args.display.into(),
        },
    })
}

fn merge_loop_command(args: MergeLoopArgs) -> Result<Command, String> {
    if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
        return Err(usage());
    }
    Ok(Command::MergeLoop {
        options: MergeLoopOptions {
            workflow_path: args.workflow_path,
            max_iterations: args.max_iterations,
            once: args.once,
            write: args.write,
            recover: loop_recover_enabled(args.write, args.recover, args.no_recover),
            max_concurrent: args.max_concurrent,
        },
    })
}

fn autopilot_loop_command(args: AutopilotLoopArgs) -> Result<Command, String> {
    let display = DisplayMode::from(args.display);
    if args.json && display == DisplayMode::Tui {
        return Err("autopilot loop --json cannot be combined with --display tui".into());
    }
    if args.max_iterations == Some(0)
        || args.poll_interval_ms == Some(0)
        || args.main_max_concurrent == Some(0)
        || args.review_max_concurrent == Some(0)
        || args.merge_max_concurrent == Some(0)
        || (!args.once && !args.continuous && args.max_iterations.is_none())
    {
        return Err(usage());
    }
    Ok(Command::AutopilotLoop {
        options: AutopilotLoopOptions {
            workflow_path: args.workflow_path,
            max_iterations: args.max_iterations,
            once: args.once,
            continuous: args.continuous,
            write: args.write,
            dry_run: args.dry_run,
            recover: loop_recover_enabled(args.write, false, args.no_recover),
            poll_interval_ms: args.poll_interval_ms,
            main_max_concurrent: args.main_max_concurrent,
            review_max_concurrent: args.review_max_concurrent,
            merge_max_concurrent: args.merge_max_concurrent,
            display,
            json: args.json,
            event_json: args.event_json,
        },
    })
}

fn loop_recover_enabled(write: bool, explicit_recover: bool, no_recover: bool) -> bool {
    explicit_recover || (write && !no_recover)
}

fn command_from_project_args(command: ProjectCommandArgs) -> Result<Command, String> {
    match command {
        ProjectCommandArgs::State(args) => Ok(Command::ProjectState {
            options: ProjectStateOptions {
                workflow_path: args.workflow_path,
                display: args.display.into(),
            },
        }),
        ProjectCommandArgs::Issue(args) => Ok(Command::ProjectIssue {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            json: args.json,
        }),
        ProjectCommandArgs::Inspect(args) => Ok(Command::ProjectInspect {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            lane: args.lane,
        }),
        ProjectCommandArgs::Relationship(args) => match args.command {
            ProjectRelationshipCommandArgs::List(args) => Ok(Command::ProjectRelationshipList {
                workflow_path: args.workflow_path,
                issue_ref: args.issue_ref,
            }),
            ProjectRelationshipCommandArgs::Verify(args) => {
                Ok(Command::ProjectRelationshipVerify {
                    workflow_path: args.workflow_path,
                    issue_ref: args.issue_ref,
                    blocked_by: args.blocked_by,
                    subissue: args.subissue,
                })
            }
            ProjectRelationshipCommandArgs::AddBlockedBy(args) => {
                Ok(Command::ProjectRelationshipAddBlockedBy {
                    workflow_path: args.workflow_path,
                    issue_ref: args.issue_ref,
                    blocker_ref: args.blocker_ref,
                    write: args.write,
                    dry_run: args.dry_run,
                })
            }
            ProjectRelationshipCommandArgs::AddSubissue(args) => {
                Ok(Command::ProjectRelationshipAddSubissue {
                    workflow_path: args.workflow_path,
                    parent_ref: args.parent_ref,
                    subissue_ref: args.subissue_ref,
                    write: args.write,
                    dry_run: args.dry_run,
                })
            }
        },
        ProjectCommandArgs::SetState(args) => Ok(Command::SetState {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            state: args.state,
            write: args.write,
        }),
        ProjectCommandArgs::LinkPr(args) => Ok(Command::LinkPr {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            pr_ref: args.pr_ref,
            write: args.write,
        }),
        ProjectCommandArgs::Add(args) => Ok(Command::AddToProject {
            workflow_path: args.workflow_path,
            issue_id: args.issue_id,
            write: args.write,
        }),
        ProjectCommandArgs::Workpad(args) => Ok(Command::Workpad {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            markdown_path: args.markdown_path,
            write: args.write,
        }),
        ProjectCommandArgs::TimelineComment(args) => Ok(Command::TimelineComment {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            markdown_path: args.markdown_path,
            write: args.write,
        }),
    }
}

fn command_from_merge_args(command: MergeCommandArgs) -> Result<Command, String> {
    match command {
        MergeCommandArgs::Claim(claim) => Ok(Command::LaneClaim {
            workflow_path: claim.workflow_path,
            issue_ref: claim.issue_ref,
            lane: AgentSessionLaneArg::Merge,
            worker: claim.worker,
            source: claim.source,
            write: claim.write,
        }),
        MergeCommandArgs::Once(args) => Ok(Command::MergeOnce {
            workflow_path: args.workflow_path,
            write: args.write,
        }),
        MergeCommandArgs::Loop(args) => merge_loop_command(args),
    }
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
                    CliCommand::Validate(args) => Ok(Self::Validate {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Doctor(args) => Ok(Self::Doctor {
                        options: DoctorOptions {
                            workflow_path: args.workflow_path,
                            json: args.json,
                            strict: args.strict,
                            display: args.display.into(),
                            interactive: args.interactive,
                            auto_fix: args.auto_fix,
                            write: args.write,
                            stale_after_ms: args.stale_after_ms,
                            action: args.action.map(|action| match action {
                                DoctorSubcommandArgs::Repair(repair) => {
                                    DoctorAction::Repair(DoctorRepairIssueOptions {
                                        issue_ref: repair.issue_ref,
                                        write: repair.write,
                                        move_need_human_input: repair.move_need_human_input,
                                        mark_pr_ready: repair.mark_pr_ready,
                                        confirm_handoff_ready: repair.confirm_handoff_ready,
                                    })
                                }
                            }),
                        },
                    }),
                    CliCommand::DoctorRepairHumanReview(args) => {
                        Ok(Self::DoctorRepairHumanReview {
                            workflow_path: args.workflow_path,
                            write: args.write,
                        })
                    }
                    CliCommand::Skills(args) => match args.command {
                        SkillsCommandArgs::Status(args) => Ok(Self::SkillsStatus {
                            input: SkillStatusInput {
                                workflow_path: args.workflow_path,
                                suite_path: args.suite_path,
                                codex_dir: args.codex_dir,
                                gemini_dir: args.gemini_dir,
                                require_gemini: args.require_gemini,
                                session_skills: args.session_skills,
                                session_skills_file: args.session_skills_file,
                            },
                            json: args.json,
                        }),
                    },
                    CliCommand::Profiles(args) => Ok(Self::Profiles {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Debug(args) => Ok(Self::Debug {
                        workflow_path: args.workflow_path,
                    }),
                    CliCommand::Autopilot(args) => match args.command {
                        AutopilotCommandArgs::Plan(args) => Ok(Self::AutopilotPlan {
                            workflow_path: args.workflow_path,
                            json: args.json,
                        }),
                        AutopilotCommandArgs::Loop(args) => autopilot_loop_command(args),
                    },
                    CliCommand::Status(args) => match args.command {
                        StatusCommandArgs::Show(show) => Ok(Self::Plan {
                            workflow_path: show.workflow_path,
                            json: show.json,
                        }),
                        StatusCommandArgs::Serve(serve) => Ok(Self::StatusApi {
                            workflow_path: serve.workflow_path,
                            bind: serve.bind,
                            once: serve.once,
                        }),
                    },
                    CliCommand::Clean(args) => match args.command {
                        CleanCommand::Plan(plan) => Ok(Self::CleanPlan {
                            workflow_path: plan.workflow_path,
                        }),
                        CleanCommand::Audit(audit) => Ok(Self::CleanAudit {
                            workflow_path: audit.workflow_path,
                        }),
                    },
                    CliCommand::Workspace(args) => match args.command {
                        WorkspaceCommandArgs::List(list) => Ok(Self::WorkspaceList {
                            workflow_path: list.workflow_path,
                        }),
                        WorkspaceCommandArgs::Show(show) => Ok(Self::WorkspaceShow {
                            workflow_path: show.workflow_path,
                            issue_ref: show.issue_ref,
                        }),
                        WorkspaceCommandArgs::Adopt(adopt) => Ok(Self::WorkspaceAdopt {
                            workflow_path: adopt.workflow_path,
                            issue_ref: adopt.issue_ref,
                            path: adopt.path,
                            write: adopt.write,
                        }),
                        WorkspaceCommandArgs::Ensure(ensure) => Ok(Self::WorkspaceEnsure {
                            workflow_path: ensure.workflow_path,
                            issue_ref: ensure.issue_ref,
                            pr_ref: ensure.pr_ref,
                            branch: ensure.branch,
                            write: ensure.write,
                        }),
                    },
                    CliCommand::Project(args) => command_from_project_args(args.command),
                    CliCommand::Main(args) => lane_command(AgentSessionLaneArg::Main, args),
                    CliCommand::Merge(args) => command_from_merge_args(args.command),
                    CliCommand::Session(args) => match args.command {
                        SessionCommand::Start(start) => Ok(Self::SessionStart {
                            workflow_path: start.workflow_path,
                            issue_ref: start.issue_ref,
                            lane: start.lane,
                            run_id: start.run_id,
                            write: start.write,
                        }),
                        SessionCommand::List(list) => Ok(Self::SessionList {
                            workflow_path: list.workflow_path,
                        }),
                        SessionCommand::Attach(attach) => Ok(Self::SessionAttach {
                            workflow_path: attach.workflow_path,
                            session: attach.session,
                            exec: attach.exec,
                        }),
                    },
                    CliCommand::CreateFollowUp(args) => Ok(Self::CreateFollowUp {
                        workflow_path: args.workflow,
                        title: args.title,
                        body_path: args.body_file,
                        write: args.write,
                    }),
                    CliCommand::Review(args) => command_from_review_args(args.command),
                    CliCommand::Forge(args) => match args.command {
                        ForgeCommandArgs::Create(args) => Ok(Self::ForgeCreate {
                            workflow_path: args.workflow,
                            title: args.title,
                            markdown: read_forge_markdown_arg(args.markdown)?,
                            status: args.status,
                            project: args.project,
                            project_fields: parse_project_field_assignments(args.project_fields)?,
                            assignees: args.assignees,
                            relationships: ForgeRelationshipPlan {
                                blocked_by: args.blocked_by,
                                parent: args.parent,
                            },
                            write: args.write,
                            dry_run: args.dry_run,
                        }),
                        ForgeCommandArgs::Promote(args) => Ok(Self::ForgePromote {
                            workflow_path: args.workflow,
                            issue_ref: args.issue_ref,
                            title: args.title,
                            markdown: read_forge_markdown_arg(args.markdown)?,
                            promotion_note: promotion_note_input(args.promotion_note)?,
                            relationships: ForgeRelationshipPlan {
                                blocked_by: args.blocked_by,
                                parent: args.parent,
                            },
                            write: args.write,
                            dry_run: args.dry_run,
                        }),
                        ForgeCommandArgs::Rework(args) => Ok(Self::ForgeRework {
                            options: ForgeReworkOptions {
                                workflow_path: args.workflow,
                                issue_ref: args.issue_ref,
                                title: args.title,
                                markdown: read_forge_markdown_arg(args.markdown)?,
                                evidence: read_required_file(args.evidence_file)?,
                                operator_confirmation: args.operator_confirmation,
                                write: args.write,
                                dry_run: args.dry_run,
                            },
                        }),
                        ForgeCommandArgs::Validate(args) => {
                            if let Some(issue_ref) = args.issue_ref {
                                Ok(Self::ForgeValidate {
                                    workflow_path: args.workflow,
                                    status: args.status,
                                    title: args.title.unwrap_or_default(),
                                    markdown: read_optional_forge_markdown_arg(args.markdown)?,
                                    issue_ref: Some(issue_ref),
                                })
                            } else {
                                Ok(Self::ForgeValidate {
                                    workflow_path: args.workflow,
                                    status: args.status,
                                    title: args.title.ok_or(
                                        "forge validate requires --title when --issue is not used",
                                    )?,
                                    markdown: read_forge_markdown_arg(args.markdown)?,
                                    issue_ref: None,
                                })
                            }
                        }
                    },
                    CliCommand::Run => {
                        Err("`shea-symphony run` is reserved for future all-lane orchestration and is not implemented yet".into())
                    }
                    CliCommand::Upgrade => {
                        Err("`shea-symphony upgrade` is reserved for future Shea Symphony binary and skill upgrades and is not implemented yet".into())
                    }
                }
            }
        }
    }
}

fn read_forge_markdown_arg(args: ForgeMarkdownArgs) -> Result<String, String> {
    match (args.body, args.body_file) {
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display())),
        _ => Err(usage()),
    }
}

fn read_optional_forge_markdown_arg(args: ForgeMarkdownArgs) -> Result<String, String> {
    match (args.body, args.body_file) {
        (Some(value), None) => Ok(value),
        (None, Some(path)) => std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display())),
        (None, None) => Ok(String::new()),
        (Some(_), Some(_)) => Err(usage()),
    }
}

fn read_required_file(path: PathBuf) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn command_from_review_args(command: ReviewCommandArgs) -> Result<Command, String> {
    match command {
        ReviewCommandArgs::Fake(args) => Ok(Command::ReviewFake {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            outcome: args.outcome.into(),
            write: args.write,
        }),
        ReviewCommandArgs::Once(args) => Ok(Command::ReviewOnce {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            write: args.write,
        }),
        ReviewCommandArgs::Claim(args) => Ok(Command::LaneClaim {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            lane: AgentSessionLaneArg::Review,
            worker: args.worker,
            source: args.source,
            write: args.write,
        }),
        ReviewCommandArgs::Pass(args) => Ok(Command::ReviewPass {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            evidence: read_required_file(args.evidence_file)?,
            write: args.write,
        }),
        ReviewCommandArgs::Reject(args) => Ok(Command::ReviewReject {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            evidence: read_required_file(args.evidence_file)?,
            target_state: args.target_state,
            write: args.write,
        }),
        ReviewCommandArgs::Session(args) => Ok(Command::ReviewSession {
            workflow_path: args.workflow_path,
            issue_ref: args.issue_ref,
            write: args.write,
        }),
        ReviewCommandArgs::Freshness(args) => Ok(Command::ReviewFreshness {
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
        ReviewCommandArgs::Loop(args) => {
            if args.max_iterations == Some(0) || args.max_concurrent == Some(0) {
                return Err(usage());
            }
            Ok(Command::ReviewLoop {
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
        ReviewCommandArgs::Status(args) => {
            if args.recent_limit == 0 {
                return Err(usage());
            }
            Ok(Command::ReviewStatus {
                options: ReviewStatusCliOptions {
                    workflow_path: args.workflow_path,
                    issue_filter: args.issue_filter,
                    recent_limit: args.recent_limit,
                    verbose: args.verbose,
                    json: args.json,
                },
            })
        }
    }
}

fn parse_project_field_assignments(
    values: Vec<String>,
) -> Result<Vec<ProjectFieldAssignment>, String> {
    values
        .into_iter()
        .map(|value| ProjectFieldAssignment::parse(&value).map_err(|error| error.to_string()))
        .collect()
}

fn promotion_note_input(args: PromotionNoteArgs) -> Result<PromotionNoteInput, String> {
    fn clean_nonempty(value: String, field: &str) -> Result<String, String> {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            Err(format!("forge promote requires non-empty {field}"))
        } else {
            Ok(trimmed)
        }
    }

    fn clean_many(values: Vec<String>, field: &str) -> Result<Vec<String>, String> {
        let cleaned = values
            .into_iter()
            .map(|value| clean_nonempty(value, field))
            .collect::<Result<Vec<_>, _>>()?;
        if cleaned.is_empty() {
            Err(format!("forge promote requires at least one {field}"))
        } else {
            Ok(cleaned)
        }
    }

    Ok(PromotionNoteInput {
        operator_confirmation: clean_nonempty(
            args.operator_confirmation,
            "--operator-confirmation",
        )?,
        decisions: clean_many(args.decisions, "--decision")?,
        scope_changes: clean_many(args.scope_changes, "--scope-change")?,
        dependencies_context: clean_many(args.dependencies_context, "--dependency-context")?,
        readback_summaries: args
            .readback_summaries
            .into_iter()
            .map(|value| clean_nonempty(value, "--readback-summary"))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn usage() -> String {
    [
        "OpenAI Symphony-style orchestration harness with Shea Symphony extensions",
        "",
        "Usage: shea-symphony [path-to-WORKFLOW.md] [COMMAND]",
        "",
        "Human / Operator operations:",
        "  plan                        Render the dispatch/status plan",
        "  validate                    Validate workflow loading and configuration",
        "  doctor                      Audit Project, workflow, and runtime invariants",
        "  skills                      Inspect per-repo skill readiness",
        "  status                      Show or serve runtime status snapshots",
        "  clean                       Plan or audit artifact cleanup",
        "  profiles                    List execution profiles",
        "  debug                       Render a combined operator debug report",
        "",
        "Project / Agent internals:",
        "  project                     Read or mutate Project facts through grouped subcommands",
        "  workspace                   Discover and record per-issue git worktrees",
        "  session                     Start, list, or attach supervised lane sessions",
        "",
        "Lane orchestration:",
        "  autopilot                   Read-only planning and bounded all-lane loop",
        "  main                        Main Agent claim, once, and loop commands",
        "  review                      Review Agent claim, pass/reject, session, freshness, and loop commands",
        "  merge                       Merging Agent claim, once, and loop commands",
        "  create-follow-up            Create an operator follow-up issue",
        "",
        "Issue Forge:",
        "  forge                       Validate, create, or promote issue contracts",
        "",
        "Reserved lifecycle topology:",
        "  run                         Reserved for future all-lane automatic orchestration",
        "  upgrade                     Reserved for future Shea Symphony binary and skill upgrades",
        "",
        "Arguments:",
        "  [path-to-WORKFLOW.md]",
        "",
        "Options:",
        "  -h, --help                  Print help",
        "",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Command {
        Command::parse(args.iter().map(|arg| arg.to_string()).collect()).unwrap()
    }

    #[test]
    fn parser_keeps_write_mode_main_loop_recover_default() {
        let command = parse(&[
            "main",
            "loop",
            "workflows/shea-symphony.md",
            "--write",
            "--max-concurrent",
            "2",
        ]);

        let Command::RunLoop { options } = command else {
            panic!("expected main loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("workflows/shea-symphony.md")
        );
        assert!(options.write);
        assert!(options.recover);
        assert_eq!(options.max_concurrent, Some(2));
    }

    #[test]
    fn parser_keeps_review_status_recent_limit_guard() {
        let error = Command::parse(vec![
            "review".into(),
            "status".into(),
            "workflows/shea-symphony.md".into(),
            "--recent-limit".into(),
            "0".into(),
        ])
        .unwrap_err();

        assert!(error.contains("Usage:"));
    }

    #[test]
    fn parser_accepts_autopilot_loop_tui_display() {
        let command = parse(&[
            "autopilot",
            "loop",
            "workflows/shea-symphony.md",
            "--once",
            "--dry-run",
            "--display",
            "tui",
        ]);

        let Command::AutopilotLoop { options } = command else {
            panic!("expected autopilot loop command");
        };

        assert_eq!(
            options.workflow_path,
            PathBuf::from("workflows/shea-symphony.md")
        );
        assert!(options.once);
        assert_eq!(options.display, DisplayMode::Tui);
    }

    #[test]
    fn parser_accepts_autopilot_loop_event_json_signal() {
        let command = parse(&[
            "autopilot",
            "loop",
            "workflows/shea-symphony.md",
            "--once",
            "--dry-run",
            "--event-json",
        ]);

        let Command::AutopilotLoop { options } = command else {
            panic!("expected autopilot loop command");
        };

        assert!(options.event_json);
    }

    #[test]
    fn parser_rejects_autopilot_loop_json_tui_display() {
        let error = Command::parse(vec![
            "autopilot".into(),
            "loop".into(),
            "workflows/shea-symphony.md".into(),
            "--once".into(),
            "--json".into(),
            "--display".into(),
            "tui".into(),
        ])
        .unwrap_err();

        assert!(error.contains("cannot be combined"));
    }
}
