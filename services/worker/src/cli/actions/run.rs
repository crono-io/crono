//! Bounded `JetStream` pull-consumer and server-mediated Attempt coordinator.
//!
//! Every worker session reports bounded presence metadata to the server, and
//! every delivery is claimed before execution. Long-running work refreshes both
//! the PostgreSQL lease and `JetStream` acknowledgement deadline. Completion is
//! persisted before the message is acknowledged, so a crash may redeliver but
//! cannot create another logical Attempt.
//!
//! # Flow Overview
//!
//! Decode a Queue dispatch, claim its immutable snapshot, emit a sanitized
//! per-Attempt execution timeline while the runner drains both process pipes,
//! then report completion before acknowledging the delivery. Malformed or
//! misrouted dispatches are terminated, while transient control failures are
//! returned for redelivery. The worker never accesses PostgreSQL directly.
//!
//! Dry-run sessions consume and complete claimed Attempts successfully while
//! printing their already-rendered command instead of starting a process.
//! The printed argv is sanitized using known sensitive input keys and argument
//! switches; unknown literal credentials remain outside this fallback's scope.

use crate::execution::{
    ConsoleSink, EventSink, ExecutionEvent, ExecutionPhase, ExecutionTimeline, FanoutSink,
    LiveOutput, LogFormat, Redactor,
    runner::{ExecutionResult, elapsed_ms, execute_snapshot},
};
use anyhow::{Context, Result, bail};
use async_nats::jetstream::{
    self,
    consumer::{AckPolicy, pull},
    message::AckKind,
};
use crono_api::{
    ClaimRequest, ClaimResponse, CompletionRequest, DispatchEnvelope, ExecutionSnapshot,
    LeaseRequest, OutputSnapshotRequest, QueueReference, QueueResolutionRequest,
    QueueResolutionResponse, QueueResolutionStatus, WorkerDiagnostics, WorkerHeartbeatRequest,
};
use futures_util::StreamExt;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const STREAM_NAME: &str = "CRONO_DISPATCH";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const QUEUE_RESOLUTION_ATTEMPTS: u8 = 30;
const QUEUE_RESOLUTION_RETRY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub nats_url: String,
    pub queue: String,
    pub worker_id: String,
    pub concurrency: u16,
    pub dry_run: bool,
    pub log_format: LogFormat,
}

/// Run a durable pull consumer until an operating-system shutdown signal.
///
/// # Errors
///
/// Returns when NATS or `JetStream` setup fails. Individual delivery failures
/// are retained for redelivery and do not stop the worker.
#[tracing::instrument(name = "worker.run", skip_all, fields(worker_id = args.worker_id, queue = args.queue))]
pub async fn execute(args: Args) -> Result<()> {
    validate_token(&args.queue, "queue")?;
    validate_token(&args.worker_id, "worker ID")?;
    let client = async_nats::ConnectOptions::new()
        .max_reconnects(None)
        .connect(&args.nats_url)
        .await
        .context("failed to connect worker to NATS")?;
    let queue = resolve_queue(&client, &args.queue).await?;
    let context = jetstream::new(client.clone());
    let stream = context
        .get_stream(STREAM_NAME)
        .await
        .context("Crono execution stream is unavailable")?;
    let durable = format!("crono-{}", queue.id);
    let consumer = stream
        .get_or_create_consumer(
            &durable,
            pull::Config {
                durable_name: Some(durable.clone()),
                description: Some(format!("Crono workers for Queue {}", queue.name)),
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_secs(90),
                max_ack_pending: i64::from(args.concurrency),
                filter_subject: format!("crono.dispatch.{}", queue.id),
                ..pull::Config::default()
            },
        )
        .await
        .context("failed to ensure durable worker consumer")?;
    let cancellation = CancellationToken::new();
    let signal = cancellation.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });
    let args = Arc::new(args);
    let event_sink: Arc<dyn EventSink> = Arc::new(ConsoleSink::new(args.log_format));
    let heartbeat = tokio::spawn(run_presence_heartbeat(
        client.clone(),
        Arc::clone(&args),
        queue.id,
        uuid::Uuid::now_v7(),
        cancellation.child_token(),
    ));
    info!(
        worker_id = args.worker_id,
        queue = args.queue,
        concurrency = args.concurrency,
        dry_run = args.dry_run,
        "Crono worker started"
    );
    while !cancellation.is_cancelled() {
        let messages = consumer
            .fetch()
            .max_messages(usize::from(args.concurrency))
            .max_bytes(usize::from(args.concurrency) * 65_536)
            .messages()
            .await;
        let mut messages = match messages {
            Ok(messages) => messages,
            Err(error) => {
                warn!(%error, "worker fetch failed");
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mut batch = Vec::with_capacity(usize::from(args.concurrency));
        while let Some(message) = messages.next().await {
            match message {
                Ok(message) => batch.push(message),
                Err(error) => warn!(%error, "worker delivery failed"),
            }
        }
        if batch.is_empty() {
            time::sleep(Duration::from_millis(100)).await;
            continue;
        }
        futures_util::stream::iter(batch)
            .for_each_concurrent(usize::from(args.concurrency), |message| {
                handle_message(
                    client.clone(),
                    Arc::clone(&args),
                    Arc::clone(&event_sink),
                    queue.id,
                    message,
                )
            })
            .await;
    }
    cancellation.cancel();
    heartbeat.await.context("worker heartbeat task failed")?;
    info!("Crono worker stopped");
    Ok(())
}

/// Refresh worker presence independently of execution traffic.
///
/// A heartbeat failure is recoverable because both NATS and the server control
/// subscriber may reconnect. Cancellation interrupts an in-flight request so a
/// missing responder cannot delay graceful shutdown.
async fn run_presence_heartbeat(
    client: async_nats::Client,
    args: Arc<Args>,
    queue_id: uuid::Uuid,
    session_id: uuid::Uuid,
    cancellation: CancellationToken,
) {
    let request = WorkerHeartbeatRequest {
        worker_id: args.worker_id.clone(),
        session_id,
        queue_id,
        concurrency: args.concurrency,
        version: env!("CARGO_PKG_VERSION").to_string(),
        diagnostics: Some(worker_diagnostics(args.dry_run)),
    };
    let mut interval = time::interval(HEARTBEAT_INTERVAL);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return,
            _ = interval.tick() => {}
        }
        let response = tokio::select! {
            () = cancellation.cancelled() => return,
            result = presence_request(
                &client,
                &args.worker_id,
                &request,
            ) => result,
        };
        match response {
            Ok(true) => {}
            Ok(false) => warn!(worker_id = args.worker_id, "worker heartbeat was rejected"),
            Err(error) => warn!(%error, worker_id = args.worker_id, "worker heartbeat failed"),
        }
    }
}

