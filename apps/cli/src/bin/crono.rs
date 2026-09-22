//! Process entrypoint for the human Crono API client.

use anyhow::Result;

fn main() -> Result<()> {
    crono_cli::cli::start().and_then(crono_cli::cli::actions::execute)
}
