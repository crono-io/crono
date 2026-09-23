//! Persistence ports for catalog, scheduling, dispatch, and worker state.
//!
//! Every method represents a short PostgreSQL transaction or query. Claims
//! carry database leases so multiple server instances can share work without
//! process-local coordination.

use super::{
    JobRecord, Overview, Page, RunRecord, ScheduleRecord, TargetRecord, VisibilityScope,
    WorkerRecord,
};
use crate::domain::{
    AttemptId, CatchupPolicy, DispatchId, ExecutorKind, MisfirePolicy, Namespace, NamespaceName,
    QueueName, ResourceName, RunId, Schedule, ScheduleId,
};
use async_trait::async_trait;
use crono_api::{
    ClaimRequest, ClaimResponse, CompletionRequest, LeaseRequest, WorkerHeartbeatRequest,
};
use std::{error::Error, fmt, time::Duration};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct JobDefinition {
    pub executor: ExecutorKind,
    pub queue: QueueName,
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
pub struct NewSchedule {
    pub namespace: NamespaceName,
    pub name: ResourceName,
    pub job_name: ResourceName,
    pub target_name: ResourceName,
    pub cron_expression: Option<String>,
    pub execute_at: Option<OffsetDateTime>,
    pub timezone: String,
    pub next_run_at: OffsetDateTime,
    pub misfire_policy: MisfirePolicy,
    pub misfire_grace_seconds: Option<u32>,
    pub catchup_policy: CatchupPolicy,
    pub max_catchup_runs: u16,
    pub max_catchup_age_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedOccurrence {
    pub scheduled_at: OffsetDateTime,
    pub execute: bool,
    pub lateness_seconds: u64,
    pub reason: Option<String>,
    pub dispatch_deadline: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePlan {
    pub schedule_id: ScheduleId,
    pub owner: Uuid,
    pub occurrences: Vec<PlannedOccurrence>,
    pub next_run_at: Option<OffsetDateTime>,
    pub disable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRecord {
    pub id: DispatchId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub subject: String,
    pub payload: Vec<u8>,
    pub attempt_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub outbox_pending: i64,
    pub outbox_oldest_seconds: i64,
    pub execution_queued: i64,
    pub execution_running: i64,
    pub worker_active: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    NotFound,
    Conflict,
    IdempotencyConflict,
    StaleRevision,
    Unavailable,
    Internal,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "resource was not found",
            Self::Conflict => "resource already exists",
            Self::IdempotencyConflict => "idempotency key conflicts with an existing request",
            Self::StaleRevision => "resource revision is stale",
            Self::Unavailable => "persistence is unavailable",
            Self::Internal => "persistence failed",
        })
    }
}

impl Error for StoreError {}

#[async_trait]
pub trait ControlPlaneStore: Send + Sync {
    async fn create_namespace(&self, name: &NamespaceName) -> Result<Namespace, StoreError>;
    async fn list_namespaces(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<Namespace>, StoreError>;
    async fn get_namespace(&self, name: &NamespaceName) -> Result<Namespace, StoreError>;
    async fn create_job(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
        definition: &JobDefinition,
    ) -> Result<JobRecord, StoreError>;
    async fn list_jobs(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, StoreError>;
    async fn get_job(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<JobRecord, StoreError>;
    async fn create_target(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
        arguments: &[String],
    ) -> Result<TargetRecord, StoreError>;
    async fn list_targets(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, StoreError>;
    async fn get_target(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<TargetRecord, StoreError>;
    async fn create_schedule(&self, schedule: &NewSchedule) -> Result<ScheduleRecord, StoreError>;
    async fn list_schedules(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<ScheduleRecord>, StoreError>;
    async fn get_schedule(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<ScheduleRecord, StoreError>;
    async fn set_schedule_enabled(
        &self,
        id: ScheduleId,
        revision: u64,
        enabled: bool,
        next_run_at: Option<OffsetDateTime>,
    ) -> Result<ScheduleRecord, StoreError>;
    async fn create_run(
        &self,
        request_id: Uuid,
        job_namespace: &NamespaceName,
        job_name: &ResourceName,
        target_namespace: &NamespaceName,
        target_name: &ResourceName,
    ) -> Result<(RunRecord, bool), StoreError>;
    async fn list_runs(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, StoreError>;
    async fn get_run(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<RunRecord, StoreError>;
    async fn record_worker_heartbeat(
        &self,
        request: &WorkerHeartbeatRequest,
    ) -> Result<(), StoreError>;
    async fn list_workers(
        &self,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<WorkerRecord>, StoreError>;
    async fn overview(&self, visibility: &VisibilityScope) -> Result<Overview, StoreError>;

    async fn claim_due_schedules(
        &self,
        owner: Uuid,
        limit: u16,
        lease: Duration,
    ) -> Result<Vec<Schedule>, StoreError>;
    async fn commit_schedule_plan(&self, plan: &SchedulePlan) -> Result<(), StoreError>;
    async fn claim_outbox(
        &self,
        owner: Uuid,
        limit: u16,
        lease: Duration,
    ) -> Result<Vec<OutboxRecord>, StoreError>;
    async fn mark_published(
        &self,
        owner: Uuid,
        record: &OutboxRecord,
        stream_sequence: u64,
    ) -> Result<(), StoreError>;
    async fn record_publish_failure(
        &self,
        owner: Uuid,
        dispatch_id: DispatchId,
        message: &str,
        next_attempt_at: OffsetDateTime,
    ) -> Result<(), StoreError>;

    async fn claim_attempt(&self, request: &ClaimRequest) -> Result<ClaimResponse, StoreError>;
    async fn renew_lease(&self, request: &LeaseRequest) -> Result<bool, StoreError>;
    async fn complete_attempt(&self, request: &CompletionRequest) -> Result<bool, StoreError>;
    async fn reconcile(&self, limit: u16) -> Result<u64, StoreError>;
    async fn metrics_snapshot(&self) -> Result<MetricsSnapshot, StoreError>;
    async fn ready(&self) -> bool;
}
