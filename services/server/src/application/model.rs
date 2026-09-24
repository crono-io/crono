//! Application query records composed from pure domain entities.

use crate::domain::{
    CatchupPolicy, ExecutorKind, Job, JobId, MisfirePolicy, NamespaceName, Run, Schedule,
    ScheduleTiming, Target, TargetSet,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateQueueInput {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateQueueInput {
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateJobInput {
    pub name: String,
    pub queue_id: Uuid,
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
    pub inputs: serde_json::Value,
    pub idempotent: bool,
    pub max_attempts: u16,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateScheduleInput {
    pub name: String,
    pub job_id: JobId,
    pub target: crate::domain::TargetSelection,
    pub inputs: serde_json::Value,
    pub timing: ScheduleTiming,
    pub misfire_policy: MisfirePolicy,
    pub misfire_grace_seconds: Option<u32>,
    pub catchup_policy: CatchupPolicy,
    pub max_catchup_runs: u16,
    pub max_catchup_age_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobRecord {
    pub namespace: NamespaceName,
    pub queue_name: crate::domain::QueueName,
    pub job: Job,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRecord {
    pub namespace: NamespaceName,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSetRecord {
    pub namespace: NamespaceName,
    pub target_set: TargetSet,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run: Run,
    pub job_namespace: NamespaceName,
    pub job_name: crate::domain::ResourceName,
    pub target_namespace: NamespaceName,
    pub target_name: crate::domain::ResourceName,
}

/// Authoritative presence and current execution count for one worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerRecord {
    pub worker_id: String,
    pub queue_id: crate::domain::QueueId,
    pub queue: String,
    pub concurrency: u16,
    pub version: String,
    pub started_at: time::OffsetDateTime,
    pub last_seen_at: time::OffsetDateTime,
    pub active_executions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRecord {
    pub schedule: Schedule,
    pub namespace: NamespaceName,
    pub job_name: crate::domain::ResourceName,
    pub target_name: crate::domain::ResourceName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overview {
    pub namespaces: u64,
    pub jobs: u64,
    pub targets: u64,
    pub target_sets: u64,
    pub schedules: u64,
    pub runs: u64,
}
