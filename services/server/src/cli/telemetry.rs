//! Service metadata adapter for the shared native telemetry implementation.

use anyhow::Result;

pub use crono_telemetry::shutdown;

/// Initialize logging using this service's name and workspace version.
///
/// # Errors
///
/// Returns an error for invalid configuration or repeated global initialization.
pub fn init(verbosity: u8) -> Result<()> {
    crono_telemetry::init(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        crono_telemetry::verbosity_level(verbosity),
    )
}
