//! Reconnecting `JetStream` execution transport.
//!
//! NATS is deliberately optional at server startup. A background manager
//! connects, verifies the `WorkQueue` stream, and exposes a reusable context to
//! bounded publishers and worker protocol responders. Losing NATS only delays
//! dispatch because PostgreSQL retains every execution intent.

use anyhow::{Context, Result, anyhow};
use async_nats::jetstream::{
    self,
    context::traits::Publisher,
    message::PublishMessage,
    stream::{Config, DiscardPolicy, RetentionPolicy, StorageType},
};
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

pub const STREAM_NAME: &str = "CRONO_DISPATCH";
pub const DISPATCH_SUBJECTS: &str = "crono.dispatch.*";

#[derive(Debug, Clone)]
pub struct NatsPublisher {
    server_url: Arc<str>,
    context: Arc<RwLock<Option<jetstream::Context>>>,
}

impl NatsPublisher {
    #[must_use]
    pub fn new(server_url: &str) -> Self {
        Self {
            server_url: Arc::from(server_url),
            context: Arc::new(RwLock::new(None)),
        }
    }

    /// Maintain a reusable NATS connection and stream until shutdown.
    pub async fn run_connection_manager(&self, cancellation: CancellationToken) {
        loop {
            tokio::select! {
                () = cancellation.cancelled() => {
                    self.clear();
                    info!("NATS connection manager stopped");
                    return;
                }
                () = time::sleep(Duration::from_secs(2)) => {
                    if self.ready().await {
                        continue;
                    }
                    self.clear();
                    match self.connect_once().await {
                        Ok(context) => {
                            self.replace(context);
                            info!("NATS JetStream execution transport is available");
                        }
                        Err(error) => warn!(%error, "NATS unavailable; durable dispatch remains in PostgreSQL"),
                    }
                }
            }
        }
    }

    async fn connect_once(&self) -> Result<jetstream::Context> {
        let client = async_nats::ConnectOptions::new()
            .max_reconnects(None)
            .connect(self.server_url.as_ref())
            .await
            .context("failed to connect to NATS")?;
        let context = jetstream::new(client);
        context
            .create_or_update_stream(Config {
                name: STREAM_NAME.to_string(),
                description: Some("Crono durable execution dispatches".to_string()),
                subjects: vec![DISPATCH_SUBJECTS.to_string()],
                retention: RetentionPolicy::WorkQueue,
                storage: StorageType::File,
                discard: DiscardPolicy::New,
                max_bytes: 8 * 1024 * 1024 * 1024,
                max_message_size: 64 * 1024,
                duplicate_window: Duration::from_secs(120),
                num_replicas: nats_replicas(),
                ..Config::default()
            })
            .await
            .context("failed to ensure the Crono execution stream")?;
        Ok(context)
    }

    /// Publish one execution event and wait for its persistence acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns when there is no active connection or `JetStream` does not confirm
    /// durable persistence.
    pub async fn publish(
        &self,
        subject: String,
        payload: Vec<u8>,
        attempt_id: Uuid,
    ) -> Result<u64> {
        let context = self.current()?;
        let message = PublishMessage::build()
            .message_id(attempt_id.to_string())
            .payload(payload.into())
            .outbound_message(subject);
        let acknowledgement = context
            .publish_message(message)
            .await
            .context("failed to publish an execution dispatch")?
            .await
            .context("JetStream did not acknowledge an execution dispatch")?;
        Ok(acknowledgement.sequence)
    }

    /// Probe transport health without changing server readiness.
    pub async fn ready(&self) -> bool {
        let Ok(context) = self.current() else {
            return false;
        };
        context.get_stream(STREAM_NAME).await.is_ok()
    }

    /// Return the connected Core NATS client used by worker control handlers.
    ///
    /// # Errors
    ///
    /// Returns when NATS is currently disconnected.
    pub fn client(&self) -> Result<async_nats::Client> {
        Ok(self.current()?.client())
    }

    fn current(&self) -> Result<jetstream::Context> {
        self.context
            .read()
            .map_err(|_| anyhow!("NATS connection state lock is poisoned"))?
            .clone()
            .ok_or_else(|| anyhow!("NATS is not connected"))
    }

    fn replace(&self, context: jetstream::Context) {
        match self.context.write() {
            Ok(mut slot) => {
                *slot = Some(context);
                crate::metrics::global().nats_connected.set(1);
            }
            Err(error) => warn!(%error, "failed to store NATS connection state"),
        }
    }

    fn clear(&self) {
        crate::metrics::global().nats_connected.set(0);
        match self.context.write() {
            Ok(mut slot) => *slot = None,
            Err(error) => warn!(%error, "failed to clear NATS connection state"),
        }
    }
}

fn nats_replicas() -> usize {
    std::env::var("CRONO_NATS_REPLICAS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| matches!(value, 1 | 3 | 5))
        .unwrap_or(1)
}
