//! Transactional outbox delivery loop.
//!
//! The loop observes only committed rows, publishes their stored versioned
//! payload, waits for `JetStream` acknowledgement, and then atomically marks the
//! outbox row and Run dispatched. Failed sends remain pending and are retried;
//! their bounded error detail is operational data and is never returned by the
//! HTTP API.

use super::NatsPublisher;
use crate::application::ControlPlaneStore;
use std::{sync::Arc, time::Duration};
use tokio::time::{self, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const BATCH_SIZE: u16 = 50;

/// Deliver committed outbox rows until process shutdown is requested.
pub async fn run_dispatcher(
    store: Arc<dyn ControlPlaneStore>,
    publisher: NatsPublisher,
    cancellation: CancellationToken,
) {
    let mut interval = time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                info!("Run dispatch loop stopped");
                return;
            }
            _ = interval.tick() => dispatch_batch(store.as_ref(), &publisher).await,
        }
    }
}

async fn dispatch_batch(store: &dyn ControlPlaneStore, publisher: &NatsPublisher) {
    let records = match store.pending_outbox(BATCH_SIZE).await {
        Ok(records) => records,
        Err(error) => {
            warn!(%error, "failed to read the Run dispatch outbox");
            return;
        }
    };
    for record in records {
        match publisher
            .publish(record.subject, record.payload, record.id.get())
            .await
        {
            Ok(sequence) => {
                if let Err(error) = store
                    .mark_published(record.id, record.run_id, sequence)
                    .await
                {
                    error!(%error, dispatch_id = %record.id.get(), "failed to record acknowledged Run dispatch");
                }
            }
            Err(publish_error) => {
                warn!(error = %publish_error, dispatch_id = %record.id.get(), "Run dispatch remains pending");
                if let Err(store_error) = store
                    .record_publish_failure(record.id, &publish_error.to_string())
                    .await
                {
                    error!(%store_error, dispatch_id = %record.id.get(), "failed to record Run dispatch failure");
                }
            }
        }
    }
}
