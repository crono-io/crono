//! Transport contracts shared by Crono clients, the server, and workers.
//!
//! HTTP resources are intentionally unversioned while Crono remains a draft.
//! `JetStream` messages contain stable identifiers and immutable execution data
//! remains in PostgreSQL, allowing the transport to be rebuilt safely.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One bounded page of public API resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateNamespaceRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NamespaceResource {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
}

/// Executor selected by a Job and copied into every Run snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ExecutorKind {
    #[default]
    Noop,
    Process,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateJobRequest {
    pub name: String,
    #[serde(default = "default_queue")]
    pub queue: String,
    #[serde(default)]
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub idempotent: bool,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u16,
    #[serde(default = "default_retry_initial_seconds")]
    pub retry_initial_seconds: u32,
    #[serde(default = "default_retry_max_seconds")]
    pub retry_max_seconds: u32,
    #[serde(default = "default_retry_multiplier")]
    pub retry_multiplier: f64,
    #[serde(default = "default_retry_jitter")]
    pub retry_jitter: f64,
}

fn default_queue() -> String {
    "default".to_string()
}

const fn default_max_attempts() -> u16 {
    1
}

const fn default_retry_initial_seconds() -> u32 {
    1
}

const fn default_retry_max_seconds() -> u32 {
    60
}

const fn default_retry_multiplier() -> f64 {
    2.0
}

const fn default_retry_jitter() -> f64 {
    0.2
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct JobResource {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    pub executor: ExecutorKind,
    pub queue: String,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
    pub idempotent: bool,
    pub max_attempts: u16,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateTargetRequest {
    pub name: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TargetResource {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    pub arguments: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MisfirePolicy {
    RunLate,
    Skip,
    GracePeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum CatchupPolicy {
    Skip,
    #[default]
    RunOnce,
    CatchUp,
}

/// Request for either a cron schedule or a one-shot UTC execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateScheduleRequest {
    pub name: String,
    pub job: String,
    pub target: String,
    pub cron_expression: Option<String>,
    pub execute_at: Option<String>,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    pub misfire_policy: MisfirePolicy,
    pub misfire_grace_seconds: Option<u32>,
    #[serde(default)]
    pub catchup_policy: CatchupPolicy,
    #[serde(default = "default_catchup_runs")]
    pub max_catchup_runs: u16,
    #[serde(default = "default_catchup_age")]
    pub max_catchup_age_seconds: u32,
}

fn default_timezone() -> String {
    "UTC".to_string()
}

const fn default_catchup_runs() -> u16 {
    100
}

const fn default_catchup_age() -> u32 {
    86_400
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ScheduleResource {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub job: String,
    pub target: String,
    pub cron_expression: Option<String>,
    pub execute_at: Option<String>,
    pub timezone: String,
    pub enabled: bool,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub misfire_policy: MisfirePolicy,
    pub misfire_grace_seconds: Option<u32>,
    pub catchup_policy: CatchupPolicy,
    pub max_catchup_runs: u16,
    pub max_catchup_age_seconds: u32,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateScheduleRequest {
    pub revision: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateRunRequest {
    pub request_id: Uuid,
    pub job: String,
    pub target: String,
}

/// Durable logical execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RunResource {
    pub id: Uuid,
    pub request_id: Option<Uuid>,
    pub schedule_id: Option<Uuid>,
    pub job: String,
    pub target: String,
    pub status: RunStatus,
    pub scheduled_at: String,
    pub created_at: String,
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub attempt_count: u16,
    pub max_attempts: u16,
    pub lateness_seconds: u64,
    pub terminal_reason: Option<String>,
}

/// Server-derived liveness of a worker's presence heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum WorkerStatus {
    Online,
    Stale,
    Offline,
}

/// Public worker presence without NATS credentials or execution payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkerResource {
    pub worker_id: String,
    pub queue: String,
    pub concurrency: u16,
    pub version: String,
    pub status: WorkerStatus,
    pub started_at: String,
    pub last_seen_at: String,
    pub active_executions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OverviewResource {
    pub namespaces: u64,
    pub jobs: u64,
    pub targets: u64,
    pub schedules: u64,
    pub runs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// Minimal durable dispatch message; PostgreSQL owns all execution details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchEnvelope {
    pub dispatch_id: Uuid,
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub queue: String,
}

/// Immutable executable configuration returned only after a successful claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
    pub inputs: serde_json::Value,
    pub idempotency_key: Uuid,
    pub queue: String,
    pub idempotent: bool,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRequest {
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub worker_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimResponse {
    pub claimed: bool,
    pub lease_seconds: u32,
    pub execution: Option<ExecutionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub attempt_id: Uuid,
    pub worker_id: String,
}

/// Presence metadata refreshed by one worker process session.
///
/// The session identifier lets the server distinguish a restarted process that
/// deliberately reuses a stable worker ID from another heartbeat in the same
/// process lifetime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerHeartbeatRequest {
    pub worker_id: String,
    pub session_id: Uuid,
    pub queue: String,
    pub concurrency: u16,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub attempt_id: Uuid,
    pub worker_id: String,
    pub succeeded: bool,
    pub exit_code: Option<i32>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub error: Option<String>,
}
