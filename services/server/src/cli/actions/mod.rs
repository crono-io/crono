//! Typed actions form the contract between CLI dispatch and execution.

pub mod server;

/// Operations selected by the command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the HTTP API server.
    Server(server::Args),
}
