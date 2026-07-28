//! Explicit local Temporal no-op runtime smoke.
//!
//! This test is ignored by default and only runs when
//! `SHEA_TEMPORAL_SMOKE=1`. It owns the Temporal CLI process and Symphony
//! worker process it starts, while refusing to run if the configured service is
//! already reachable. That refusal avoids sharing task queues with an operator
//! process or terminating a resource the harness did not create.

use std::{
    fmt,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use shea_symphony::{
    config::TemporalConfig,
    symphony::{IssueWorkflowInput, IssueWorkflowQueryResult, SymphonyTemporalClient},
    RuntimeConfig, WorkflowStore,
};
use temporalio_client::{
    Client, ClientOptions, Connection, ConnectionOptions, RetryOptions, UntypedWorkflow,
    WorkflowDescribeOptions, WorkflowExecutionInfo, WorkflowHandle,
};
use temporalio_common::protos::temporal::api::enums::v1::WorkflowExecutionStatus;
use temporalio_sdk_core::Url;

const SMOKE_ENABLED_ENV: &str = "SHEA_TEMPORAL_SMOKE";
const WORKFLOW_PROFILE: &str = ".shea/workflows/shea-symphony.md";
const WORKER_QUERY_HOLD_ENV: &str = "SHEA_TEMPORAL_SMOKE_QUERY_HOLD_MS";
const WORKER_QUERY_HOLD_MS: &str = "5000";
const TEMPORAL_DEV_SERVER_COMMAND: &str = "temporal --disable-config-env --disable-config-file server start-dev --headless --ip 127.0.0.1 --port 7233";
const MAX_SYNTHETIC_WORKFLOW_ID_BYTES: usize = 96;
const DIAGNOSTIC_LIMIT_BYTES: usize = 4 * 1024;
const SERVICE_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
const SERVICE_READINESS_ATTEMPTS: usize = 40;
const WORKER_READINESS_ATTEMPTS: usize = 20;
const RETRY_DELAY: Duration = Duration::from_millis(250);
const OPERATION_TIMEOUT: Duration = Duration::from_millis(500);
const RESULT_TIMEOUT: Duration = Duration::from_secs(15);

static SYNTHETIC_WORKFLOW_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
enum SmokeFailure {
    NotEnabled,
    Profile(String),
    ExistingService {
        address: String,
    },
    ServiceProbe {
        address: String,
        detail: String,
    },
    TemporalCliMissing {
        detail: String,
    },
    TemporalCliVersion {
        detail: String,
    },
    ServiceExited {
        status: ExitStatus,
        diagnostic: String,
    },
    ServiceReadinessTimeout {
        detail: String,
        diagnostic: String,
    },
    WorkerSpawn {
        detail: String,
    },
    WorkerExited {
        status: ExitStatus,
        diagnostic: String,
    },
    WorkflowStart {
        workflow_id: String,
        detail: String,
    },
    WorkflowDescribe {
        workflow_id: String,
        detail: String,
    },
    QueryTimeout {
        workflow_id: String,
        detail: String,
    },
    QueryUnexpected {
        detail: String,
    },
    ResultTimeout {
        workflow_id: String,
    },
    ResultFailure {
        workflow_id: String,
        detail: String,
    },
    ResultUnexpected {
        detail: String,
    },
    Cleanup(String),
}

impl fmt::Display for SmokeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnabled => write!(
                formatter,
                "Temporal smoke is opt-in; run `./scripts/temporal-noop-smoke`"
            ),
            Self::Profile(detail) => write!(formatter, "workflow-profile diagnostic: {detail}"),
            Self::ExistingService { address } => write!(
                formatter,
                "existing Temporal service detected at {address}; the smoke refuses to share or stop an operator-owned service. Stop it or choose a clean local environment before retrying"
            ),
            Self::ServiceProbe { address, detail } => write!(
                formatter,
                "service-probe diagnostic for {address}: {detail}"
            ),
            Self::TemporalCliMissing { detail } => write!(
                formatter,
                "dev-server-prerequisite diagnostic: Temporal CLI is required for `{TEMPORAL_DEV_SERVER_COMMAND}`: {detail}"
            ),
            Self::TemporalCliVersion { detail } => write!(
                formatter,
                "dev-server-prerequisite diagnostic: `temporal --version` did not succeed: {detail}"
            ),
            Self::ServiceExited { status, diagnostic } => write!(
                formatter,
                "dev-service-startup diagnostic: the test-owned Temporal service exited with {status}; bounded child output: {diagnostic}"
            ),
            Self::ServiceReadinessTimeout { detail, diagnostic } => write!(
                formatter,
                "service-readiness-timeout diagnostic after {SERVICE_READINESS_ATTEMPTS} attempts: {detail}; bounded child output: {diagnostic}"
            ),
            Self::WorkerSpawn { detail } => {
                write!(formatter, "worker-registration diagnostic: could not start the test-owned worker: {detail}")
            }
            Self::WorkerExited { status, diagnostic } => write!(
                formatter,
                "worker-registration diagnostic: the test-owned worker exited with {status}; bounded child output: {diagnostic}"
            ),
            Self::WorkflowStart {
                workflow_id,
                detail,
            } => write!(
                formatter,
                "workflow-start diagnostic for {workflow_id}: {detail}"
            ),
            Self::WorkflowDescribe {
                workflow_id,
                detail,
            } => write!(
                formatter,
                "workflow-describe diagnostic for {workflow_id}: {detail}"
            ),
            Self::QueryTimeout {
                workflow_id,
                detail,
            } => write!(
                formatter,
                "workflow-query-timeout diagnostic for {workflow_id} after {WORKER_READINESS_ATTEMPTS} attempts: {detail}"
            ),
            Self::QueryUnexpected { detail } => {
                write!(formatter, "workflow-query diagnostic: {detail}")
            }
            Self::ResultTimeout { workflow_id } => write!(
                formatter,
                "terminal-result-timeout diagnostic for {workflow_id} after {} seconds",
                RESULT_TIMEOUT.as_secs()
            ),
            Self::ResultFailure {
                workflow_id,
                detail,
            } => write!(
                formatter,
                "terminal-result-failure diagnostic for {workflow_id}: {detail}"
            ),
            Self::ResultUnexpected { detail } => {
                write!(formatter, "terminal-result diagnostic: {detail}")
            }
            Self::Cleanup(detail) => write!(formatter, "cleanup diagnostic: {detail}"),
        }
    }
}

