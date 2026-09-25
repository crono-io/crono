//! Persistence ports for catalog, scheduling, dispatch, and worker state.
//!
//! Every method represents a short PostgreSQL transaction or query. Claims
//! carry database leases so multiple server instances can share work without
//! process-local coordination.

use super::{
    JobRecord, Overview, Page, RunAttemptRecord, RunEventRecord, RunRecord, ScheduleRecord,
    TargetRecord, TargetSetRecord, VisibilityScope, WorkerRecord,
};
use crate::domain::{
    AttemptId, CatchupPolicy, DispatchId, ExecutorKind, JobId, MisfirePolicy, Namespace,
    NamespaceId, NamespaceName, Queue, QueueId, QueueName, ResourceName, RunId, RunStatus,
    Schedule, ScheduleId, TargetId, TargetSelection, TargetSetId,
};
use async_trait::async_trait;
use crono_api::{
    ClaimRequest, ClaimResponse, CompletionRequest, LeaseRequest, OutputSnapshotRequest,
    WorkerHeartbeatRequest,
};
use std::{error::Error, fmt, time::Duration};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct JobDefinition {
    pub executor: ExecutorKind,
    pub queue_id: QueueId,
    pub executable: Option<String>,
    pub shell_command: Option<String>,
    pub arguments: Vec<String>,
    pub inputs: serde_json::Value,
    pub idempotent: bool,
    pub dry_run: bool,
    pub max_attempts: u16,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
}

