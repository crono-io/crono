//! Typed actions form the contract between CLI dispatch and execution.

pub mod run;

/// Operations selected by the command line.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the bounded `JetStream` execution worker.
    Run(run::Args),
}
