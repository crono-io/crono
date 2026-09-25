//! Durable logical execution state.
//!
//! Run state is authoritative in PostgreSQL. `JetStream` may redeliver any
//! attempt, so transitions are conditional and terminal Runs are immutable.

use super::{JobId, QueueId, RunId, ScheduleId, TargetId};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    PendingDispatch,
    Queued,
    Running,
    RetryWait,
    Succeeded,
    Failed,
    Dead,
    Skipped,
    Cancelled,
    Unknown,
}

impl RunStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::Dead
                | Self::Skipped
                | Self::Cancelled
                | Self::Unknown
        )
    }

    /// Repeat only outcomes whose original execution is no longer uncertain.
    /// An `Unknown` Run may still have run despite lost completion reporting.
    #[must_use]
    pub const fn is_repeatable(self) -> bool {
        self.is_terminal() && !matches!(self, Self::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::RunStatus;

    #[test]
    fn unknown_outcome_is_terminal_but_not_safe_to_repeat() {
        assert!(RunStatus::Unknown.is_terminal());
        assert!(!RunStatus::Unknown.is_repeatable());
        assert!(!RunStatus::Running.is_repeatable());
        assert!(RunStatus::Succeeded.is_repeatable());
        assert!(RunStatus::Failed.is_repeatable());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    id: RunId,
    request_id: Option<Uuid>,
    rerun_of_run_id: Option<RunId>,
    schedule_id: Option<ScheduleId>,
    job_id: JobId,
    target_id: TargetId,
    queue_id: QueueId,
    status: RunStatus,
    scheduled_at: OffsetDateTime,
    created_at: OffsetDateTime,
    queued_at: Option<OffsetDateTime>,
    started_at: Option<OffsetDateTime>,
    completed_at: Option<OffsetDateTime>,
    attempt_count: u16,
    max_attempts: u16,
    lateness_seconds: u64,
    terminal_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunData {
    pub id: RunId,
    pub request_id: Option<Uuid>,
    pub rerun_of_run_id: Option<RunId>,
    pub schedule_id: Option<ScheduleId>,
    pub job_id: JobId,
    pub target_id: TargetId,
    pub queue_id: QueueId,
    pub status: RunStatus,
    pub scheduled_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub queued_at: Option<OffsetDateTime>,
    pub started_at: Option<OffsetDateTime>,
    pub completed_at: Option<OffsetDateTime>,
    pub attempt_count: u16,
    pub max_attempts: u16,
    pub lateness_seconds: u64,
    pub terminal_reason: Option<String>,
}

impl Run {
    #[must_use]
    pub fn new(data: RunData) -> Self {
        let RunData {
            id,
            request_id,
            rerun_of_run_id,
            schedule_id,
            job_id,
            target_id,
            queue_id,
            status,
            scheduled_at,
            created_at,
            queued_at,
            started_at,
            completed_at,
            attempt_count,
            max_attempts,
            lateness_seconds,
            terminal_reason,
        } = data;
        Self {
            id,
            request_id,
            rerun_of_run_id,
            schedule_id,
            job_id,
            target_id,
            queue_id,
            status,
            scheduled_at,
            created_at,
            queued_at,
            started_at,
            completed_at,
            attempt_count,
            max_attempts,
            lateness_seconds,
            terminal_reason,
        }
    }

    #[must_use]
    pub const fn id(&self) -> RunId {
        self.id
    }
    #[must_use]
    pub const fn request_id(&self) -> Option<Uuid> {
        self.request_id
    }
    #[must_use]
    pub const fn rerun_of_run_id(&self) -> Option<RunId> {
        self.rerun_of_run_id
    }
    #[must_use]
    pub const fn schedule_id(&self) -> Option<ScheduleId> {
        self.schedule_id
    }
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }
    #[must_use]
    pub const fn target_id(&self) -> TargetId {
        self.target_id
    }
    #[must_use]
    pub const fn queue_id(&self) -> QueueId {
        self.queue_id
    }
    #[must_use]
    pub const fn status(&self) -> RunStatus {
        self.status
    }
    #[must_use]
    pub const fn scheduled_at(&self) -> OffsetDateTime {
        self.scheduled_at
    }
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
    #[must_use]
    pub const fn queued_at(&self) -> Option<OffsetDateTime> {
        self.queued_at
    }
    #[must_use]
    pub const fn started_at(&self) -> Option<OffsetDateTime> {
        self.started_at
    }
    #[must_use]
    pub const fn completed_at(&self) -> Option<OffsetDateTime> {
        self.completed_at
    }
    #[must_use]
    pub const fn attempt_count(&self) -> u16 {
        self.attempt_count
    }
    #[must_use]
    pub const fn max_attempts(&self) -> u16 {
        self.max_attempts
    }
    #[must_use]
    pub const fn lateness_seconds(&self) -> u64 {
        self.lateness_seconds
    }
    #[must_use]
    pub fn terminal_reason(&self) -> Option<&str> {
        self.terminal_reason.as_deref()
    }
}
