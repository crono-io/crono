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
use crate::application::ControlPlaneStore;
use crono_api::{ClaimRequest, CompletionRequest, LeaseRequest, WorkerHeartbeatRequest};
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
    store.record_worker_heartbeat(&request).await?;
    respond(client, reply, &true).await
}

/// Validate presence metadata at the server trust boundary.
///
/// The subject identity must agree with the payload, while worker IDs and queues
/// remain restricted to the same single-token grammar used by dispatch subjects.
fn validate_heartbeat(
    request: &WorkerHeartbeatRequest,
    subject_worker: &str,
) -> anyhow::Result<()> {
    if request.worker_id != subject_worker {
        anyhow::bail!("worker identity does not match control subject");
    }
    for (value, label) in [(&request.worker_id, "worker ID"), (&request.queue, "queue")] {
        let valid = !value.is_empty()
            && value.len() <= 63
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !valid {
            anyhow::bail!("{label} must be a lowercase NATS-safe token");
        }
    }
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
            queue: "default".to_string(),
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

        let mut invalid_queue = request();
        invalid_queue.queue = "other.queue".to_string();
        assert!(validate_heartbeat(&invalid_queue, "worker-01").is_err());

        let mut invalid_concurrency = request();
        invalid_concurrency.concurrency = 0;
        assert!(validate_heartbeat(&invalid_concurrency, "worker-01").is_err());
    }
}