impl std::error::Error for SmokeFailure {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceOwnershipDecision {
    StartTestOwned,
    RefuseExisting,
}

#[derive(Debug)]
struct ManagedChild {
    name: &'static str,
    child: Child,
    stdout_reader: Option<thread::JoinHandle<Vec<u8>>>,
    stderr_reader: Option<thread::JoinHandle<Vec<u8>>>,
    output_tail: Option<String>,
    cleaned_up: bool,
}

impl ManagedChild {
    fn spawn(name: &'static str, command: &mut Command) -> io::Result<Self> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .expect("stdout was configured as a pipe before spawning the child");
        let stderr = child
            .stderr
            .take()
            .expect("stderr was configured as a pipe before spawning the child");

        Ok(Self {
            name,
            child,
            stdout_reader: Some(thread::spawn(move || read_bounded(stdout))),
            stderr_reader: Some(thread::spawn(move || read_bounded(stderr))),
            output_tail: None,
            cleaned_up: false,
        })
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    fn diagnostic_tail(&mut self) -> String {
        if self.output_tail.is_none() {
            let mut bytes = self
                .stdout_reader
                .take()
                .and_then(|reader| reader.join().ok())
                .unwrap_or_default();
            bytes.extend(
                self.stderr_reader
                    .take()
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default(),
            );
            self.output_tail = Some(diagnostic_tail(&read_bounded(bytes.as_slice())));
        }

        self.output_tail.clone().unwrap_or_default()
    }

    fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned_up {
            return Ok(());
        }

        let status = self
            .child
            .try_wait()
            .map_err(|error| format!("could not inspect {}: {error}", self.name))?;
        if status.is_none() {
            // This target is the exact child created by this harness. The
            // ownership boundary is why an already-running service is refused
            // before any worker is started.
            self.child
                .kill()
                .map_err(|error| format!("could not stop test-owned {}: {error}", self.name))?;
            self.child
                .wait()
                .map_err(|error| format!("could not reap test-owned {}: {error}", self.name))?;
        }

        self.cleaned_up = true;
        let _ = self.diagnostic_tail();
        Ok(())
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn read_bounded(mut reader: impl Read) -> Vec<u8> {
    let mut tail = Vec::new();
    let mut chunk = [0_u8; 1024];

    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => return tail,
            Ok(length) => {
                tail.extend_from_slice(&chunk[..length]);
                if tail.len() > DIAGNOSTIC_LIMIT_BYTES {
                    let excess = tail.len() - DIAGNOSTIC_LIMIT_BYTES;
                    tail.drain(..excess);
                }
            }
        }
    }
}

fn diagnostic_tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "<no child output captured>".to_string()
    } else {
        trimmed.to_string()
    }
}

