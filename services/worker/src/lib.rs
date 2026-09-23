//! Crono execution-worker application boundary.
//!
//! The CLI translates startup arguments into a bounded `JetStream` pull worker.
//! PostgreSQL remains behind the server-mediated claim protocol, so workers
//! hold no database credentials.

pub mod cli;

/// Metadata captured at compilation; Git fields may be absent in source archives.
pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
