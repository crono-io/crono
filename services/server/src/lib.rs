//! Crono's authoritative control-plane boundary.
//!
//! The HTTP API accepts external control-plane requests, while the pure domain
//! layer defines workload identities and relationships without depending on
//! transports, persistence, workers, or executor-specific concepts. CLI
//! startup remains a separate adapter around this application boundary.

pub mod api;
pub mod application;
pub mod cli;
pub mod domain;
pub mod infrastructure;

/// Metadata captured at compilation; Git fields may be absent in source archives.
pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
