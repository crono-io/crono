//! HTTP-owned dependencies and readiness checks.

use crate::{
    application::{Application, ControlPlaneStore},
    infrastructure::NatsPublisher,
};
use std::sync::Arc;

/// Cloneable dependencies available to request handlers.
#[derive(Clone)]
pub struct AppState {
    application: Application,
    store: Arc<dyn ControlPlaneStore>,
    publisher: NatsPublisher,
}

impl AppState {
    /// Construct HTTP state from application and infrastructure boundaries.
    #[must_use]
    pub fn new(
        application: Application,
        store: Arc<dyn ControlPlaneStore>,
        publisher: NatsPublisher,
    ) -> Self {
        Self {
            application,
            store,
            publisher,
        }
    }

    /// Return the authorization-enforcing application facade.
    #[must_use]
    pub const fn application(&self) -> &Application {
        &self.application
    }

    /// Check PostgreSQL, the durability boundary required to accept work.
    pub async fn ready(&self) -> bool {
        self.store.ready().await
    }

    /// Report whether the optional `JetStream` transport is currently usable.
    pub async fn nats_ready(&self) -> bool {
        self.publisher.ready().await
    }

    /// Refresh database-backed gauges before metrics exposition.
    pub async fn refresh_metrics(&self) -> bool {
        let Ok(snapshot) = self.store.metrics_snapshot().await else {
            return false;
        };
        let metrics = crate::metrics::global();
        metrics.outbox_pending.set(snapshot.outbox_pending);
        metrics
            .outbox_oldest_seconds
            .set(snapshot.outbox_oldest_seconds);
        metrics.execution_queued.set(snapshot.execution_queued);
        metrics.execution_running.set(snapshot.execution_running);
        metrics.worker_active.set(snapshot.worker_active);
        true
    }
}
