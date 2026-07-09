use futures_util::try_join;
use temporalio_client::Client;
use temporalio_common::telemetry::TelemetryOptions;
use temporalio_sdk::Worker;
use temporalio_sdk_core::{CoreRuntime, RuntimeOptions};

use crate::config::RuntimeConfig;
use crate::symphony::client::{SymphonyTemporalClient, TemporalRuntimeError};
use crate::symphony::workers::{agent_worker_options, core_worker_options, local_worker_options};

pub async fn run_symphony_workers(config: RuntimeConfig) -> Result<(), TemporalRuntimeError> {
    let runtime = core_runtime()?;
    let temporal_client = SymphonyTemporalClient::new(config.temporal.clone());
    let client = temporal_client.connect().await?;

    // One process hosts the starting queue topology, but capacity is still
    // isolated at Temporal task-queue and Activity-concurrency boundaries.
    let mut core_worker = worker(
        &runtime,
        client.clone(),
        &config.temporal.task_queues.core,
        core_worker_options(&config.temporal)?,
    )?;
    let mut agent_worker = worker(
        &runtime,
        client.clone(),
        &config.temporal.task_queues.agent,
        agent_worker_options(&config.temporal),
    )?;
    let mut local_worker = worker(
        &runtime,
        client,
        &config.temporal.task_queues.local,
        local_worker_options(&config.temporal),
    )?;

    try_join!(core_worker.run(), agent_worker.run(), local_worker.run())
        .map_err(|error| TemporalRuntimeError::WorkerRuntime(error.to_string()))?;

    Ok(())
}

fn core_runtime() -> Result<CoreRuntime, TemporalRuntimeError> {
    // Keep runtime construction close to the SDK quickstart so API drift is
    // obvious when the Public Preview SDK changes.
    let telemetry_options = TelemetryOptions::builder().build();
    let runtime_options = RuntimeOptions::builder()
        .telemetry_options(telemetry_options)
        .build()
        .map_err(|error| TemporalRuntimeError::RuntimeInitialization(error.to_string()))?;

    CoreRuntime::new_assume_tokio(runtime_options)
        .map_err(|error| TemporalRuntimeError::RuntimeInitialization(error.to_string()))
}

fn worker(
    runtime: &CoreRuntime,
    client: Client,
    task_queue: &str,
    options: temporalio_sdk::WorkerOptions,
) -> Result<Worker, TemporalRuntimeError> {
    Worker::new(runtime, client, options).map_err(|error| {
        TemporalRuntimeError::WorkerRegistration {
            task_queue: task_queue.to_string(),
            source_error: error.to_string(),
        }
    })
}
