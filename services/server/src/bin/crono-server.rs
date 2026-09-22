//! Process entrypoint: execute a typed action and flush telemetry before exit.

use anyhow::{Context, Result};
use crono_server::cli;

/// Preserve the action result while reporting any telemetry shutdown failure.
#[tokio::main]
async fn main() -> Result<()> {
    let result = match cli::start() {
        Ok(cli::actions::Action::Server(args)) => cli::actions::server::execute(args).await,
        Err(error) => Err(error),
    };

    // The blocking SDK shutdown must run while Tokio can still drive gRPC I/O.
    let shutdown = tokio::task::spawn_blocking(cli::telemetry::shutdown)
        .await
        .context("telemetry shutdown task failed")
        .and_then(std::convert::identity);
    if let Err(error) = shutdown {
        eprintln!("Telemetry shutdown failed: {error:#}");
    }

    result
}
