//! Typed client actions and their user-facing execution behavior.
//!
//! No operational actions exist until `crono-server` publishes stable public
//! contracts. The sole action proves the configuration and client boundaries
//! without performing network I/O or pretending an API operation succeeded.

use crate::client::CronoClient;
use anyhow::{Result, bail};

/// Operations selected by the command line.
pub enum Action {
    /// Validated client shell with no operational command available yet.
    Unavailable(CronoClient),
}

impl Action {
    /// Return the configured API client without exposing transport internals.
    #[must_use]
    pub const fn client(&self) -> &CronoClient {
        match self {
            Self::Unavailable(client) => client,
        }
    }
}

/// Execute a client action without inventing an unavailable server operation.
///
/// # Errors
///
/// Always reports that operational commands are not implemented. The validated
/// client is never used to make a request in this initial shell.
pub fn execute(action: Action) -> Result<()> {
    match action {
        Action::Unavailable(_client) => {
            bail!("crono operational commands are not implemented; use --help")
        }
    }
}