/// Report only the child environment values the executor intentionally copies.
fn worker_diagnostics(dry_run: bool) -> WorkerDiagnostics {
    fn safe_variable(name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .filter(|value| value.len() <= 128 && !value.chars().any(char::is_control))
    }
    WorkerDiagnostics {
        hostname: whoami::hostname().unwrap_or_else(|_| "unknown".to_string()),
        os: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        default_shell_path: "/bin/sh".to_string(),
        default_shell_present: std::path::Path::new("/bin/sh").is_file(),
        dry_run,
        lang: safe_variable("LANG"),
        lc_all: safe_variable("LC_ALL"),
        tz: safe_variable("TZ"),
    }
}

async fn resolve_queue(client: &async_nats::Client, name: &str) -> Result<QueueReference> {
    let subject = format!("crono.worker.queue.resolve.{name}");
    let request = QueueResolutionRequest {
        name: name.to_string(),
    };
    for attempt in 1..=QUEUE_RESOLUTION_ATTEMPTS {
        let message = match client
            .request(subject.clone(), serde_json::to_vec(&request)?.into())
            .await
        {
            Ok(message) => message,
            Err(error) if attempt < QUEUE_RESOLUTION_ATTEMPTS => {
                warn!(%error, attempt, queue = name, "Queue resolution responder is not ready");
                time::sleep(QUEUE_RESOLUTION_RETRY).await;
                continue;
            }
            Err(error) => {
                return Err(error).context("Crono server did not answer Queue resolution");
            }
        };
        let response: QueueResolutionResponse = serde_json::from_slice(&message.payload)
            .context("Crono server returned an invalid Queue resolution")?;
        match response.status {
            QueueResolutionStatus::Ready => {
                return response
                    .queue
                    .context("Crono server omitted resolved Queue identity");
            }
            QueueResolutionStatus::NotFound => {
                bail!("Queue {name:?} does not exist; create it before starting this worker");
            }
            QueueResolutionStatus::Unavailable if attempt < QUEUE_RESOLUTION_ATTEMPTS => {
                warn!(
                    attempt,
                    queue = name,
                    "Queue resolution is temporarily unavailable"
                );
                time::sleep(QUEUE_RESOLUTION_RETRY).await;
            }
            QueueResolutionStatus::Unavailable => {
                bail!("Crono server could not resolve Queue {name:?}");
            }
        }
    }
    bail!("Crono server could not resolve Queue {name:?}")
}

async fn presence_request(
    client: &async_nats::Client,
    worker_id: &str,
    request: &WorkerHeartbeatRequest,
) -> Result<bool> {
    let subject = format!("crono.worker.presence.{worker_id}");
    let response = client
        .request(subject, serde_json::to_vec(request)?.into())
        .await
        .context("worker presence request failed")?;
    serde_json::from_slice(&response.payload).context("worker presence response is invalid")
}

