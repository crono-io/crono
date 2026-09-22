//! Clap definitions for the human API client.
//!
//! This module describes only user-facing syntax. It does not read environment
//! defaults, validate URLs, construct clients, or perform HTTP operations.

use clap::{
    Arg, ColorChoice, Command,
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

/// Build the command tree without resolving configuration or contacting a server.
#[must_use]
pub fn new() -> Command {
    let styles = Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Blue.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default());

    Command::new("crono")
        .version(env!("CARGO_PKG_VERSION"))
        .long_version(LONG_VERSION.as_str())
        .color(ColorChoice::Auto)
        .styles(styles)
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::new("address")
                .long("address")
                .global(true)
                .value_name("URL")
                .help("Crono server address; overrides CRONO_ADDR"),
        )
}