fn smoke_enabled(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("yes"))
}

fn service_ownership_decision(reachable: bool) -> ServiceOwnershipDecision {
    if reachable {
        ServiceOwnershipDecision::RefuseExisting
    } else {
        ServiceOwnershipDecision::StartTestOwned
    }
}

fn workflow_profile_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(WORKFLOW_PROFILE)
}

fn temporal_config(profile_path: &Path) -> Result<TemporalConfig, SmokeFailure> {
    let workflow_store = WorkflowStore::load(profile_path)
        .map_err(|error| SmokeFailure::Profile(error.to_string()))?;
    let config = RuntimeConfig::from_workflow(workflow_store.active(), profile_path)
        .map_err(|error| SmokeFailure::Profile(error.to_string()))?;

    if config.temporal.address != "localhost:7233" {
        return Err(SmokeFailure::Profile(format!(
            "{WORKFLOW_PROFILE} configures temporal.address={} but the supported smoke service binds localhost:7233",
            config.temporal.address
        )));
    }

    Ok(config.temporal)
}

async fn service_reachable(client: &SymphonyTemporalClient) -> Result<bool, SmokeFailure> {
    match tokio::time::timeout(SERVICE_PROBE_TIMEOUT, client.check_service()).await {
        Ok(Ok(())) => Ok(true),
        Ok(Err(error)) => {
            let unavailable = matches!(error, shea_symphony::symphony::TemporalRuntimeError::Unavailable { .. });
            if unavailable {
                Ok(false)
            } else {
                Err(SmokeFailure::ServiceProbe {
                    address: client.config().address.clone(),
                    detail: error.to_string(),
                })
            }
        }
        Err(_) => Err(SmokeFailure::ServiceProbe {
            address: client.config().address.clone(),
            detail: format!(
                "connection probe exceeded {} ms; refusing to start another service against an uncertain endpoint",
                SERVICE_PROBE_TIMEOUT.as_millis()
            ),
        }),
    }
}

