//! Directly editable Job execution definitions.
//!
//! Crono's draft model does not expose Job versions. A scheduler or manual Run
//! copies these fields into the Run's immutable execution snapshot, so editing
//! a Job can affect future Runs without changing committed work.

use super::{JobId, NamespaceId, QueueId, ResourceName};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorKind {
    Noop,
    Process,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    id: JobId,
    namespace_id: NamespaceId,
    name: ResourceName,
    executor: ExecutorKind,
    queue_id: QueueId,
    executable: Option<String>,
    arguments: Vec<String>,
    inputs: serde_json::Value,
    idempotent: bool,
    max_attempts: u16,
    retry_initial_seconds: u32,
    retry_max_seconds: u32,
    retry_multiplier: f64,
    retry_jitter: f64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobData {
    pub id: JobId,
    pub namespace_id: NamespaceId,
    pub name: ResourceName,
    pub executor: ExecutorKind,
    pub queue_id: QueueId,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
    pub inputs: serde_json::Value,
    pub idempotent: bool,
    pub max_attempts: u16,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Job {
    #[must_use]
    pub fn new(data: JobData) -> Self {
        let JobData {
            id,
            namespace_id,
            name,
            executor,
            queue_id,
            executable,
            arguments,
            inputs,
            idempotent,
            max_attempts,
            retry_initial_seconds,
            retry_max_seconds,
            retry_multiplier,
            retry_jitter,
            created_at,
            updated_at,
        } = data;
        Self {
            id,
            namespace_id,
            name,
            executor,
            queue_id,
            executable,
            arguments,
            inputs,
            idempotent,
            max_attempts,
            retry_initial_seconds,
            retry_max_seconds,
            retry_multiplier,
            retry_jitter,
            created_at,
            updated_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> JobId {
        self.id
    }
    #[must_use]
    pub const fn namespace_id(&self) -> NamespaceId {
        self.namespace_id
    }
    #[must_use]
    pub const fn name(&self) -> &ResourceName {
        &self.name
    }
    #[must_use]
    pub const fn executor(&self) -> ExecutorKind {
        self.executor
    }
    #[must_use]
    pub const fn queue_id(&self) -> QueueId {
        self.queue_id
    }
    #[must_use]
    pub fn executable(&self) -> Option<&str> {
        self.executable.as_deref()
    }
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }
    #[must_use]
    pub const fn inputs(&self) -> &serde_json::Value {
        &self.inputs
    }
    #[must_use]
    pub const fn idempotent(&self) -> bool {
        self.idempotent
    }
    #[must_use]
    pub const fn max_attempts(&self) -> u16 {
        self.max_attempts
    }
    #[must_use]
    pub const fn retry_initial_seconds(&self) -> u32 {
        self.retry_initial_seconds
    }
    #[must_use]
    pub const fn retry_max_seconds(&self) -> u32 {
        self.retry_max_seconds
    }
    #[must_use]
    pub const fn retry_multiplier(&self) -> f64 {
        self.retry_multiplier
    }
    #[must_use]
    pub const fn retry_jitter(&self) -> f64 {
        self.retry_jitter
    }
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
    #[must_use]
    pub const fn updated_at(&self) -> OffsetDateTime {
        self.updated_at
    }
}
