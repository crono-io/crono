//! PostgreSQL-backed scheduling calculations and planner loop.
//!
//! Recurrence and policy decisions are pure and testable. The planner adapter
//! claims bounded database batches; no Tokio task sleeps for an individual
//! Schedule.

mod misfire;
mod planner;
mod recurrence;

pub use misfire::{MisfireDecision, decide_misfire};
pub use planner::run_scheduler;
pub use recurrence::{RecurrenceError, next_cron_occurrence, validate_cron};
