//! HTTP server action boundary.

use crate::{
    api,
    application::{Application, ControlPlaneStore, PermitAllAuthorizer},
    infrastructure::{NatsPublisher, PostgresStore},
};
use anyhow::{Context, Result};
use std::{env, sync::Arc};

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
/// Returns an error when the API listener or server fails.
#[tracing::instrument(name = "server.serve", skip_all, fields(port = args.port))]
pub async fn execute(args: Args) -> Result<()> {
    let database_url = env::var("CRONO_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://crono_runtime@127.0.0.1:5432/crono".to_string());
    let nats_url =
        env::var("CRONO_NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
    let postgres = PostgresStore::connect(&database_url)
        .await
        .context("failed to initialize PostgreSQL control-plane storage")?;
    let store: Arc<dyn ControlPlaneStore> = Arc::new(postgres);
    let application = Application::new(Arc::clone(&store), Arc::new(PermitAllAuthorizer));
    let publisher = NatsPublisher::connect(&nats_url)
        .await
        .context("failed to initialize NATS dispatch")?;

    tracing::warn!(
        principal = "development/local",
        "development authentication policy grants every defined capability"
    );
    let result = api::serve(args.port, application, store, publisher).await;
    if let Err(error) = &result {
        tracing::error!(%error, "Crono API server stopped with an error");
    }
    result
}
