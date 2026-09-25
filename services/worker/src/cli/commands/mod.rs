//! Clap definitions only; dispatch validates and selects the action.

use clap::{
    Arg, ArgAction, ColorChoice, Command,
    builder::styling::{AnsiColor, Effects, Styles},
};
use std::sync::LazyLock;

static LONG_VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{} - {}",
        env!("CARGO_PKG_VERSION"),
        crate::built_info::GIT_COMMIT_HASH.unwrap_or("unknown")
    )
});

/// Build the CLI without initializing logging or contacting external services.
#[must_use]
pub fn new() -> Command {
    let styles = Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Blue.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default());

    Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .long_version(LONG_VERSION.as_str())
        .color(ColorChoice::Auto)
        .styles(styles)
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg_required_else_help(true)
        .subcommand_required(true)
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .global(true)
                .action(ArgAction::Count)
                .help("Increase logging verbosity: -v info, -vv debug, -vvv trace"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Print claimed commands without executing them; Runs still succeed"),
        )
        .arg(
            Arg::new("log-format")
                .long("log-format")
                .global(true)
                .value_parser(["pretty", "json"])
                .default_value("pretty")
                .help("Execution timeline format on stderr (worker diagnostics remain JSON)"),
        )
        .subcommand(
            Command::new("run")
                .about("Start the JetStream execution worker")
                .arg(
                    Arg::new("nats-url")
                        .long("nats-url")
                        .env("CRONO_NATS_URL")
                        .default_value("nats://127.0.0.1:4222"),
                )
                .arg(
                    Arg::new("queue")
                        .long("queue")
                        .env("CRONO_WORKER_QUEUE")
                        .default_value("default"),
                )
                .arg(
                    Arg::new("worker-id")
                        .long("worker-id")
                        .env("CRONO_WORKER_ID"),
                )
                .arg(
                    Arg::new("concurrency")
                        .long("concurrency")
                        .env("CRONO_WORKER_CONCURRENCY")
                        .value_parser(clap::value_parser!(u16).range(1..=256))
                        .default_value("8"),
                ),
        )
}
