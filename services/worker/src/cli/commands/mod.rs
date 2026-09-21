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
        .subcommand(Command::new("run").about("Start the worker (not implemented yet)"))
}
