//! Transport contracts shared by Crono clients, the server, and workers.
//!
//! HTTP resources are intentionally unversioned while Crono remains a draft.
//! `JetStream` messages contain stable identifiers and immutable execution data
//! remains in PostgreSQL, allowing the transport to be rebuilt safely.

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};
use uuid::Uuid;

/// Maximum byte length of a canonical DNS-1123 resource label.
pub const RESOURCE_NAME_MAX_LENGTH: usize = 63;

/// Why a canonical Namespace or resource name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceNameError {
    Empty,
    InvalidBoundary,
    InvalidCharacter { position: usize, character: char },
    TooLong,
}

impl fmt::Display for ResourceNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("name must contain at least one character"),
            Self::InvalidBoundary => {
                formatter.write_str("name must start and end with a lowercase letter or number")
            }
            Self::InvalidCharacter {
                position,
                character,
            } => write!(
                formatter,
                "name contains invalid character {character:?} at byte {position}; use lowercase letters, numbers, and '-'"
            ),
            Self::TooLong => write!(
                formatter,
                "name must not exceed {RESOURCE_NAME_MAX_LENGTH} characters"
            ),
        }
    }
}

impl Error for ResourceNameError {}

/// Validate one canonical DNS-1123 label without changing the supplied value.
///
/// # Errors
///
/// Rejects empty or oversized values, non-ASCII characters, characters other
/// than lowercase letters, digits, and `-`, and labels with `-` boundaries.
pub fn validate_resource_name(value: &str) -> Result<(), ResourceNameError> {
    if value.len() > RESOURCE_NAME_MAX_LENGTH {
        return Err(ResourceNameError::TooLong);
    }
    let mut characters = value.char_indices();
    let Some((_, first)) = characters.next() else {
        return Err(ResourceNameError::Empty);
    };
    if !is_name_boundary(first) {
        return Err(ResourceNameError::InvalidBoundary);
    }

    let mut last = first;
    for (position, character) in characters {
        if !is_name_character(character) {
            return Err(ResourceNameError::InvalidCharacter {
                position,
                character,
            });
        }
        last = character;
    }
    if !is_name_boundary(last) {
        return Err(ResourceNameError::InvalidBoundary);
    }
    Ok(())
}

const fn is_name_boundary(character: char) -> bool {
    character.is_ascii_lowercase() || character.is_ascii_digit()
}

const fn is_name_character(character: char) -> bool {
    is_name_boundary(character) || character == '-'
}

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
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NamespaceResource {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
}

/// Create a global worker Queue used by Jobs and worker processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateQueueRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub description: Option<String>,
}

/// Replace the editable metadata of an existing Queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateQueueRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
}

/// Public Queue metadata; the UUID remains authoritative across renames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QueueResource {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub system: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Executor selected by a Job and copied into every Run snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ExecutorKind {
    #[default]
    Noop,
    Process,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateJobRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub queue_id: Uuid,
    #[serde(default)]
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    /// Literal shell source; input templates belong in positional arguments.
    #[serde(default)]
    pub shell_command: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default = "default_inputs")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
    #[serde(default)]
    pub idempotent: bool,
    /// Preview future Runs without starting the configured executable.
    #[serde(default)]
    pub dry_run: bool,
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

/// Replace an existing Job definition while preserving its UUID and Namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateJobRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub queue_id: Uuid,
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    #[serde(default)]
    pub shell_command: Option<String>,
    pub arguments: Vec<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
    pub idempotent: bool,
    #[serde(default)]
    pub dry_run: bool,
    pub max_attempts: u16,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
}

fn default_inputs() -> serde_json::Value {
    serde_json::json!({})
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
    pub namespace_id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    pub executor: ExecutorKind,
    pub queue_id: Uuid,
    pub queue: String,
    pub executable: Option<String>,
    #[serde(default)]
    pub shell_command: Option<String>,
    pub arguments: Vec<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
    pub idempotent: bool,
    pub dry_run: bool,
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
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default = "default_inputs")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
}

/// Replace editable Target metadata while preserving identity and Namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateTargetRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub arguments: Vec<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TargetResource {
    pub id: Uuid,
    pub namespace_id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    pub arguments: Vec<String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

/// Create a named, explicit selection of Targets in one Namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateTargetSetRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub target_ids: Vec<Uuid>,
    #[serde(default = "default_inputs")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
}

