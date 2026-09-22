//! Human command-line client for the public Crono control-plane API.
//!
//! Argument parsing produces typed actions, actions own user-facing behavior,
//! and [`client::CronoClient`] is the only boundary intended to communicate
//! with `crono-server`. The initial shell validates connection configuration
//! but deliberately performs no requests until stable public API contracts
//! exist.

pub mod cli;
pub mod client;
pub mod config;

/// Metadata captured at compilation; Git fields may be absent in source archives.
pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