async fn handle_message(
    client: async_nats::Client,
    args: Arc<Args>,
    event_sink: Arc<dyn EventSink>,
    queue_id: uuid::Uuid,
    message: jetstream::Message,
) {
    let envelope: DispatchEnvelope = match serde_json::from_slice(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            warn!(%error, "terminating malformed execution message");
            let _ = message.ack_with(AckKind::Term).await;
            return;
        }
    };
    if envelope.queue_id != queue_id {
        warn!(
            expected_queue_id = %queue_id,
            actual_queue_id = %envelope.queue_id,
            "terminating dispatch delivered to the wrong Queue"
        );
        let _ = message.ack_with(AckKind::Term).await;
        return;
    }
    let claim = ClaimRequest {
        run_id: envelope.run_id,
        attempt_id: envelope.attempt_id,
        queue_id,
        worker_id: args.worker_id.clone(),
    };
    let response: ClaimResponse =
        match control_request(&client, "claim", &args.worker_id, &claim).await {
            Ok(response) => response,
            Err(error) => {
                warn!(%error, run_id = %envelope.run_id, "claim failed; requesting redelivery");
                let _ = message
                    .ack_with(AckKind::Nak(Some(Duration::from_secs(5))))
                    .await;
                return;
            }
        };
    let Some(execution) = response.execution.filter(|_| response.claimed) else {
        let _ = message.double_ack().await;
        return;
    };
    execute_claimed(&client, &args, &message, &envelope, execution, event_sink).await;
}

/// Observe and report an already-claimed Attempt, preserving the server-first
/// completion and `JetStream` acknowledgement ordering.
async fn execute_claimed(
    client: &async_nats::Client,
    args: &Args,
    message: &jetstream::Message,
    envelope: &DispatchEnvelope,
    execution: ExecutionSnapshot,
    event_sink: Arc<dyn EventSink>,
) {
    let live = Arc::new(LiveOutput::default());
    let combined: Arc<dyn EventSink> = Arc::new(FanoutSink {
        console: event_sink,
        live: Arc::clone(&live),
    });
    let timeline = Arc::new(ExecutionTimeline::new(
        envelope.run_id,
        envelope.attempt_id,
        execution.job_id,
        args.worker_id.clone(),
        execution.queue.clone(),
        combined,
    ));
    let upload_cancel = CancellationToken::new();
    let uploader = tokio::spawn(upload_live_output(
        client.clone(),
        Arc::clone(&live),
        envelope.attempt_id,
        args.worker_id.clone(),
        upload_cancel.clone(),
    ));
    let started = Instant::now();
    timeline.emit(ExecutionEvent::RunReceived {
        dispatch_id: envelope.dispatch_id,
        trigger: execution.trigger,
        scheduled_at: execution.scheduled_at,
    });
    timeline.emit(ExecutionEvent::RunStarted {
        dry_run: args.dry_run || execution.dry_run,
    });
    let redactor = Arc::new(Redactor::from_inputs_and_arguments(
        &execution.inputs,
        &execution.arguments,
    ));
    let result = run_with_heartbeat(
        client,
        args,
        envelope,
        execution,
        Arc::clone(&timeline),
        Arc::clone(&redactor),
        message,
    )
    .await;
    upload_cancel.cancel();
    if let Err(error) = uploader.await {
        warn!(%error, "live output task failed");
    }
    let result = match result {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            succeeded: false,
            skipped: false,
            exit_code: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(error.to_string()),
            failure_phase: Some(ExecutionPhase::Execution),
        },
    };
    let completion = CompletionRequest {
        attempt_id: envelope.attempt_id,
        worker_id: args.worker_id.clone(),
        succeeded: result.succeeded,
        skipped: result.skipped,
        exit_code: result.exit_code,
        stdout_tail: result.stdout_tail.clone(),
        stderr_tail: result.stderr_tail.clone(),
        error: result.error.as_deref().map(|error| redactor.text(error)),
    };
    match control_request::<_, bool>(client, "complete", &args.worker_id, &completion).await {
        Ok(true) => {
            emit_result(&timeline, &redactor, started, &result);
            let _ = message.double_ack().await;
        }
        Ok(false) => {
            timeline.emit(ExecutionEvent::ResultReportingFailed {
                error: "completion lost its worker lease".to_string(),
                execution_succeeded: result.succeeded,
                execution_skipped: result.skipped,
                total_duration_ms: elapsed_ms(started),
            });
            warn!(attempt_id = %envelope.attempt_id, "completion lost its worker lease");
            let _ = message.ack_with(AckKind::Term).await;
        }
        Err(error) => {
            timeline.emit(ExecutionEvent::ResultReportingFailed {
                error: "completion was not confirmed".to_string(),
                execution_succeeded: result.succeeded,
                execution_skipped: result.skipped,
                total_duration_ms: elapsed_ms(started),
            });
            warn!(%error, attempt_id = %envelope.attempt_id, "completion was not confirmed");
            let _ = message
                .ack_with(AckKind::Nak(Some(Duration::from_secs(5))))
                .await;
        }
    }
}

