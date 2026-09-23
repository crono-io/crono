//! Authorization-aware application boundary for control-plane use cases.
//!
//! HTTP and future schedulers enter through [`Application`]. It requests a
//! typed authorization decision before delegating to the persistence port, so
//! the pure domain remains unaware of principals while no transport can bypass
//! policy. PostgreSQL and NATS adapters live outside this module.

mod auth;
mod error;
mod model;
mod service;
mod store;

pub use auth::{
    AuthorizationError, Authorizer, Capability, DevelopmentIdentity, PermitAllAuthorizer,
    Principal, PrincipalKind, RequestContext, ResourceScope, VisibilityScope,
};
pub use error::ApplicationError;
pub use model::{
    CreateJobInput, CreateScheduleInput, JobRecord, Overview, Page, RunRecord, ScheduleRecord,
    TargetRecord, WorkerRecord,
};
pub use service::{Application, CreateRunOutcome};
pub use store::{
    ControlPlaneStore, JobDefinition, MetricsSnapshot, NewSchedule, OutboxRecord,
    PlannedOccurrence, SchedulePlan, StoreError,
};
