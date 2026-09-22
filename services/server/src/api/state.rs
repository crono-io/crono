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

    /// Check both dependencies used to accept and dispatch Run requests.
    pub async fn ready(&self) -> bool {
        let (database, nats) = tokio::join!(self.store.ready(), self.publisher.ready());
        database && nats
    }
}
