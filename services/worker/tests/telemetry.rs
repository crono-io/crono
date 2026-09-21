//! Test logging and trace export in an isolated service process.

const BINARY: &str = env!("CARGO_BIN_EXE_crono-worker");
#[cfg(feature = "telemetry")]
const SERVICE: &str = "crono-worker";
#[cfg(feature = "telemetry")]
const SPAN: &str = "worker.run";
#[cfg(feature = "telemetry")]
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[path = "../../../tests/support/telemetry.rs"]
mod checks;