/// Replace a Target Set's name, explicit membership, and shared inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateTargetSetRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub target_ids: Vec<Uuid>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
}

/// Display metadata for one Target selected by a Target Set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TargetReference {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TargetSetResource {
    pub id: Uuid,
    pub namespace_id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    pub targets: Vec<TargetReference>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

/// One explicit execution destination selected by immutable UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ExecutionTarget {
    Target { id: Uuid },
    TargetSet { id: Uuid },
}

/// Display form of an execution destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ExecutionTargetResource {
    Target { id: Uuid, name: String },
    TargetSet { id: Uuid, name: String },
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
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub job_id: Uuid,
    pub target: ExecutionTarget,
    #[serde(default = "default_inputs")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
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
    pub namespace_id: Uuid,
    pub namespace: String,
    pub name: String,
    pub job_id: Uuid,
    pub job: String,
    pub target: ExecutionTargetResource,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
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
    pub job_id: Uuid,
    pub target: ExecutionTarget,
    #[serde(default = "default_inputs")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
}

/// Idempotent request to repeat one historical Run's immutable execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RerunRequest {
    pub request_id: Uuid,
}

/// Server-established origin; a manual HTTP request cannot claim to be a CLI user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RunTriggerSource {
    #[default]
    Unknown,
    Api,
    Scheduler,
    Rerun,
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
    pub job_id: Uuid,
    pub job: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub job_name: String,
    pub target_id: Uuid,
    pub target: String,
    #[serde(default)]
    pub target_name: String,
    /// Original Target Set label when this Run was one member of a batch.
    pub target_set: Option<String>,
    #[serde(default)]
    pub queue: String,
    #[serde(default)]
    pub trigger_source: RunTriggerSource,
    /// Reserved for a future verified identity; never inferred from client input.
    pub trigger_actor: Option<String>,
    pub rerun_of_run_id: Option<Uuid>,
    /// Whether a terminal Run has a stored executable snapshot to repeat.
    #[serde(default)]
    pub rerunnable: bool,
    pub status: RunStatus,
    pub scheduled_at: String,
    pub created_at: String,
    /// Time the Run record was materialized; scheduled occurrence stays separate.
    #[serde(default)]
    pub triggered_at: String,
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// Database-observed elapsed time from Run start to completion.
    pub duration_ms: Option<u64>,
    pub attempt_count: u16,
    pub max_attempts: u16,
    pub lateness_seconds: u64,
    pub terminal_reason: Option<String>,
}

/// One durable server-side lifecycle event, without execution payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RunEventResource {
    pub event_type: String,
    pub created_at: String,
}

#[cfg(test)]
mod run_resource_compatibility_tests {
    use super::{RunResource, RunTriggerSource};

    #[test]
    fn older_server_run_response_remains_readable_without_new_metadata()
    -> Result<(), serde_json::Error> {
        let id = uuid::Uuid::now_v7();
        let old = serde_json::json!({
            "id": id, "job_id": id, "job": "demo/backup", "target_id": id,
            "target": "demo/db", "status": "succeeded",
            "scheduled_at": "2026-09-25T12:00:00Z", "created_at": "2026-09-25T12:00:00Z",
            "attempt_count": 1, "max_attempts": 1, "lateness_seconds": 0
        });
        let run: RunResource = serde_json::from_value(old)?;
        assert_eq!(run.trigger_source, RunTriggerSource::Unknown);
        assert!(!run.rerunnable);
        assert!(run.triggered_at.is_empty());
        Ok(())
    }
}

/// State of one execution Attempt within a durable Run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AttemptStatus {
    PendingDispatch,
    Queued,
    Running,
    Succeeded,
    Skipped,
    Failed,
    Dead,
    Unknown,
}

/// Bounded process output and completion metadata for an authorized Run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RunAttemptResource {
    pub id: Uuid,
    pub worker_id: Option<String>,
    pub attempt: u16,
    pub status: AttemptStatus,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout_tail: Option<String>,
    pub stderr_tail: Option<String>,
    pub error: Option<String>,
}

