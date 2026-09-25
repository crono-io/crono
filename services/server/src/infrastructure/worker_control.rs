//! Server-mediated worker claims over NATS request/reply.
//!
//! Workers never receive PostgreSQL credentials. A queue subscription lets any
//! server instance conditionally change an Attempt against authoritative state.
//! Presence and bounded live output use separate subjects so older control
//! subscribers cannot steal new operations during a rolling deployment. Output
//! writes require the current worker lease and an increasing per-Attempt sequence.
//! The worker identity is repeated in each subject and payload to prevent
//! accidental cross-worker operations; production deployments must additionally
//! enforce these subjects with NATS credentials.

use super::NatsPublisher;
use crate::{
    application::{ControlPlaneStore, StoreError},
    domain::{QueueId, QueueName},
};
use crono_api::{
    ClaimRequest, CompletionRequest, LeaseRequest, OutputSnapshotRequest, QueueReference,
    QueueResolutionRequest, QueueResolutionResponse, QueueResolutionStatus, WorkerHeartbeatRequest,
    validate_resource_name,
};
use futures_util::StreamExt;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const CONTROL_SUBJECT: &str = "crono.worker.control.*.*";
const CONTROL_QUEUE: &str = "crono-server-control";
const OUTPUT_SUBJECT: &str = "crono.worker.output.*";
const OUTPUT_QUEUE: &str = "crono-server-output";
const MAX_OUTPUT_WRITES: usize = 32;
const MAX_LIVE_TAIL_BYTES: usize = 16_384;
const MAX_OUTPUT_REQUEST_BYTES: usize = 262_144;
const PRESENCE_SUBJECT: &str = "crono.worker.presence.*";
const PRESENCE_QUEUE: &str = "crono-server-presence";
const QUEUE_RESOLUTION_SUBJECT: &str = "crono.worker.queue.resolve.*";
const QUEUE_RESOLUTION_QUEUE: &str = "crono-server-queue-resolution";

/// Serve worker state transitions whenever NATS is connected.
pub async fn run_worker_control(
    store: Arc<dyn ControlPlaneStore>,
    nats: NatsPublisher,
    cancellation: CancellationToken,
) {
    loop {
        if cancellation.is_cancelled() {
            info!("worker control loop stopped");
            return;
        }
        let Ok(client) = nats.client() else {
            tokio::select! {
                () = cancellation.cancelled() => continue,
                () = time::sleep(Duration::from_secs(1)) => continue,
            }
        };
        let mut messages = match client
            .queue_subscribe(CONTROL_SUBJECT, CONTROL_QUEUE.to_string())
            .await
        {
            Ok(messages) => messages,
            Err(error) => {
                warn!(%error, "failed to subscribe to worker control subjects");
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mut presence = match client
            .queue_subscribe(PRESENCE_SUBJECT, PRESENCE_QUEUE.to_string())
            .await
        {
            Ok(messages) => messages,
            Err(error) => {
                warn!(%error, "failed to subscribe to worker presence subjects");
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mut output = match client
            .queue_subscribe(OUTPUT_SUBJECT, OUTPUT_QUEUE.to_string())
            .await
        {
            Ok(messages) => messages,
            Err(error) => {
                warn!(%error, "failed to subscribe to worker output subjects");
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let output_limit = Arc::new(Semaphore::new(MAX_OUTPUT_WRITES));
        let mut queue_resolution = match client
            .queue_subscribe(QUEUE_RESOLUTION_SUBJECT, QUEUE_RESOLUTION_QUEUE.to_string())
            .await
        {
            Ok(messages) => messages,
            Err(error) => {
                warn!(%error, "failed to subscribe to worker Queue resolution subjects");
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                message = messages.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    if let Err(error) = handle_control(&store, &client, message).await {
                        warn!(%error, "worker control request failed");
                    }
                }
                message = presence.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    if let Err(error) = handle_presence(&store, &client, message).await {
                        warn!(%error, "worker presence request failed");
                    }
                }
                message = output.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    dispatch_output(&store, &client, &output_limit, message).await;
                }
                message = queue_resolution.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    if let Err(error) = handle_queue_resolution(&store, &client, message).await {
                        warn!(%error, "worker Queue resolution request failed");
                    }
                }
            }
        }
    }
}

/// Isolate bounded output writes from claim, lease, and completion handling.
async fn dispatch_output(
    store: &Arc<dyn ControlPlaneStore>,
    client: &async_nats::Client,
    limit: &Arc<Semaphore>,
    message: async_nats::Message,
) {
    if let Ok(permit) = Arc::clone(limit).try_acquire_owned() {
        let store = Arc::clone(store);
        let client = client.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = handle_output(&store, &client, message).await {
                warn!(%error, "worker output update failed");
            }
        });
    } else if let Some(reply) = message.reply {
        let _ = respond(client, reply, &false).await;
    }
}