/// Server-side history filters applied after Run visibility and before pagination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunListFilter {
    pub namespace_id: Option<NamespaceId>,
    pub status: Option<RunStatus>,
    pub job_id: Option<JobId>,
    pub target_id: Option<TargetId>,
    pub target_set_id: Option<TargetSetId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDefinition {
    pub arguments: Vec<String>,
    pub inputs: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSchedule {
    pub namespace_id: NamespaceId,
    pub name: ResourceName,
    pub job_id: JobId,
    pub target: TargetSelection,
    pub inputs: serde_json::Value,
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

/// System-wide PostgreSQL and execution state sampled for the operator monitor.
/// Pool values belong only to the API instance that served the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorSnapshot {
    pub database_size_bytes: i64,
    pub database_connections: i64,
    pub pool_connections: u32,
    pub pool_idle_connections: u32,
    pub pool_max_connections: u32,
    pub enabled_schedules: i64,
    pub due_schedules: i64,
    pub earliest_next_run_at: Option<OffsetDateTime>,
    pub online_workers: i64,
    pub metrics: MetricsSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    NotFound,
    Conflict,
    InUse,
    IdempotencyConflict,
    StaleRevision,
    QueueDisabled,
    Unavailable,
    Internal,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "resource was not found",
            Self::Conflict => "resource already exists",
            Self::InUse => "resource is still in use",
            Self::IdempotencyConflict => "idempotency key conflicts with an existing request",
            Self::StaleRevision => "resource revision is stale",
            Self::QueueDisabled => "the original Run's Queue is disabled",
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
    async fn get_namespace(&self, id: NamespaceId) -> Result<Namespace, StoreError>;
    async fn create_queue(
        &self,
        name: &QueueName,
        description: Option<&str>,
    ) -> Result<Queue, StoreError>;
    async fn list_queues(&self, limit: u16, after: Option<&str>)
    -> Result<Page<Queue>, StoreError>;
    async fn get_queue(&self, id: QueueId) -> Result<Queue, StoreError>;
    async fn get_queue_by_name(&self, name: &QueueName) -> Result<Queue, StoreError>;
    async fn update_queue(
        &self,
        id: QueueId,
        name: &QueueName,
        description: Option<&str>,
        enabled: bool,
    ) -> Result<Queue, StoreError>;
    async fn delete_queue(&self, id: QueueId) -> Result<(), StoreError>;
    async fn create_job(
        &self,
        namespace_id: NamespaceId,
        name: &ResourceName,
        definition: &JobDefinition,
    ) -> Result<JobRecord, StoreError>;
    async fn list_jobs(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, StoreError>;
    async fn get_job(&self, id: JobId) -> Result<JobRecord, StoreError>;
    async fn update_job(
        &self,
        id: JobId,
        name: &ResourceName,
        definition: &JobDefinition,
    ) -> Result<JobRecord, StoreError>;
    async fn create_target(
        &self,
        namespace_id: NamespaceId,
        name: &ResourceName,
        definition: &TargetDefinition,
    ) -> Result<TargetRecord, StoreError>;
    async fn list_targets(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, StoreError>;
    async fn get_target(&self, id: TargetId) -> Result<TargetRecord, StoreError>;
    async fn update_target(
        &self,
        id: TargetId,
        name: &ResourceName,
        definition: &TargetDefinition,
    ) -> Result<TargetRecord, StoreError>;
    async fn create_target_set(
        &self,
        namespace_id: NamespaceId,
        name: &ResourceName,
        target_ids: &[TargetId],
        inputs: &serde_json::Value,
    ) -> Result<TargetSetRecord, StoreError>;
    async fn list_target_sets(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetSetRecord>, StoreError>;
    async fn get_target_set(&self, id: TargetSetId) -> Result<TargetSetRecord, StoreError>;
    async fn update_target_set(
        &self,
        id: TargetSetId,
        name: &ResourceName,
        target_ids: &[TargetId],
        inputs: &serde_json::Value,
    ) -> Result<TargetSetRecord, StoreError>;
    async fn create_schedule(&self, schedule: &NewSchedule) -> Result<ScheduleRecord, StoreError>;
    async fn list_schedules(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<ScheduleRecord>, StoreError>;
    async fn get_schedule(&self, id: ScheduleId) -> Result<ScheduleRecord, StoreError>;
    async fn set_schedule_enabled(
        &self,
        id: ScheduleId,
        revision: u64,
        enabled: bool,
        next_run_at: Option<OffsetDateTime>,
    ) -> Result<ScheduleRecord, StoreError>;
    async fn create_runs(
        &self,
        request_id: Uuid,
        job_id: JobId,
        target: TargetSelection,
        inputs: &serde_json::Value,
    ) -> Result<(Vec<RunRecord>, bool), StoreError>;
    /// Create one direct-target Run through the batch-aware persistence path.
    ///
    /// This compatibility boundary keeps internal callers concise while the
    /// public API always returns a batch, including single-target requests.
    async fn create_run(
        &self,
        request_id: Uuid,
        job_id: JobId,
        target_id: TargetId,
    ) -> Result<(RunRecord, bool), StoreError> {
        let (runs, created) = self
            .create_runs(
                request_id,
                job_id,
                TargetSelection::Target(target_id),
                &serde_json::json!({}),
            )
            .await?;
        let run = runs.into_iter().next().ok_or(StoreError::Internal)?;
        Ok((run, created))
    }
    async fn list_runs(
        &self,
        visibility: &VisibilityScope,
        filter: RunListFilter,
        limit: u16,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, StoreError>;
    /// Copy one immutable Run snapshot into a new single-Target invocation.
    async fn rerun_run(
        &self,
        source_id: RunId,
        request_id: Uuid,
    ) -> Result<(RunRecord, bool), StoreError>;
    async fn get_run(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<RunRecord, StoreError>;
    async fn list_run_attempts(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<Vec<RunAttemptRecord>, StoreError>;
    async fn list_run_events(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<Vec<RunEventRecord>, StoreError>;
    async fn record_worker_heartbeat(
        &self,
        request: &WorkerHeartbeatRequest,
    ) -> Result<(), StoreError>;
    async fn record_attempt_output(
        &self,
        request: &OutputSnapshotRequest,
    ) -> Result<bool, StoreError>;
    async fn list_workers(
        &self,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<WorkerRecord>, StoreError>;
    async fn get_worker(&self, worker_id: &str) -> Result<WorkerRecord, StoreError>;
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
    async fn monitor_snapshot(&self) -> Result<MonitorSnapshot, StoreError>;
    async fn ready(&self) -> bool;
}
