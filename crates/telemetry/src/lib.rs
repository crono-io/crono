//! Shared native logging and optional OTLP trace export.
//!
//! Services supply their own name and version, initialize once during startup,
//! and call `shutdown` on a blocking thread before their Tokio runtime ends.
//! Browser code does not depend on this crate. Local JSON logs go to stderr;
//! export requires both the `telemetry` feature and an explicit endpoint.

use anyhow::Result;
use tracing::Level;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(feature = "telemetry")]
mod otlp;

/// Map repeatable CLI verbosity to a logging threshold.
#[must_use]
pub const fn verbosity_level(count: u8) -> Level {
    match count {
        0 => Level::ERROR,
        1 => Level::INFO,
        2 => Level::DEBUG,
        _ => Level::TRACE,
    }
}

/// Initialize process-wide logging and, when enabled and configured, tracing.
///
/// `RUST_LOG` directives override the verbosity-derived default. Service metadata
/// comes from the calling application, never this library's package name.
///
/// # Errors
///
/// Returns an error for invalid logging/export configuration or if a global
/// subscriber has already been installed.
pub fn init(service_name: &'static str, service_version: &'static str, level: Level) -> Result<()> {
    let filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env()?;
    let console = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false)
        .with_writer(std::io::stderr);
    let subscriber = tracing_subscriber::registry().with(filter).with(console);

    #[cfg(feature = "telemetry")]
    let provider = otlp::provider(service_name, service_version)?;
    #[cfg(feature = "telemetry")]
    let subscriber = {
        use opentelemetry::trace::TracerProvider as _;
        let layer = provider.as_ref().map(|provider| {
            tracing_opentelemetry::layer().with_tracer(provider.tracer(service_name))
        });
        subscriber.with(layer)
    };

    subscriber.try_init()?;
    #[cfg(feature = "telemetry")]
    if let Some(provider) = provider {
        otlp::install(provider)?;
    }

    tracing::debug!(service_name, service_version, "telemetry initialized");
    Ok(())
}

/// Flush and shut down an initialized exporter, or do nothing if none exists.
///
/// Call on a blocking thread while the Tokio runtime remains alive to drive
/// gRPC I/O. Shutdown waits at most five seconds for the batch processor.
///
/// # Errors
///
/// Returns an SDK error if shutdown fails or exceeds its deadline. Pending
/// traces can be lost on failure; the caller should report the error.
pub fn shutdown() -> Result<()> {
    #[cfg(feature = "telemetry")]
    otlp::shutdown()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbosity_saturates_at_trace() {
        assert_eq!(verbosity_level(0), Level::ERROR);
        assert_eq!(verbosity_level(1), Level::INFO);
        assert_eq!(verbosity_level(2), Level::DEBUG);
        assert_eq!(verbosity_level(3), Level::TRACE);
        assert_eq!(verbosity_level(u8::MAX), Level::TRACE);
    }
}
