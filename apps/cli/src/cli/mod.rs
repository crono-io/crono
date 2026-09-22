//! Parse human-facing arguments and dispatch them through the client boundary.
//!
//! Command definitions contain presentation and syntax only. Dispatch resolves
//! configuration into a typed action, while action execution owns behavior and
//! delegates future network operations to `CronoClient`.

pub mod actions;
pub mod commands;
pub mod dispatch;

mod start;
pub use start::start;