/// Accept only bounded output from the worker that still holds the Attempt lease.
async fn handle_output(
    store: &Arc<dyn ControlPlaneStore>,
    client: &async_nats::Client,
    message: async_nats::Message,
) -> anyhow::Result<()> {
    let Some(reply) = message.reply else {
        return Ok(());
    };
    if message.payload.len() > MAX_OUTPUT_REQUEST_BYTES {
        anyhow::bail!("live output request is too large");
    }
    let subject_worker = message
        .subject
        .as_str()
        .split('.')
        .nth(3)
        .unwrap_or_default();
    let request: OutputSnapshotRequest = serde_json::from_slice(&message.payload)?;
    validate_output(&request, subject_worker)?;
    respond(client, reply, &store.record_attempt_output(&request).await?).await
}

/// Keep worker identity and per-stream bounds at the NATS trust boundary.
fn validate_output(request: &OutputSnapshotRequest, subject_worker: &str) -> anyhow::Result<()> {
    if request.worker_id != subject_worker
        || validate_resource_name(&request.worker_id).is_err()
        || request.sequence == 0
        || request.stdout_tail.len() > MAX_LIVE_TAIL_BYTES
        || request.stderr_tail.len() > MAX_LIVE_TAIL_BYTES
    {
        anyhow::bail!("invalid live output request");
    }
    Ok(())
}

async fn handle_control(
    store: &Arc<dyn ControlPlaneStore>,
    client: &async_nats::Client,
    message: async_nats::Message,
) -> anyhow::Result<()> {
    let Some(reply) = message.reply.clone() else {
        return Ok(());
    };
    let mut tokens = message.subject.as_str().split('.');
    let operation = tokens.nth(3).unwrap_or_default();
    let subject_worker = tokens.next().unwrap_or_default();
    match operation {
        "claim" => {
            let request: ClaimRequest = serde_json::from_slice(&message.payload)?;
            if request.worker_id != subject_worker {
                anyhow::bail!("worker identity does not match control subject");
            }
            respond(client, reply, &store.claim_attempt(&request).await?).await?;
        }
        "renew" => {
            let request: LeaseRequest = serde_json::from_slice(&message.payload)?;
            if request.worker_id != subject_worker {
                anyhow::bail!("worker identity does not match control subject");
            }
            respond(client, reply, &store.renew_lease(&request).await?).await?;
        }
        "complete" => {
            let request: CompletionRequest = serde_json::from_slice(&message.payload)?;
            if request.worker_id != subject_worker {
                anyhow::bail!("worker identity does not match control subject");
            }
            respond(client, reply, &store.complete_attempt(&request).await?).await?;
        }
        _ => anyhow::bail!("unsupported worker control operation"),
    }
    Ok(())
}

async fn handle_presence(
    store: &Arc<dyn ControlPlaneStore>,
    client: &async_nats::Client,
    message: async_nats::Message,
) -> anyhow::Result<()> {
    let Some(reply) = message.reply.clone() else {
        return Ok(());
    };
    let subject_worker = message
        .subject
        .as_str()
        .split('.')
        .nth(3)
        .unwrap_or_default();
    let request: WorkerHeartbeatRequest = serde_json::from_slice(&message.payload)?;
    validate_heartbeat(&request, subject_worker)?;
    store.get_queue(QueueId::new(request.queue_id)).await?;
    store.record_worker_heartbeat(&request).await?;
    respond(client, reply, &true).await
}

async fn handle_queue_resolution(
    store: &Arc<dyn ControlPlaneStore>,
    client: &async_nats::Client,
    message: async_nats::Message,
) -> anyhow::Result<()> {
    let Some(reply) = message.reply.clone() else {
        return Ok(());
    };
    let subject_name = message
        .subject
        .as_str()
        .split('.')
        .nth(4)
        .unwrap_or_default();
    let request: QueueResolutionRequest = serde_json::from_slice(&message.payload)?;
    let name = QueueName::parse(&request.name)?;
    if name.as_str() != subject_name {
        anyhow::bail!("Queue name does not match resolution subject");
    }
    let response = match store.get_queue_by_name(&name).await {
        Ok(queue) => QueueResolutionResponse {
            status: QueueResolutionStatus::Ready,
            queue: Some(QueueReference {
                id: queue.id().get(),
                name: queue.name().to_string(),
            }),
        },
        Err(StoreError::NotFound) => QueueResolutionResponse {
            status: QueueResolutionStatus::NotFound,
            queue: None,
        },
        Err(_) => QueueResolutionResponse {
            status: QueueResolutionStatus::Unavailable,
            queue: None,
        },
    };
    respond(client, reply, &response).await
}

