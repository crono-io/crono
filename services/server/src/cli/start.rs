//! Orchestrate CLI setup; execution belongs to the binary.

use crate::cli::{actions::Action, commands, dispatch, telemetry};
use anyhow::Result;

/// Parse arguments, initialize telemetry, and return the selected action.
///
/// # Errors
///
/// Returns telemetry configuration or dispatch errors. Clap handles help,
/// version output, and invalid arguments before telemetry initialization.
pub fn start() -> Result<Action> {
    let matches = commands::new().get_matches();
    telemetry::init(matches.get_count("verbose"))?;
    dispatch::handler(&matches)
}
