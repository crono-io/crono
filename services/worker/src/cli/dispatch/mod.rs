//! Convert parsed arguments into the service's typed action contract.

use crate::cli::actions::Action;
use anyhow::{Result, bail};
use clap::ArgMatches;

/// Select an action after clap validates the command line.
///
/// # Errors
///
/// Returns an error if no supported action was selected.
pub fn handler(matches: &ArgMatches) -> Result<Action> {
    match matches.subcommand_name() {
        Some("run") => Ok(Action::Run),
        _ => bail!("a supported subcommand is required"),
    }
}
