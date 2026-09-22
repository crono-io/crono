//! Top-level CLI parsing orchestration.

use crate::cli::{actions::Action, commands, dispatch};
use anyhow::Result;

/// Parse process arguments and return a fully configured typed action.
///
/// # Errors
///
/// Returns an error when client configuration is invalid. Clap handles help,
/// version output, and syntactically invalid arguments before dispatch.
pub fn start() -> Result<Action> {
    let matches = commands::new().get_matches();
    dispatch::handler(&matches)
}
