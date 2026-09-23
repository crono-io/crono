//! Bounded `JetStream` pull-consumer and process executor.
//!
//! Every delivery is claimed through the server before execution. Long-running
//! work refreshes both the PostgreSQL lease and `JetStream` acknowledgement
//! deadline. Completion is persisted before the message is acknowledged, so a
//! crash may redeliver but cannot create another logical Attempt.

use anyhow::{Context, Result, bail};
use async_nats::jetstream::{
    self,
    consumer::{AckPolicy, pull},
    message::AckKind,
};
use crono_api::{
    ClaimRequest, ClaimResponse, CompletionRequest, DispatchEnvelope, ExecutionSnapshot,
    ExecutorKind, LeaseRequest,
};
use futures_util::StreamExt;
use std::{env, io::Write, process::Stdio, sync::Arc, time::Duration};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time,
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const STREAM_NAME: &str = "CRONO_DISPATCH";
const OUTPUT_LIMIT: usize = 65_536;
const INPUT_LIMIT: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub nats_url: String,
    pub queue: String,
    pub worker_id: String,
    pub concurrency: u16,
}

#[derive(Debug)]
struct ExecutionResult {
    succeeded: bool,
    exit_code: Option<i32>,
    stdout_tail: String,
    stderr_tail: String,
    error: Option<String>,
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
    let context = jetstream::new(client.clone());
    let stream = context
        .get_stream(STREAM_NAME)
        .await
        .context("Crono execution stream is unavailable")?;
    let durable = format!("crono-{}", args.queue);
    let consumer = stream
        .get_or_create_consumer(
            &durable,
            pull::Config {
                durable_name: Some(durable.clone()),
                description: Some(format!("Crono workers for queue {}", args.queue)),
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_secs(90),
                max_ack_pending: i64::from(args.concurrency),
                filter_subject: format!("crono.dispatch.{}", args.queue),
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
    info!(
        worker_id = args.worker_id,
        queue = args.queue,
        concurrency = args.concurrency,
        "Crono worker started"
    );
    let args = Arc::new(args);
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
                handle_message(client.clone(), Arc::clone(&args), message)
            })
            .await;
    }
    info!("Crono worker stopped");
    Ok(())
}

