//! Application query records composed from pure domain entities.

use crate::domain::{
    CatchupPolicy, ExecutorKind, Job, MisfirePolicy, NamespaceName, Run, Schedule, ScheduleTiming,
    Target,
};

#[derive(Debug, Clone, PartialEq)]
pub struct CreateJobInput {
    pub name: String,
    pub queue: String,
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
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
    pub job: String,
    pub target: String,
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
    pub job: Job,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRecord {
    pub namespace: NamespaceName,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run: Run,
    pub job_namespace: NamespaceName,
    pub job_name: crate::domain::ResourceName,
    pub target_namespace: NamespaceName,
    pub target_name: crate::domain::ResourceName,
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
    pub schedules: u64,
    pub runs: u64,
}
