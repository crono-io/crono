//! Convert parsed arguments into the service's typed action contract.

use crate::cli::actions::{Action, run};
use anyhow::{Context, Result, bail};
use clap::ArgMatches;

/// Select an action after clap validates the command line.
///
/// # Errors
///
/// Returns an error if no supported action was selected.
pub fn handler(matches: &ArgMatches) -> Result<Action> {
    match matches.subcommand() {
        Some(("run", values)) => Ok(Action::Run(run::Args {
            nats_url: values
                .get_one::<String>("nats-url")
                .cloned()
                .context("missing NATS URL")?,
            queue: values
                .get_one::<String>("queue")
                .cloned()
                .context("missing worker queue")?,
            worker_id: values
                .get_one::<String>("worker-id")
                .cloned()
                .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
            concurrency: values
                .get_one::<u16>("concurrency")
                .copied()
                .context("missing worker concurrency")?,
        })),
        _ => bail!("a supported subcommand is required"),
    }
}