async fn handle_message(client: async_nats::Client, args: Arc<Args>, message: jetstream::Message) {
    let envelope: DispatchEnvelope = match serde_json::from_slice(&message.payload) {
        Ok(envelope) => envelope,
        Err(error) => {
            warn!(%error, "terminating malformed execution message");
            let _ = message.ack_with(AckKind::Term).await;
            return;
        }
    };
    let claim = ClaimRequest {
        run_id: envelope.run_id,
        attempt_id: envelope.attempt_id,
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
    let result = run_with_heartbeat(
        &client,
        &args.worker_id,
        envelope.attempt_id,
        execution,
        &message,
    )
    .await;
    let result = match result {
        Ok(result) => result,
        Err(error) => ExecutionResult {
            succeeded: false,
            exit_code: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(error.to_string()),
        },
    };
    let completion = CompletionRequest {
        attempt_id: envelope.attempt_id,
        worker_id: args.worker_id.clone(),
        succeeded: result.succeeded,
        exit_code: result.exit_code,
        stdout_tail: result.stdout_tail,
        stderr_tail: result.stderr_tail,
        error: result.error,
    };
    match control_request::<_, bool>(&client, "complete", &args.worker_id, &completion).await {
        Ok(true) => {
            let _ = message.double_ack().await;
        }
        Ok(false) => {
            warn!(attempt_id = %envelope.attempt_id, "completion lost its worker lease");
            let _ = message.ack_with(AckKind::Term).await;
        }
        Err(error) => {
            warn!(%error, attempt_id = %envelope.attempt_id, "completion was not confirmed");
            let _ = message
                .ack_with(AckKind::Nak(Some(Duration::from_secs(5))))
                .await;
        }
    }
}

async fn run_with_heartbeat(
    client: &async_nats::Client,
    worker_id: &str,
    attempt_id: uuid::Uuid,
    execution: ExecutionSnapshot,
    message: &jetstream::Message,
) -> Result<ExecutionResult> {
    let work = execute_snapshot(execution);
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
                    worker_id,
                    &LeaseRequest {
                        attempt_id,
                        worker_id: worker_id.to_string(),
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

async fn execute_snapshot(execution: ExecutionSnapshot) -> Result<ExecutionResult> {
    match execution.executor {
        ExecutorKind::Noop => Ok(ExecutionResult {
            succeeded: true,
            exit_code: Some(0),
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: None,
        }),
        ExecutorKind::Process => execute_process(execution).await,
    }
}

async fn execute_process(execution: ExecutionSnapshot) -> Result<ExecutionResult> {
    let executable = execution
        .executable
        .as_deref()
        .context("process snapshot has no executable")?;
    if !executable.starts_with('/') {
        bail!("process executable is not absolute");
    }
    let inputs = serde_json::to_vec(&execution.inputs)?;
    if inputs.len() > INPUT_LIMIT {
        bail!("execution inputs exceed the worker limit");
    }
    let mut input_file = NamedTempFile::new().context("failed to create execution input file")?;
    input_file
        .write_all(&inputs)
        .context("failed to write execution input file")?;
    input_file
        .flush()
        .context("failed to flush execution input file")?;

    let mut command = Command::new(executable);
    command
        .args(&execution.arguments)
        .env_clear()
        .env("CRONO_INPUTS_FILE", input_file.path())
        .env("CRONO_RUN_ID", execution.idempotency_key.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in ["LANG", "LC_ALL", "TZ"] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command
        .spawn()
        .context("failed to spawn process executor")?;
    let stdout = child
        .stdout
        .take()
        .context("process stdout pipe is missing")?;
    let stderr = child
        .stderr
        .take()
        .context("process stderr pipe is missing")?;
    let stdout_task = tokio::spawn(read_tail(stdout));
    let stderr_task = tokio::spawn(read_tail(stderr));
    let status = child.wait().await.context("failed to wait for process")?;
    let stdout_tail = stdout_task.await.context("stdout reader task failed")??;
    let stderr_tail = stderr_task.await.context("stderr reader task failed")??;
    Ok(ExecutionResult {
        succeeded: status.success(),
        exit_code: status.code(),
        stdout_tail,
        stderr_tail,
        error: (!status.success()).then(|| "process exited unsuccessfully".to_string()),
    })
}

async fn read_tail(mut reader: impl AsyncRead + Unpin) -> Result<String> {
    let mut tail = Vec::with_capacity(OUTPUT_LIMIT);
    let mut chunk = vec![0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        tail.extend_from_slice(chunk.get(..read).context("invalid output read size")?);
        if tail.len() > OUTPUT_LIMIT {
            let excess = tail.len() - OUTPUT_LIMIT;
            tail.drain(..excess);
        }
    }
    Ok(bounded_utf8_tail(&tail))
}

/// Decode a byte tail while retaining the database's byte-size invariant even
/// when invalid UTF-8 expands into multi-byte replacement characters.
fn bounded_utf8_tail(bytes: &[u8]) -> String {
    let value = String::from_utf8_lossy(bytes);
    if value.len() <= OUTPUT_LIMIT {
        return value.into_owned();
    }
    let mut start = value.len() - OUTPUT_LIMIT;
    while !value.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    value.get(start..).unwrap_or_default().to_owned()
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
    let valid = !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        bail!("{label} must be a lowercase NATS-safe token")
    }
}

#[cfg(test)]
mod tests {
    use super::{OUTPUT_LIMIT, read_tail, validate_token};
    use anyhow::Result;

    #[test]
    fn validates_subject_tokens() {
        assert!(validate_token("worker-01", "worker").is_ok());
        assert!(validate_token("Worker.01", "worker").is_err());
    }

    #[tokio::test]
    async fn output_capture_retains_only_the_tail() -> Result<()> {
        let input = vec![b'x'; OUTPUT_LIMIT + 10];
        let output = read_tail(input.as_slice()).await?;
        assert_eq!(output.len(), OUTPUT_LIMIT);
        Ok(())
    }
}