fn temporal_cli_version() -> Result<String, SmokeFailure> {
    let output = Command::new("temporal")
        // Ignore developer CLI profiles so the smoke has one controlled
        // startup configuration rather than inheriting operator shell state.
        .args(["--disable-config-env", "--disable-config-file", "--version"])
        .output()
        .map_err(|error| SmokeFailure::TemporalCliMissing {
            detail: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(SmokeFailure::TemporalCliVersion {
            detail: combined_output(&output),
        });
    }

    Ok(combined_output(&output))
}

fn combined_output(output: &Output) -> String {
    let mut bytes = output.stdout.clone();
    bytes.extend_from_slice(&output.stderr);
    diagnostic_tail(&read_bounded(bytes.as_slice()))
}

async fn start_test_owned_service(
    client: &SymphonyTemporalClient,
) -> Result<ManagedChild, SmokeFailure> {
    match service_ownership_decision(service_reachable(client).await?) {
        ServiceOwnershipDecision::RefuseExisting => {
            return Err(SmokeFailure::ExistingService {
                address: client.config().address.clone(),
            });
        }
        ServiceOwnershipDecision::StartTestOwned => {}
    }

    let version = temporal_cli_version()?;
    println!("temporal smoke: Temporal CLI {version}");

    let mut command = Command::new("temporal");
    command.args([
        "--disable-config-env",
        "--disable-config-file",
        "server",
        "start-dev",
        "--headless",
        "--ip",
        "127.0.0.1",
        "--port",
        "7233",
    ]);
    let mut service =
        ManagedChild::spawn("Temporal dev service", &mut command).map_err(|error| {
            SmokeFailure::TemporalCliMissing {
                detail: error.to_string(),
            }
        })?;

    let mut last_detail = "service has not accepted a connection yet".to_string();
    for _attempt in 1..=SERVICE_READINESS_ATTEMPTS {
        if let Some(status) = service
            .try_wait()
            .map_err(|error| SmokeFailure::ServiceProbe {
                address: client.config().address.clone(),
                detail: error.to_string(),
            })?
        {
            let diagnostic = service.diagnostic_tail();
            return Err(SmokeFailure::ServiceExited { status, diagnostic });
        }

        match tokio::time::timeout(OPERATION_TIMEOUT, client.check_service()).await {
            Ok(Ok(())) => {
                println!(
                    "temporal smoke: test-owned dev service is ready at {}",
                    client.config().address
                );
                return Ok(service);
            }
            Ok(Err(error)) => last_detail = error.to_string(),
            Err(_) => last_detail = "per-attempt connection probe timed out".to_string(),
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }

    let cleanup = service.cleanup();
    let diagnostic = service.diagnostic_tail();
    if let Err(error) = cleanup {
        return Err(SmokeFailure::Cleanup(error));
    }
    Err(SmokeFailure::ServiceReadinessTimeout {
        detail: last_detail,
        diagnostic,
    })
}

fn start_worker(profile_path: &Path) -> Result<ManagedChild, SmokeFailure> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shea-symphony"));
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        // Both the test client and the worker load this exact checked-in profile;
        // a shell's legacy workflow path cannot affect the smoke run.
        .env("SHEA_WORKFLOW_PATH", profile_path)
        // The Activity-only hold creates a deterministic Query observation
        // window for this test-owned worker and synthetic input only.
        .env(WORKER_QUERY_HOLD_ENV, WORKER_QUERY_HOLD_MS);
    ManagedChild::spawn("Symphony Temporal worker", &mut command).map_err(|error| {
        SmokeFailure::WorkerSpawn {
            detail: error.to_string(),
        }
    })
}

fn synthetic_input() -> IssueWorkflowInput {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time must be after the Unix epoch for a test workflow ID")
        .as_millis();
    let sequence = SYNTHETIC_WORKFLOW_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let workflow_id = format!(
        "smoke:shea-symphony:{}:{timestamp}:{sequence}",
        std::process::id()
    );
    assert!(
        workflow_id.len() <= MAX_SYNTHETIC_WORKFLOW_ID_BYTES,
        "synthetic workflow ID must remain bounded"
    );

    IssueWorkflowInput {
        workflow_id,
        repo_id: "synthetic/temporal-smoke".to_string(),
        tracker_backend: "synthetic".to_string(),
        issue_ref: format!("synthetic:temporal-smoke:{timestamp}"),
        from_tracker_state: "SyntheticSmoke".to_string(),
        target_kind: "noop-smoke".to_string(),
        source_kind: "smoke".to_string(),
        source_ref: "test-owned:temporal-noop-smoke".to_string(),
        source_tracker_revision: "synthetic-revision-1".to_string(),
        started_at: format!("unix-millis:{timestamp}"),
        audit_reason: "Exercise the explicit local Temporal smoke.".to_string(),
        operator_action_ref: None,
        capacity_policy_ref: None,
    }
}

async fn describe_execution(
    config: &TemporalConfig,
    workflow_id: &str,
    run_id: Option<&str>,
) -> Result<(String, String, SystemTime, WorkflowExecutionStatus), SmokeFailure> {
    let normalized = if config.address.contains("://") {
        config.address.clone()
    } else {
        format!("http://{}", config.address)
    };
    let address = Url::parse(&normalized).map_err(|error| SmokeFailure::WorkflowDescribe {
        workflow_id: workflow_id.to_string(),
        detail: error.to_string(),
    })?;
    let connection = Connection::connect(
        ConnectionOptions::new(address)
            .retry_options(RetryOptions::no_retries())
            .build(),
    )
    .await
    .map_err(|error| SmokeFailure::WorkflowDescribe {
        workflow_id: workflow_id.to_string(),
        detail: error.to_string(),
    })?;
    let client = Client::new(
        connection,
        ClientOptions::new(config.namespace.as_str()).build(),
    )
    .map_err(|error| SmokeFailure::WorkflowDescribe {
        workflow_id: workflow_id.to_string(),
        detail: error.to_string(),
    })?;
    let handle = WorkflowHandle::<_, UntypedWorkflow>::new(
        client,
        WorkflowExecutionInfo {
            namespace: config.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            run_id: run_id.map(str::to_string),
            first_execution_run_id: None,
        },
    );
    let description = handle
        .describe(WorkflowDescribeOptions::default())
        .await
        .map_err(|error| SmokeFailure::WorkflowDescribe {
            workflow_id: workflow_id.to_string(),
            detail: error.to_string(),
        })?;
    let temporal_started_at =
        description
            .start_time()
            .ok_or_else(|| SmokeFailure::WorkflowDescribe {
                workflow_id: workflow_id.to_string(),
                detail: "Describe omitted Temporal's authoritative start time".to_string(),
            })?;

    Ok((
        description.id().to_string(),
        description.run_id().to_string(),
        temporal_started_at,
        description.status(),
    ))
}

async fn wait_for_query(
    client: &SymphonyTemporalClient,
    workflow_id: &str,
    worker: &mut ManagedChild,
) -> Result<IssueWorkflowQueryResult, SmokeFailure> {
    let mut last_detail = "query has not been attempted yet".to_string();
    for _attempt in 1..=WORKER_READINESS_ATTEMPTS {
        if let Some(status) = worker
            .try_wait()
            .map_err(|error| SmokeFailure::WorkerSpawn {
                detail: error.to_string(),
            })?
        {
            let diagnostic = worker.diagnostic_tail();
            return Err(SmokeFailure::WorkerExited { status, diagnostic });
        }

        match tokio::time::timeout(OPERATION_TIMEOUT, client.query_issue_workflow(workflow_id))
            .await
        {
            Ok(Ok(query)) => return Ok(query),
            Ok(Err(error)) => last_detail = error.to_string(),
            Err(_) => last_detail = "per-attempt query timed out".to_string(),
        }
        tokio::time::sleep(RETRY_DELAY).await;
    }

    Err(SmokeFailure::QueryTimeout {
        workflow_id: workflow_id.to_string(),
        detail: last_detail,
    })
}

async fn exercise_noop_workflow(
    client: &SymphonyTemporalClient,
    worker: &mut ManagedChild,
) -> Result<(), SmokeFailure> {
    let input = synthetic_input();
    let workflow_id = input.workflow_id.clone();
    let retry_input = input.clone();
    let started = client
        .start_noop_issue_workflow(input)
        .await
        .map_err(|error| SmokeFailure::WorkflowStart {
            workflow_id: workflow_id.clone(),
            detail: error.to_string(),
        })?;
    let run_id = started
        .run_id
        .filter(|run_id| !run_id.is_empty())
        .ok_or_else(|| SmokeFailure::WorkflowStart {
            workflow_id: workflow_id.clone(),
            detail: "accepted start did not preserve the SDK Run ID".to_string(),
        })?;
    if started.workflow_id != workflow_id {
        return Err(SmokeFailure::WorkflowStart {
            workflow_id: workflow_id.clone(),
            detail: format!(
                "accepted start returned mismatched Workflow ID {}",
                started.workflow_id
            ),
        });
    }
    println!("temporal smoke: accepted {workflow_id} with real Run ID {run_id}");

    let (described_workflow_id, described_run_id, temporal_started_at, status) =
        describe_execution(client.config(), &workflow_id, Some(&run_id)).await?;
    if described_workflow_id != workflow_id
        || described_run_id != run_id
        || status == WorkflowExecutionStatus::Unspecified
    {
        return Err(SmokeFailure::WorkflowDescribe {
            workflow_id: workflow_id.clone(),
            detail: format!(
                "unexpected immediate evidence: workflow_id={described_workflow_id}, \
                 run_id={described_run_id}, start={temporal_started_at:?}, status={status:?}"
            ),
        });
    }
    println!(
        "temporal smoke: immediate Describe matched {workflow_id}/{run_id} \
         with server start {temporal_started_at:?} and status {status:?}"
    );

    // The exact retry-stable ID must be rejected while the first execution is
    // open; the client must not bind to it or generate a replacement episode.
    if let Ok(unexpected) = client.start_noop_issue_workflow(retry_input.clone()).await {
        return Err(SmokeFailure::WorkflowStart {
            workflow_id: workflow_id.clone(),
            detail: format!("exact open-execution retry unexpectedly accepted: {unexpected:?}"),
        });
    }
    let (_, retry_described_run_id, _, _) =
        describe_execution(client.config(), &workflow_id, None).await?;
    if retry_described_run_id != run_id {
        return Err(SmokeFailure::WorkflowDescribe {
            workflow_id: workflow_id.clone(),
            detail: format!(
                "exact retry changed current Run ID from {run_id} to {retry_described_run_id}"
            ),
        });
    }

    let query = wait_for_query(client, &workflow_id, worker).await?;
    if query.workflow_id != workflow_id
        || !query.issue_ref.starts_with("synthetic:temporal-smoke:")
        || query.terminal_outcome.is_some()
        || query.runtime_health_summary != "initialized"
    {
        return Err(SmokeFailure::QueryUnexpected {
            detail: format!("unexpected read-only state: {query:?}"),
        });
    }
    println!("temporal smoke: observed read-only query for {workflow_id}");

    let result = tokio::time::timeout(
        RESULT_TIMEOUT,
        client.get_issue_workflow_result(&workflow_id),
    )
    .await
    .map_err(|_| SmokeFailure::ResultTimeout {
        workflow_id: workflow_id.clone(),
    })?
    .map_err(|error| SmokeFailure::ResultFailure {
        workflow_id: workflow_id.clone(),
        detail: error.to_string(),
    })?;

    if result.workflow_id != workflow_id
        || result.repo_id != "synthetic/temporal-smoke"
        || !result.issue_ref.starts_with("synthetic:temporal-smoke:")
        || result.active_step != "noop_completed"
        || result.terminal_outcome.as_deref() != Some("completed_noop")
        || !result
            .runtime_health_summary
            .contains("NoopCoreActivity completed without side effects")
        || !result.artifact_refs.is_empty()
    {
        return Err(SmokeFailure::ResultUnexpected {
            detail: format!("unexpected terminal no-op result: {result:?}"),
        });
    }
    println!("temporal smoke: observed completed NoopCoreActivity result for {workflow_id}");

    // RejectDuplicate also forbids reusing the same episode-scoped ID after
    // closure. A later caller may create a separately validated activation;
    // this start boundary never invents one on retry.
    if let Ok(unexpected) = client.start_noop_issue_workflow(retry_input).await {
        return Err(SmokeFailure::WorkflowStart {
            workflow_id,
            detail: format!("exact closed-execution retry unexpectedly accepted: {unexpected:?}"),
        });
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires an explicit local Temporal CLI and SHEA_TEMPORAL_SMOKE=1"]
async fn temporal_noop_smoke() {
    if !smoke_enabled(std::env::var(SMOKE_ENABLED_ENV).ok().as_deref()) {
        panic!("{}", SmokeFailure::NotEnabled);
    }

    let profile_path = workflow_profile_path();
    let temporal_config = temporal_config(&profile_path).unwrap_or_else(|error| panic!("{error}"));
    let client = SymphonyTemporalClient::new(temporal_config);
    let service = start_test_owned_service(&client)
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let worker = start_worker(&profile_path).unwrap_or_else(|error| panic!("{error}"));
    let mut resources = (worker, service);

    let exercise = exercise_noop_workflow(&client, &mut resources.0).await;
    let worker_cleanup = resources.0.cleanup();
    let worker_diagnostic = worker_cleanup
        .as_ref()
        .ok()
        .map(|_| resources.0.diagnostic_tail());
    let service_cleanup = resources.1.cleanup();

    match (exercise, worker_cleanup, service_cleanup) {
        (Ok(()), Ok(()), Ok(())) => {
            println!("temporal smoke: cleaned up test-owned worker and dev service");
        }
        (Err(error), Ok(()), Ok(())) => panic!(
            "{error}; bounded worker output: {}",
            worker_diagnostic.unwrap_or_else(|| "<worker output unavailable>".to_string())
        ),
        (Ok(()), worker, service) => {
            let mut failures = Vec::new();
            if let Err(error) = worker {
                failures.push(error);
            }
            if let Err(error) = service {
                failures.push(error);
            }
            panic!("{}", SmokeFailure::Cleanup(failures.join("; ")));
        }
        (Err(error), worker, service) => {
            let mut detail = error.to_string();
            if let Err(cleanup_error) = worker {
                detail.push_str(&format!("; worker cleanup: {cleanup_error}"));
            }
            if let Err(cleanup_error) = service {
                detail.push_str(&format!("; service cleanup: {cleanup_error}"));
            }
            panic!("{detail}");
        }
    }
}

#[test]
fn smoke_opt_in_values_are_explicit() {
    assert!(smoke_enabled(Some("1")));
    assert!(smoke_enabled(Some("true")));
    assert!(!smoke_enabled(Some("0")));
    assert!(!smoke_enabled(None));
}

#[test]
fn existing_service_is_never_marked_test_owned() {
    assert_eq!(
        service_ownership_decision(true),
        ServiceOwnershipDecision::RefuseExisting
    );
    assert_eq!(
        service_ownership_decision(false),
        ServiceOwnershipDecision::StartTestOwned
    );
}

#[test]
fn synthetic_input_is_unique_bounded_and_non_tracker_backed() {
    let first = synthetic_input();
    let second = synthetic_input();

    assert_ne!(first.workflow_id, second.workflow_id);
    assert!(first.workflow_id.starts_with("smoke:shea-symphony:"));
    assert!(first.workflow_id.len() <= MAX_SYNTHETIC_WORKFLOW_ID_BYTES);
    assert_eq!(first.repo_id, "synthetic/temporal-smoke");
    assert!(first.issue_ref.starts_with("synthetic:temporal-smoke:"));
}

#[test]
fn diagnostic_tail_is_bounded_without_starting_temporal() {
    let output = vec![b'x'; DIAGNOSTIC_LIMIT_BYTES + 20];
    let tail = read_bounded(output.as_slice());

    assert_eq!(tail.len(), DIAGNOSTIC_LIMIT_BYTES);
    assert_eq!(diagnostic_tail(&tail).len(), DIAGNOSTIC_LIMIT_BYTES);
}

#[cfg(unix)]
#[test]
fn cleanup_reaps_a_test_owned_child_without_temporal() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let mut child = ManagedChild::spawn("unit-test child", &mut command).unwrap();

    assert!(child.try_wait().unwrap().is_none());
    child.cleanup().unwrap();
    assert!(child.cleaned_up);
    assert!(child.try_wait().unwrap().is_some());
}
