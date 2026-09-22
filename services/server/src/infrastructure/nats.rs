//! `JetStream` publisher for durable Run dispatch messages.
//!
//! Startup verifies or creates one bounded work-queue stream. Each outbox row
//! supplies the message ID, allowing `JetStream`'s duplicate window to collapse
//! retries after an acknowledgement is lost. Message subjects contain only a
//! validated queue token created by the application layer.

use anyhow::{Context, Result};
use async_nats::jetstream::{
    self,
    message::PublishMessage,
    stream::{Config, RetentionPolicy, StorageType},
};
use std::time::Duration;
use uuid::Uuid;

const STREAM_NAME: &str = "CRONO_DISPATCH";

/// Connected `JetStream` publisher with a preconfigured dispatch stream.
#[derive(Debug, Clone)]
pub struct NatsPublisher {
    context: jetstream::Context,
}

impl NatsPublisher {
    /// Connect to NATS and ensure the durable dispatch stream exists.
    ///
    /// # Errors
    ///
    /// Returns an error when NATS or `JetStream` is unavailable or rejects the
    /// bounded stream configuration.
    pub async fn connect(server_url: &str) -> Result<Self> {
        let client = async_nats::connect(server_url)
            .await
            .context("failed to connect to NATS")?;
        let context = jetstream::new(client);
        context
            .get_or_create_stream(Config {
                name: STREAM_NAME.to_string(),
                description: Some("Crono durable Run dispatches".to_string()),
                subjects: vec!["crono.dispatch.*".to_string()],
                retention: RetentionPolicy::WorkQueue,
                storage: StorageType::File,
                max_messages: 10_000,
                max_bytes: 64 * 1024 * 1024,
                max_age: Duration::from_hours(24),
                max_message_size: 64 * 1024,
                duplicate_window: Duration::from_mins(2),
                ..Config::default()
            })
            .await
            .context("failed to ensure the Crono JetStream dispatch stream")?;
        Ok(Self { context })
    }

    /// Publish one outbox payload and wait for a durable stream acknowledgement.
    ///
    /// The dispatch UUID is sent as `Nats-Msg-Id`; retrying the same row during
    /// the stream duplicate window therefore returns an acknowledgement without
    /// storing a second message.
    ///
    /// # Errors
    ///
    /// Returns an error unless both publication and acknowledgement succeed.
    pub async fn publish(
        &self,
        subject: String,
        payload: Vec<u8>,
        dispatch_id: Uuid,
    ) -> Result<u64> {
        let acknowledgement = self
            .context
            .send_publish(
                subject,
                PublishMessage::build()
                    .message_id(dispatch_id.to_string())
                    .payload(payload.into()),
            )
            .await
            .context("failed to publish a Run dispatch")?
            .await
            .context("JetStream did not acknowledge a Run dispatch")?;
        Ok(acknowledgement.sequence)
    }

    /// Probe the `JetStream` account with a bounded client request timeout.
    pub async fn ready(&self) -> bool {
        self.context.query_account().await.is_ok()
    }
}