/// Send only changed, bounded tails; network stalls cannot block pipe readers.
async fn upload_live_output(
    client: async_nats::Client,
    live: Arc<LiveOutput>,
    attempt_id: uuid::Uuid,
    worker_id: String,
    cancellation: CancellationToken,
) {
    let mut interval = time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    let mut acknowledged_revision = 0;
    let mut sequence = 0_u64;
    loop {
        tokio::select! {
            () = cancellation.cancelled() => return,
            _ = interval.tick() => {}
        }
        let (revision, stdout_tail, stderr_tail) = live.snapshot();
        if revision == acknowledged_revision {
            continue;
        }
        sequence = sequence.saturating_add(1);
        let request = OutputSnapshotRequest {
            attempt_id,
            worker_id: worker_id.clone(),
            sequence,
            stdout_tail,
            stderr_tail,
        };
        let subject = format!("crono.worker.output.{worker_id}");
        let sent = tokio::select! {
            () = cancellation.cancelled() => return,
            result = time::timeout(Duration::from_secs(2), client.request(subject, match serde_json::to_vec(&request) {
                Ok(payload) => payload.into(),
                Err(error) => {
                    warn!(%error, "failed to encode live output");
                    continue;
                }
            })) => result,
        };
        match sent {
            Ok(Ok(response))
                if matches!(serde_json::from_slice::<bool>(&response.payload), Ok(true)) =>
            {
                acknowledged_revision = revision;
            }
            Ok(Ok(_)) => warn!(attempt_id = %attempt_id, "live output update was rejected"),
            Ok(Err(error)) => warn!(%error, attempt_id = %attempt_id, "live output request failed"),
            Err(_) => warn!(attempt_id = %attempt_id, "live output request timed out"),
        }
    }
}

/// Record the terminal worker outcome only after the server confirms completion.
fn emit_result(
    timeline: &ExecutionTimeline,
    redactor: &Redactor,
    started: Instant,
    result: &ExecutionResult,
) {
    if result.skipped {
        timeline.emit(ExecutionEvent::RunSkipped {
            reason: "dry run".to_string(),
            total_duration_ms: elapsed_ms(started),
        });
    } else if result.succeeded {
        timeline.emit(ExecutionEvent::RunCompleted {
            total_duration_ms: elapsed_ms(started),
        });
    } else {
        timeline.emit(ExecutionEvent::RunFailed {
            phase: result.failure_phase.unwrap_or(ExecutionPhase::Execution),
            error: redactor.text(result.error.as_deref().unwrap_or("execution failed")),
            exit_code: result.exit_code,
            total_duration_ms: elapsed_ms(started),
        });
    }
}

async fn run_with_heartbeat(
    client: &async_nats::Client,
    args: &Args,
    envelope: &DispatchEnvelope,
    execution: ExecutionSnapshot,
    timeline: Arc<ExecutionTimeline>,
    redactor: Arc<Redactor>,
    message: &jetstream::Message,
) -> Result<ExecutionResult> {
    let dry_run = args.dry_run || execution.dry_run;
    let work = execute_snapshot(execution, dry_run, timeline, redactor);
    tokio::pin!(work);
    let mut heartbeat = time::interval(Duration::from_secs(20));
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut work => return result,
            _ = heartbeat.tick() => {
                let renewed: bool = control_request(
                    client,
                    "renew",
                    &args.worker_id,
                    &LeaseRequest {
                        attempt_id: envelope.attempt_id,
                        worker_id: args.worker_id.clone(),
                    },
                )
                .await?;
                if !renewed {
                    bail!("worker lease expired during execution");
                }
                message
                    .ack_with(AckKind::Progress)
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "failed to extend JetStream acknowledgement deadline: {error}"
                        )
                    })?;
            }
        }
    }
}

async fn control_request<T, R>(
    client: &async_nats::Client,
    operation: &str,
    worker_id: &str,
    request: &T,
) -> Result<R>
where
    T: serde::Serialize,
    R: serde::de::DeserializeOwned,
{
    let subject = format!("crono.worker.control.{operation}.{worker_id}");
    let response = client
        .request(subject, serde_json::to_vec(request)?.into())
        .await
        .context("worker control request failed")?;
    serde_json::from_slice(&response.payload).context("worker control response is invalid")
}

fn validate_token(value: &str, label: &str) -> Result<()> {
    crono_api::validate_resource_name(value)
        .with_context(|| format!("{label} must be a canonical DNS-1123 label"))
}

#[cfg(test)]
mod tests;
