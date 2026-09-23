//! Server-mediated worker claims over NATS request/reply.
//!
//! Workers never receive PostgreSQL credentials. A queue subscription lets any
//! server instance conditionally claim, renew, or complete an Attempt against
//! authoritative state. The worker identity is repeated in the subject and
//! payload to prevent accidental cross-worker lease operations; production
//! deployments must additionally enforce these subjects with NATS credentials.

use super::NatsPublisher;
use crate::application::ControlPlaneStore;
use crono_api::{ClaimRequest, CompletionRequest, LeaseRequest};
use futures_util::StreamExt;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

const CONTROL_SUBJECT: &str = "crono.worker.control.*.*";
const CONTROL_QUEUE: &str = "crono-server-control";

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
        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                message = messages.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    if let Err(error) = handle(&store, &client, message).await {
                        warn!(%error, "worker control request failed");
                    }
                }
            }
        }
    }
}

async fn handle(
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
