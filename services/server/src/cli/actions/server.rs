//! HTTP server action boundary.

use crate::api;
use anyhow::Result;

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
    let result = api::serve(args.port).await;
    if let Err(error) = &result {
        tracing::error!(%error, "Crono API server stopped with an error");
    }
    result
}
