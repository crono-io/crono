//! Runtime boundary reserved for the server's application logic.

use anyhow::{Result, bail};

/// Report the unfinished runtime without pretending the service started.
///
/// # Errors
///
/// Always returns an error until the server runtime is implemented.
#[tracing::instrument(name = "server.run")]
pub fn execute() -> Result<()> {
    tracing::error!("crono-server runtime not implemented");
    bail!("crono-server runtime not implemented");
}
