//! Durable Schedule configuration and policy types.
//!
//! Occurrence calculation is pure and lives in the scheduler module. This
//! entity stores the persisted inputs and cursor used by that calculation.

use super::{JobId, NamespaceId, ResourceName, ScheduleId, TargetId, TargetSetId};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisfirePolicy {
    RunLate,
    Skip,
    GracePeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchupPolicy {
    Skip,
    RunOnce,
    CatchUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleTiming {
    Cron {
        expression: String,
        timezone: String,
    },
    Once {
        execute_at: OffsetDateTime,
    },
}

/// Explicit single-Target or Target Set execution selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSelection {
    Target(TargetId),
    TargetSet(TargetSetId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    pub id: ScheduleId,
    pub namespace_id: NamespaceId,
    pub name: ResourceName,
    pub job_id: JobId,
    pub target: TargetSelection,
    pub inputs: serde_json::Value,
    pub timing: ScheduleTiming,
    pub enabled: bool,
    pub next_run_at: Option<OffsetDateTime>,
    pub last_run_at: Option<OffsetDateTime>,
    pub misfire_policy: MisfirePolicy,
    pub misfire_grace_seconds: Option<u32>,
    pub catchup_policy: CatchupPolicy,
    pub max_catchup_runs: u16,
    pub max_catchup_age_seconds: u32,
    pub revision: u64,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
