//! HTTP server action boundary.
//!
//! Startup validates pool and outbox configuration before touching any
//! dependency, connects the PostgreSQL pool, and hands it to the API. After the
//! API and every background loop have stopped, the pool is closed so PostgreSQL
//! sees a clean terminate for each connection rather than a dropped socket.

use crate::{
    api,
    application::{Application, ControlPlaneStore, PermitAllAuthorizer},
    infrastructure::{DatabasePoolConfig, DispatcherConfig, NatsPublisher, PostgresStore},
};
use anyhow::{Context, Result};
use std::{env, sync::Arc, time::Duration};

/// Upper bound on waiting for pooled connections to close at shutdown.
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Validated server startup arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Args {
    /// TCP port exposed on the wildcard listener.
    pub port: u16,
}

/// Run the Crono HTTP API until graceful shutdown.
///
/// # Errors
///
/// Returns an error when configuration is invalid, PostgreSQL is unreachable
/// at startup, or the API listener or server fails.
#[tracing::instrument(name = "server.serve", skip_all, fields(port = args.port))]
pub async fn execute(args: Args) -> Result<()> {
    let database_url = env::var("CRONO_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://crono_runtime@127.0.0.1:5432/crono".to_string());
    let nats_url =
        env::var("CRONO_NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let pool_config =
        DatabasePoolConfig::from_env().context("failed to load PostgreSQL pool configuration")?;
    let dispatcher_config =
        DispatcherConfig::from_env().context("failed to load outbox publisher configuration")?;
    let postgres = PostgresStore::connect(&database_url, &pool_config)
        .await
        .context("failed to initialize PostgreSQL control-plane storage")?;
    let store: Arc<dyn ControlPlaneStore> = Arc::new(postgres.clone());
    let application = Application::new(Arc::clone(&store), Arc::new(PermitAllAuthorizer));
    let publisher = NatsPublisher::new(&nats_url);

    tracing::warn!(
        principal = "development/local",
        "development authentication policy grants every defined capability"
    );
    let result = api::serve(args.port, application, store, publisher, dispatcher_config).await;
    if let Err(error) = &result {
        tracing::error!(%error, "Crono API server stopped with an error");
    }
    // A background task that failed to join may still hold a connection, so
    // the close is bounded rather than allowed to stall process exit.
    if tokio::time::timeout(POOL_CLOSE_TIMEOUT, postgres.close())
        .await
        .is_err()
    {
        tracing::warn!(
            timeout_seconds = POOL_CLOSE_TIMEOUT.as_secs(),
            "PostgreSQL pool did not close in time; remaining connections are dropped"
        );
    }
    result
}