/// Validate presence metadata at the server trust boundary.
///
/// The subject identity must agree with the payload, while worker IDs remain
/// restricted to one safe NATS subject token. Queue identity is authoritative
/// only after the store lookup performed by the presence handler.
fn validate_heartbeat(
    request: &WorkerHeartbeatRequest,
    subject_worker: &str,
) -> anyhow::Result<()> {
    if request.worker_id != subject_worker {
        anyhow::bail!("worker identity does not match control subject");
    }
    validate_resource_name(&request.worker_id)
        .map_err(|error| anyhow::anyhow!("worker ID is invalid: {error}"))?;
    if !(1..=256).contains(&request.concurrency) {
        anyhow::bail!("worker concurrency must be between 1 and 256");
    }
    if request.version.is_empty() || request.version.len() > 128 || request.version.contains('\0') {
        anyhow::bail!("worker version must be a bounded, NUL-free value");
    }
    if let Some(details) = &request.diagnostics {
        for value in [
            &details.hostname,
            &details.os,
            &details.architecture,
            &details.default_shell_path,
        ] {
            if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
                anyhow::bail!("worker diagnostics contain an invalid identity or shell path");
            }
        }
        if !details.default_shell_path.starts_with('/') {
            anyhow::bail!("worker default shell path must be absolute");
        }
        for value in [
            details.lang.as_deref(),
            details.lc_all.as_deref(),
            details.tz.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.len() > 128 || value.chars().any(char::is_control) {
                anyhow::bail!("worker environment diagnostics are not bounded");
            }
        }
    }
    Ok(())
}

async fn respond<T: Serialize>(
    client: &async_nats::Client,
    reply: async_nats::Subject,
    response: &T,
) -> anyhow::Result<()> {
    client
        .publish(reply, serde_json::to_vec(response)?.into())
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_heartbeat, validate_output};
    use crono_api::{OutputSnapshotRequest, WorkerHeartbeatRequest};
    use uuid::Uuid;

    fn request() -> WorkerHeartbeatRequest {
        WorkerHeartbeatRequest {
            worker_id: "worker-01".to_string(),
            session_id: Uuid::now_v7(),
            queue_id: Uuid::now_v7(),
            concurrency: 8,
            version: "0.1.0".to_string(),
            diagnostics: None,
        }
    }

    #[test]
    fn heartbeat_requires_matching_bounded_identity_and_metadata() {
        let valid = request();
        assert!(validate_heartbeat(&valid, "worker-01").is_ok());

        let mut mismatched = request();
        mismatched.worker_id = "worker-02".to_string();
        assert!(validate_heartbeat(&mismatched, "worker-01").is_err());

        let mut invalid_worker = request();
        invalid_worker.worker_id = "-worker".to_string();
        assert!(validate_heartbeat(&invalid_worker, "-worker").is_err());

        let mut invalid_concurrency = request();
        invalid_concurrency.concurrency = 0;
        assert!(validate_heartbeat(&invalid_concurrency, "worker-01").is_err());
    }

    #[test]
    fn live_output_requires_identity_sequence_and_bounded_streams() {
        let request = OutputSnapshotRequest {
            attempt_id: Uuid::now_v7(),
            worker_id: "worker-01".to_string(),
            sequence: 1,
            stdout_tail: "started\n".to_string(),
            stderr_tail: String::new(),
        };
        assert!(validate_output(&request, "worker-01").is_ok());
        assert!(validate_output(&request, "worker-02").is_err());
        let invalid_sequence = OutputSnapshotRequest {
            sequence: 0,
            ..request.clone()
        };
        assert!(validate_output(&invalid_sequence, "worker-01").is_err());
        let oversized = OutputSnapshotRequest {
            stdout_tail: "x".repeat(16_385),
            ..request
        };
        assert!(validate_output(&oversized, "worker-01").is_err());
    }
}
