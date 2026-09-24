//! Server-mediated worker claims over NATS request/reply.
//!
//! Workers never receive PostgreSQL credentials. A queue subscription lets any
//! server instance conditionally changes an Attempt against authoritative state.
//! Presence uses a separate subject so an older control subscriber cannot steal
//! and reject new heartbeat operations during a rolling deployment. The worker
//! identity is repeated in each subject and payload to prevent accidental
//! cross-worker operations; production deployments must additionally enforce
//! these subjects with NATS credentials.

use super::NatsPublisher;
use crate::{
    application::{ControlPlaneStore, StoreError},
    domain::{QueueId, QueueName},
};
use crono_api::{
    ClaimRequest, CompletionRequest, LeaseRequest, QueueReference, QueueResolutionRequest,
    QueueResolutionResponse, QueueResolutionStatus, WorkerHeartbeatRequest, validate_resource_name,
};
use futures_util::StreamExt;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const CONTROL_SUBJECT: &str = "crono.worker.control.*.*";
const CONTROL_QUEUE: &str = "crono-server-control";
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
    use super::validate_heartbeat;
    use crono_api::WorkerHeartbeatRequest;
    use uuid::Uuid;

    fn request() -> WorkerHeartbeatRequest {
        WorkerHeartbeatRequest {
            worker_id: "worker-01".to_string(),
            session_id: Uuid::now_v7(),
            queue_id: Uuid::now_v7(),
            concurrency: 8,
            version: "0.1.0".to_string(),
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
}
