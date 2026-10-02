//! Convert parsed arguments into the service's typed action contract.

use crate::authentication::AuthMode;
use crate::cli::actions::{Action, server};
use anyhow::{Context, Result};
use clap::ArgMatches;

/// Select an action after clap validates the command line.
///
/// # Errors
///
/// Returns an error if validated arguments are unexpectedly unavailable.
pub fn handler(matches: &ArgMatches) -> Result<Action> {
    let port = matches
        .get_one::<u16>("port")
        .copied()
        .context("missing server port")?;

    let auth_mode = matches
        .get_one::<AuthMode>("auth-mode")
        .copied()
        .context("missing authentication mode")?;
    Ok(Action::Server(server::Args { port, auth_mode }))
}
