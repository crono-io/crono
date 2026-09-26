//! External PostgreSQL and NATS adapters for the control-plane application.
//!
//! Startup constructs these adapters and injects their narrow ports into the
//! application layer. Domain and authorization code therefore remain
//! independent from SQL, `JetStream`, environment variables, and process
//! lifecycle concerns.

mod database;
mod dispatcher;
mod environment;
mod nats;
mod postgres;
mod worker_control;

pub use database::DatabasePoolConfig;
pub use dispatcher::{DispatcherConfig, run_dispatcher};
pub use nats::NatsPublisher;
pub use postgres::PostgresStore;
pub use worker_control::run_worker_control;
