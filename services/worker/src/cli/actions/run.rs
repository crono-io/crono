//! Runtime boundary reserved for the worker's application logic.

use anyhow::{Result, bail};

/// Report the unfinished runtime without pretending the service started.
///
/// # Errors
///
/// Always returns an error until the worker runtime is implemented.
#[tracing::instrument(name = "worker.run")]
pub fn execute() -> Result<()> {
    tracing::error!("crono-worker runtime not implemented");
    bail!("crono-worker runtime not implemented");
}
