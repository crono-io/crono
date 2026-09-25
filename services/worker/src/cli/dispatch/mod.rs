//! Convert parsed arguments into the service's typed action contract.

use crate::cli::actions::{Action, run};
use crate::execution::LogFormat;
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
                .unwrap_or_else(default_worker_id),
            concurrency: values
                .get_one::<u16>("concurrency")
                .copied()
                .context("missing worker concurrency")?,
            dry_run: values.get_flag("dry-run"),
            log_format: match values.get_one::<String>("log-format").map(String::as_str) {
                Some("json") => LogFormat::Json,
                _ => LogFormat::Pretty,
            },
        })),
        _ => bail!("a supported subcommand is required"),
    }
}

/// Give each process an identifiable host prefix without persisting hardware IDs.
fn default_worker_id() -> String {
    let hostname = whoami::fallible::hostname().unwrap_or_else(|_| "worker".to_string());
    let mut prefix = hostname
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    prefix.truncate(46);
    let prefix = prefix.trim_matches('-');
    let prefix = if prefix.is_empty() { "worker" } else { prefix };
    let random = uuid::Uuid::now_v7().simple().to_string();
    let suffix = random.get(16..).unwrap_or(random.as_str());
    format!("{prefix}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::default_worker_id;

    #[test]
    fn default_identity_is_bounded_and_unique_per_process() {
        let first = default_worker_id();
        let second = default_worker_id();
        assert_ne!(first, second);
        assert!(first.len() <= 63);
        assert!(first.chars().all(|character| character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'));
    }
}
