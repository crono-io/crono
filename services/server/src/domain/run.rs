//! Run identity and the initial durable dispatch state machine.
//!
//! A Run pins immutable execution inputs before an outbox message is visible.
//! This slice stops after `JetStream` confirms dispatch; claims, leases, worker
//! execution, and terminal outcomes remain separate future transitions.

use super::{JobVersionId, RunId, TargetId};
use time::OffsetDateTime;
use uuid::Uuid;

/// Durable state implemented by the initial dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    PendingDispatch,
    Dispatched,
}

/// One idempotently requested execution awaiting or completing dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    id: RunId,
    request_id: Uuid,
    job_version_id: JobVersionId,
    target_id: TargetId,
    status: RunStatus,
    created_at: OffsetDateTime,
    dispatched_at: Option<OffsetDateTime>,
}

impl Run {
    #[must_use]
    pub const fn new(
        id: RunId,
        request_id: Uuid,
        job_version_id: JobVersionId,
        target_id: TargetId,
        status: RunStatus,
        created_at: OffsetDateTime,
        dispatched_at: Option<OffsetDateTime>,
    ) -> Self {
        Self {
            id,
            request_id,
            job_version_id,
            target_id,
            status,
            created_at,
            dispatched_at,
        }
    }
    #[must_use]
    pub const fn id(&self) -> RunId {
        self.id
    }
    #[must_use]
    pub const fn request_id(&self) -> Uuid {
        self.request_id
    }
    #[must_use]
    pub const fn job_version_id(&self) -> JobVersionId {
        self.job_version_id
    }
    #[must_use]
    pub const fn target_id(&self) -> TargetId {
        self.target_id
    }
    #[must_use]
    pub const fn status(&self) -> RunStatus {
        self.status
    }
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
    #[must_use]
    pub const fn dispatched_at(&self) -> Option<OffsetDateTime> {
        self.dispatched_at
    }
}
