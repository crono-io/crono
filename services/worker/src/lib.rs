//! Crono execution-worker application boundary.
//!
//! The CLI translates startup arguments into typed actions. Runtime behavior is
//! deliberately unfinished until the execution protocol is defined.

pub mod cli;

/// Metadata captured at compilation; Git fields may be absent in source archives.
pub mod built_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