/// All per-Target Runs created by one idempotent manual request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RunBatchResource {
    pub request_id: Uuid,
    pub runs: Vec<RunResource>,
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
    pub queue_id: Uuid,
    pub queue: String,
    pub concurrency: u16,
    pub version: String,
    pub status: WorkerStatus,
    pub started_at: String,
    pub last_seen_at: String,
    pub active_executions: u64,
}

/// Bounded, allowlisted worker facts; arbitrary process environment is never sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkerDiagnostics {
    pub hostname: String,
    pub os: String,
    pub architecture: String,
    pub default_shell_path: String,
    pub default_shell_present: bool,
    pub dry_run: bool,
    pub lang: Option<String>,
    pub lc_all: Option<String>,
    pub tz: Option<String>,
}

/// Authorized detail view of one worker presence record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkerDetailsResource {
    #[serde(flatten)]
    pub worker: WorkerResource,
    pub diagnostics: Option<WorkerDiagnostics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OverviewResource {
    pub namespaces: u64,
    pub jobs: u64,
    pub targets: u64,
    pub target_sets: u64,
    pub schedules: u64,
    pub runs: u64,
}

/// Read-only operator snapshot; database values cover the connected database,
/// while instance poll times and transport availability describe one API process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MonitorResource {
    pub sampled_at: String,
    pub database_available: bool,
    pub nats_available: bool,
    pub database: Option<DatabaseMonitorResource>,
    pub pipeline: Option<PipelineMonitorResource>,
    pub instance: InstanceMonitorResource,
}

/// Aggregate PostgreSQL database size and this API instance's pool usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatabaseMonitorResource {
    pub size_bytes: u64,
    pub connections: u64,
    pub pool_connections: u32,
    pub pool_idle_connections: u32,
    pub pool_max_connections: u32,
}

/// Durable scheduler, outbox, Run, and worker counts from PostgreSQL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PipelineMonitorResource {
    pub enabled_schedules: u64,
    pub due_schedules: u64,
    pub earliest_next_run_at: Option<String>,
    pub outbox_pending: u64,
    pub outbox_oldest_seconds: u64,
    pub runs_queued: u64,
    pub runs_running: u64,
    pub active_worker_leases: u64,
    pub online_workers: u64,
}

/// Local scheduler and publisher polling signals; not cluster-wide counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct InstanceMonitorResource {
    pub scheduler_last_poll_at: Option<String>,
    pub publisher_last_poll_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorBody {
    /// Stable machine-readable error code. Known values are `invalid_request`,
    /// `unauthenticated`, `forbidden`, `not_found`, `method_not_allowed`,
    /// `already_exists`, `resource_in_use`, `idempotency_conflict`,
    /// `payload_too_large`, `unsupported_media_type`, `dependency_unavailable`,
    /// and `internal_error`; clients should treat unknown codes by HTTP status.
    pub code: String,
    /// Human-readable explanation that never contains internal details.
    pub message: String,
    /// Request field responsible for an `invalid_request` error, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[cfg(test)]
mod resource_name_tests {
    use super::{RESOURCE_NAME_MAX_LENGTH, validate_resource_name};

    #[test]
    fn dns_1123_labels_accept_the_public_examples() {
        for value in ["production", "job-01", "a", "a1", "aws-eu-central-1"] {
            assert!(validate_resource_name(value).is_ok(), "{value:?}");
        }
        assert!(validate_resource_name(&"a".repeat(RESOURCE_NAME_MAX_LENGTH)).is_ok());
    }

    #[test]
    fn dns_1123_labels_reject_noncanonical_values() {
        for value in [
            "",
            "Production",
            "hello_world",
            "hello world",
            "hello.world",
            "hello/world",
            "-job",
            "job-",
            "Zürich",
            "🚀",
        ] {
            assert!(validate_resource_name(value).is_err(), "{value:?}");
        }
        assert!(validate_resource_name(&"a".repeat(RESOURCE_NAME_MAX_LENGTH + 1)).is_err());
    }
}

/// Minimal durable dispatch message; PostgreSQL owns all execution details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchEnvelope {
    pub dispatch_id: Uuid,
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub queue_id: Uuid,
}

/// Source known when the server creates the immutable Run snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTrigger {
    Manual,
    Schedule,
    Rerun,
}

