//! Immutable executable definition belonging to a Job.
//!
//! The first vertical slice supports only a typed no-op executor. This makes a
//! Run honestly dispatchable without accepting arbitrary commands before the
//! worker security and execution contracts exist.

use super::{JobId, JobVersionId, QueueName};
use time::OffsetDateTime;

/// Executor contract pinned by one immutable Job version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorKind {
    /// Dispatch-only definition that performs no worker operation.
    Noop,
}

/// Immutable version of a Job definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobVersion {
    id: JobVersionId,
    job_id: JobId,
    number: u32,
    executor: ExecutorKind,
    queue: QueueName,
    created_at: OffsetDateTime,
}

impl JobVersion {
    /// Construct a persisted immutable Job version.
    #[must_use]
    pub const fn new(
        id: JobVersionId,
        job_id: JobId,
        number: u32,
        executor: ExecutorKind,
        queue: QueueName,
        created_at: OffsetDateTime,
    ) -> Self {
        Self {
            id,
            job_id,
            number,
            executor,
            queue,
            created_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> JobVersionId {
        self.id
    }
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }
    #[must_use]
    pub const fn executor(&self) -> ExecutorKind {
        self.executor
    }
    #[must_use]
    pub const fn queue(&self) -> &QueueName {
        &self.queue
    }
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
}
