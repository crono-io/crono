//! Parse arguments, initialize logging, and dispatch to typed actions.
//!
//! Command definitions contain no runtime logic. The binary owns execution and
//! telemetry shutdown so error paths receive the same cleanup as success paths.

pub mod actions;
pub mod commands;
pub mod dispatch;
pub mod telemetry;

mod start;
pub use start::start;