/// Immutable executable configuration returned only after a successful claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionSnapshot {
    pub executor: ExecutorKind,
    pub executable: Option<String>,
    #[serde(default)]
    pub shell_command: Option<String>,
    /// Original argv templates; absent in snapshots created before timeline support.
    #[serde(default)]
    pub argument_templates: Vec<String>,
    pub arguments: Vec<String>,
    pub inputs: serde_json::Value,
    /// Job identity; absent in older immutable snapshots.
    #[serde(default)]
    pub job_id: Option<Uuid>,
    /// Origin known when the server creates the Run; absent in older snapshots.
    #[serde(default)]
    pub trigger: Option<ExecutionTrigger>,
    /// Scheduled occurrence instant, when this Run came from a Schedule.
    #[serde(default)]
    pub scheduled_at: Option<time::OffsetDateTime>,
    pub idempotency_key: Uuid,
    pub queue_id: Uuid,
    pub queue: String,
    pub idempotent: bool,
    /// Copied from the Job when the Run was created; older snapshots execute normally.
    #[serde(default)]
    pub dry_run: bool,
    pub retry_initial_seconds: u32,
    pub retry_max_seconds: u32,
    pub retry_multiplier: f64,
    pub retry_jitter: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRequest {
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub queue_id: Uuid,
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
    pub queue_id: Uuid,
    pub concurrency: u16,
    pub version: String,
    #[serde(default)]
    pub diagnostics: Option<WorkerDiagnostics>,
}

/// Worker request for resolving a configured Queue name to stable routing data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueResolutionRequest {
    pub name: String,
}

/// Minimal Queue identity returned over the server-mediated NATS boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueReference {
    pub id: Uuid,
    pub name: String,
}

/// Result of Queue resolution without exposing persistence failures to workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueResolutionStatus {
    Ready,
    NotFound,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueResolutionResponse {
    pub status: QueueResolutionStatus,
    pub queue: Option<QueueReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub attempt_id: Uuid,
    pub worker_id: String,
    pub succeeded: bool,
    /// A non-executed dry run. Defaults to false for older workers.
    /// Skips also set `succeeded` for compatibility with older servers.
    #[serde(default)]
    pub skipped: bool,
    pub exit_code: Option<i32>,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub error: Option<String>,
}

/// Lease-scoped, bounded live output snapshot; completion remains authoritative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSnapshotRequest {
    pub attempt_id: Uuid,
    pub worker_id: String,
    pub sequence: u64,
    pub stdout_tail: String,
    pub stderr_tail: String,
}

#[cfg(test)]
mod dry_run_compatibility_tests {
    use super::{CompletionRequest, CreateJobRequest, ExecutionSnapshot, UpdateJobRequest};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn older_job_requests_and_run_snapshots_default_to_execution() -> Result<(), serde_json::Error>
    {
        let job: CreateJobRequest = serde_json::from_value(json!({
            "name": "example",
            "queue_id": Uuid::now_v7(),
            "executable": null
        }))?;
        assert!(!job.dry_run);

        let update: UpdateJobRequest = serde_json::from_value(json!({
            "name": "example",
            "queue_id": Uuid::now_v7(),
            "executor": "noop",
            "executable": null,
            "arguments": [],
            "inputs": {},
            "idempotent": false,
            "max_attempts": 1,
            "retry_initial_seconds": 1,
            "retry_max_seconds": 60,
            "retry_multiplier": 2.0,
            "retry_jitter": 0.2
        }))?;
        assert!(!update.dry_run);

        let snapshot: ExecutionSnapshot = serde_json::from_value(json!({
            "executor": "noop",
            "executable": null,
            "arguments": [],
            "inputs": {},
            "idempotency_key": Uuid::now_v7(),
            "queue_id": Uuid::now_v7(),
            "queue": "default",
            "idempotent": false,
            "retry_initial_seconds": 1,
            "retry_max_seconds": 60,
            "retry_multiplier": 2.0,
            "retry_jitter": 0.2
        }))?;
        assert!(!snapshot.dry_run);
        Ok(())
    }

    #[test]
    fn older_worker_completions_default_to_not_skipped() -> Result<(), serde_json::Error> {
        let completion: CompletionRequest = serde_json::from_value(json!({
            "attempt_id": Uuid::now_v7(),
            "worker_id": "worker-1",
            "succeeded": true,
            "exit_code": 0,
            "stdout_tail": "",
            "stderr_tail": "",
            "error": null
        }))?;
        assert!(!completion.skipped);
        Ok(())
    }
}
