//! Convert parsed arguments into the API client's typed action contract.
//!
//! Dispatch is the only CLI layer that knows both clap and application
//! configuration. It resolves the address once, constructs the client, and
//! passes ownership to action execution without leaking clap into client code.

use crate::{cli::actions::Action, client::CronoClient, config::Config};
use anyhow::Result;
use clap::ArgMatches;

/// Resolve configuration and select the explicit unfinished-shell action.
///
/// # Errors
///
/// Returns an error for malformed, unsupported, or credential-bearing server
/// addresses. No network connection is attempted during dispatch.
pub fn handler(matches: &ArgMatches) -> Result<Action> {
    let explicit_address = matches.get_one::<String>("address").map(String::as_str);
    let config = Config::load(explicit_address)?;
    let client = CronoClient::new(config.into_client());
    Ok(Action::Unavailable(client))
}
