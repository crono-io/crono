//! PostgreSQL implementation of Crono's authoritative state.
//!
//! Catalog writes are direct because this project is still a draft. Before any
//! execution leaves PostgreSQL, the adapter snapshots the Job and Target into a
//! Run, creates its first Attempt, and inserts an outbox event in one
//! transaction. Scheduler, publisher, and worker coordination use short leases
//! and conditional updates; uniqueness constraints remain the final duplicate
//! boundary.

use crate::{
    application::{
        ControlPlaneStore, JobDefinition, JobRecord, MetricsSnapshot, MonitorSnapshot, NewSchedule,
        OutboxRecord, Overview, Page, RunAttemptRecord, RunRecord, SchedulePlan, ScheduleRecord,
        StoreError, TargetDefinition, TargetRecord, TargetSetRecord, VisibilityScope, WorkerRecord,
    },
    domain::{
        AttemptId, CatchupPolicy, DispatchId, ExecutorKind, Job, JobData, JobId, MisfirePolicy,
        Namespace, NamespaceId, NamespaceName, Queue, QueueId, QueueName, ResourceName, Run,
        RunData, RunId, RunStatus, Schedule, ScheduleId, ScheduleTiming, Target, TargetId,
        TargetSelection, TargetSet, TargetSetId,
    },
};
use async_trait::async_trait;
use crono_api::{
    ClaimRequest, ClaimResponse, CompletionRequest, DispatchEnvelope, ExecutionSnapshot,
    ExecutionTrigger, ExecutorKind as ApiExecutor, LeaseRequest, WorkerHeartbeatRequest,
};
use crono_execution::{merge_inputs, render_arguments};
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::time::Duration;
use time::OffsetDateTime;
use tracing::error;
use uuid::Uuid;

const MAX_POOL_CONNECTIONS: u32 = 20;

#[derive(Debug, sqlx::FromRow)]
struct JobRow {
    id: Uuid,
    namespace_id: Uuid,
    name: String,
    executor: String,
    queue_id: Uuid,
    queue_name: String,
    executable: Option<String>,
    arguments: serde_json::Value,
    inputs: serde_json::Value,
    idempotent: bool,
    dry_run: bool,
    max_attempts: i32,
    retry_initial_seconds: i32,
    retry_max_seconds: i32,
    retry_multiplier: f64,
    retry_jitter: f64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    namespace_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct QueueRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    enabled: bool,
    system: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, sqlx::FromRow)]
struct TargetRow {
    id: Uuid,
    namespace_id: Uuid,
    name: String,
    arguments: serde_json::Value,
    inputs: serde_json::Value,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    namespace_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct TargetSetRow {
    id: Uuid,
    namespace_id: Uuid,
    name: String,
    inputs: serde_json::Value,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    namespace_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct ScheduleRow {
    id: Uuid,
    namespace_id: Uuid,
    job_id: Uuid,
    target_id: Option<Uuid>,
    target_set_id: Option<Uuid>,
    inputs: serde_json::Value,
    name: String,
    schedule_type: String,
    cron_expression: Option<String>,
    execute_at: Option<OffsetDateTime>,
    timezone: String,
    enabled: bool,
    next_run_at: Option<OffsetDateTime>,
    last_run_at: Option<OffsetDateTime>,
    misfire_policy: String,
    misfire_grace_seconds: Option<i32>,
    catchup_policy: String,
    max_catchup_runs: i32,
    max_catchup_age_seconds: i32,
    revision: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    namespace_name: Option<String>,
    job_name: Option<String>,
    target_name: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct RunRow {
    id: Uuid,
    request_id: Option<Uuid>,
    schedule_id: Option<Uuid>,
    job_id: Uuid,
    target_id: Uuid,
    status: String,
    scheduled_at: OffsetDateTime,
    created_at: OffsetDateTime,
    queued_at: Option<OffsetDateTime>,
    started_at: Option<OffsetDateTime>,
    completed_at: Option<OffsetDateTime>,
    attempt_count: i32,
    max_attempts: i32,
    lateness_seconds: i64,
    terminal_reason: Option<String>,
    job_namespace: String,
    job_name: String,
    target_namespace: String,
    target_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct RunAttemptRow {
    id: Uuid,
    attempt: i32,
    status: String,
    started_at: Option<OffsetDateTime>,
    completed_at: Option<OffsetDateTime>,
    exit_code: Option<i32>,
    stdout_tail: Option<String>,
    stderr_tail: Option<String>,
    error: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct ExecutionRow {
    namespace_id: Uuid,
    job_id: Uuid,
    target_id: Uuid,
    executor: String,
    queue_id: Uuid,
    queue_name: String,
    executable: Option<String>,
    job_arguments: serde_json::Value,
    target_arguments: serde_json::Value,
    job_inputs: serde_json::Value,
    target_inputs: serde_json::Value,
    idempotent: bool,
    dry_run: bool,
    max_attempts: i32,
    retry_initial_seconds: i32,
    retry_max_seconds: i32,
    retry_multiplier: f64,
    retry_jitter: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct RunRequestRow {
    job_id: Uuid,
    target_id: Option<Uuid>,
    target_set_id: Option<Uuid>,
    inputs: serde_json::Value,
}

#[derive(Debug, sqlx::FromRow)]
struct RetryRow {
    attempt_count: i32,
    max_attempts: i32,
    idempotent: bool,
    retry_initial_seconds: i32,
    retry_max_seconds: i32,
    retry_multiplier: f64,
    retry_jitter: f64,
}

#[derive(Debug, sqlx::FromRow)]
struct WorkerRow {
    worker_id: String,
    queue_id: Uuid,
    queue: String,
    concurrency: i32,
    version: String,
    started_at: OffsetDateTime,
    last_seen_at: OffsetDateTime,
    active_executions: i64,
}

#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    /// Connect a bounded pool to an initialized Crono database.
    ///
    /// # Errors
    ///
    /// Returns an availability error when the pool cannot connect.
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(MAX_POOL_CONNECTIONS)
            .acquire_timeout(Duration::from_secs(3))
            .connect(database_url)
            .await
            .map_err(store_error)?;
        Ok(Self { pool })
    }

    fn namespace_ids(visibility: &VisibilityScope) -> Vec<Uuid> {
        match visibility {
            VisibilityScope::Namespaces(ids) => ids.iter().map(|id| id.get()).collect(),
            VisibilityScope::All | VisibilityScope::None => Vec::new(),
        }
    }
}

#[async_trait]
impl ControlPlaneStore for PostgresStore {
    async fn create_namespace(&self, name: &NamespaceName) -> Result<Namespace, StoreError> {
        let row = sqlx::query_as::<_, (Uuid, String, OffsetDateTime)>(
            "INSERT INTO crono.namespaces (name) VALUES ($1)
             RETURNING id, name, created_at",
        )
        .bind(name.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        namespace_from_row(row)
    }

    async fn list_namespaces(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<Namespace>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(empty_page());
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, (Uuid, String, OffsetDateTime)>(
            "SELECT id, name, created_at FROM crono.namespaces
             WHERE ($1::text IS NULL OR name > $1)
               AND (NOT $2 OR id = ANY($3::uuid[]))
             ORDER BY name LIMIT $4",
        )
        .bind(after)
        .bind(restrict)
        .bind(&ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(rows, limit, namespace_from_row, |value| {
            value.name().to_string()
        })
    }

    async fn get_namespace(&self, id: NamespaceId) -> Result<Namespace, StoreError> {
        let row = sqlx::query_as::<_, (Uuid, String, OffsetDateTime)>(
            "SELECT id, name, created_at FROM crono.namespaces WHERE id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        namespace_from_row(row)
    }

    async fn create_queue(
        &self,
        name: &QueueName,
        description: Option<&str>,
    ) -> Result<Queue, StoreError> {
        let row = sqlx::query_as::<_, QueueRow>(
            "INSERT INTO crono.queues (name, description) VALUES ($1, $2)
             RETURNING id, name, description, enabled, system, created_at, updated_at",
        )
        .bind(name.as_str())
        .bind(description)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        queue_from_row(row)
    }

    async fn list_queues(
        &self,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<Queue>, StoreError> {
        let rows = sqlx::query_as::<_, QueueRow>(
            "SELECT id, name, description, enabled, system, created_at, updated_at
               FROM crono.queues
              WHERE ($1::text IS NULL OR name > $1)
              ORDER BY name LIMIT $2",
        )
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(rows, limit, queue_from_row, |queue| {
            queue.name().to_string()
        })
    }

    async fn get_queue(&self, id: QueueId) -> Result<Queue, StoreError> {
        let row = sqlx::query_as::<_, QueueRow>(
            "SELECT id, name, description, enabled, system, created_at, updated_at
               FROM crono.queues WHERE id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        queue_from_row(row)
    }

    async fn get_queue_by_name(&self, name: &QueueName) -> Result<Queue, StoreError> {
        let row = sqlx::query_as::<_, QueueRow>(
            "SELECT id, name, description, enabled, system, created_at, updated_at
               FROM crono.queues WHERE name = $1",
        )
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        queue_from_row(row)
    }

    async fn update_queue(
        &self,
        id: QueueId,
        name: &QueueName,
        description: Option<&str>,
        enabled: bool,
    ) -> Result<Queue, StoreError> {
        let row = sqlx::query_as::<_, QueueRow>(
            "UPDATE crono.queues
                SET name = $2, description = $3, enabled = $4,
                    updated_at = statement_timestamp()
              WHERE id = $1
          RETURNING id, name, description, enabled, system, created_at, updated_at",
        )
        .bind(id.get())
        .bind(name.as_str())
        .bind(description)
        .bind(enabled)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        queue_from_row(row)
    }

    async fn delete_queue(&self, id: QueueId) -> Result<(), StoreError> {
        let result = sqlx::query("DELETE FROM crono.queues WHERE id = $1")
            .bind(id.get())
            .execute(&self.pool)
            .await
            .map_err(store_error)?;
        if result.rows_affected() == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    async fn create_job(
        &self,
        namespace_id: NamespaceId,
        name: &ResourceName,
        definition: &JobDefinition,
    ) -> Result<JobRecord, StoreError> {
        let executor = executor_name(definition.executor);
        let arguments = serde_json::to_value(&definition.arguments).map_err(json_error)?;
        let row = sqlx::query_as::<_, JobRow>(
            "INSERT INTO crono.jobs (
                 namespace_id, name, executor, queue_id, executable, arguments,
                 inputs, idempotent, dry_run, max_attempts, retry_initial_seconds,
                 retry_max_seconds, retry_multiplier, retry_jitter
             )
             SELECT n.id, $2, $3, q.id, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
               FROM crono.namespaces n CROSS JOIN crono.queues q
              WHERE n.id = $1 AND q.id = $4 AND q.enabled
             RETURNING id, namespace_id, name, executor, queue_id,
                       (SELECT name FROM crono.queues WHERE id = $4) AS queue_name, executable,
                       arguments, inputs, idempotent, dry_run, max_attempts, retry_initial_seconds,
                       retry_max_seconds, retry_multiplier, retry_jitter,
                       created_at, updated_at,
                       (SELECT name FROM crono.namespaces WHERE id = $1) AS namespace_name",
        )
        .bind(namespace_id.get())
        .bind(name.as_str())
        .bind(executor)
        .bind(definition.queue_id.get())
        .bind(&definition.executable)
        .bind(arguments)
        .bind(&definition.inputs)
        .bind(definition.idempotent)
        .bind(definition.dry_run)
        .bind(i32::from(definition.max_attempts))
        .bind(i32::try_from(definition.retry_initial_seconds).map_err(|_| StoreError::Internal)?)
        .bind(i32::try_from(definition.retry_max_seconds).map_err(|_| StoreError::Internal)?)
        .bind(definition.retry_multiplier)
        .bind(definition.retry_jitter)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        job_from_row(row)
    }

    async fn list_jobs(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(empty_page());
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, JobRow>(
            "SELECT j.id, j.namespace_id, j.name, j.executor, j.queue_id,
                    q.name AS queue_name, j.executable,
                    j.arguments, j.inputs, j.idempotent, j.dry_run, j.max_attempts, j.retry_initial_seconds,
                    j.retry_max_seconds, j.retry_multiplier, j.retry_jitter,
                    j.created_at, j.updated_at,
                    n.name AS namespace_name
             FROM crono.jobs j
             JOIN crono.namespaces n ON n.id = j.namespace_id
             JOIN crono.queues q ON q.id = j.queue_id
             WHERE n.id = $1 AND ($2::text IS NULL OR j.name > $2)
               AND (NOT $3 OR n.id = ANY($4::uuid[]))
             ORDER BY j.name LIMIT $5",
        )
        .bind(namespace_id.get())
        .bind(after)
        .bind(restrict)
        .bind(&ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(rows, limit, job_from_row, |value| {
            value.job.name().to_string()
        })
    }

    async fn get_job(&self, id: JobId) -> Result<JobRecord, StoreError> {
        let row = sqlx::query_as::<_, JobRow>(
            "SELECT j.id, j.namespace_id, j.name, j.executor, j.queue_id,
                    q.name AS queue_name, j.executable,
                    j.arguments, j.inputs, j.idempotent, j.dry_run, j.max_attempts, j.retry_initial_seconds,
                    j.retry_max_seconds, j.retry_multiplier, j.retry_jitter,
                    j.created_at, j.updated_at,
                    n.name AS namespace_name
             FROM crono.jobs j
             JOIN crono.namespaces n ON n.id = j.namespace_id
             JOIN crono.queues q ON q.id = j.queue_id
             WHERE j.id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        job_from_row(row)
    }

    async fn update_job(
        &self,
        id: JobId,
        name: &ResourceName,
        definition: &JobDefinition,
    ) -> Result<JobRecord, StoreError> {
        let arguments = serde_json::to_value(&definition.arguments).map_err(json_error)?;
        let row = sqlx::query_as::<_, JobRow>(
            "WITH changed AS (
                 UPDATE crono.jobs
                    SET name = $2, executor = $3, queue_id = q.id, executable = $5,
                        arguments = $6, inputs = $7, idempotent = $8,
                        dry_run = $9, max_attempts = $10, retry_initial_seconds = $11,
                        retry_max_seconds = $12, retry_multiplier = $13,
                        retry_jitter = $14, updated_at = statement_timestamp()
                   FROM crono.queues q
                  WHERE crono.jobs.id = $1 AND q.id = $4
                    AND (q.enabled OR q.id = crono.jobs.queue_id)
              RETURNING crono.jobs.*
             )
             SELECT j.id, j.namespace_id, j.name, j.executor, j.queue_id,
                    q.name AS queue_name, j.executable, j.arguments, j.inputs,
                    j.idempotent, j.dry_run, j.max_attempts, j.retry_initial_seconds,
                    j.retry_max_seconds, j.retry_multiplier, j.retry_jitter,
                    j.created_at, j.updated_at, n.name AS namespace_name
               FROM changed j
               JOIN crono.namespaces n ON n.id = j.namespace_id
               JOIN crono.queues q ON q.id = j.queue_id",
        )
        .bind(id.get())
        .bind(name.as_str())
        .bind(executor_name(definition.executor))
        .bind(definition.queue_id.get())
        .bind(&definition.executable)
        .bind(arguments)
        .bind(&definition.inputs)
        .bind(definition.idempotent)
        .bind(definition.dry_run)
        .bind(i32::from(definition.max_attempts))
        .bind(i32::try_from(definition.retry_initial_seconds).map_err(|_| StoreError::Internal)?)
        .bind(i32::try_from(definition.retry_max_seconds).map_err(|_| StoreError::Internal)?)
        .bind(definition.retry_multiplier)
        .bind(definition.retry_jitter)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        job_from_row(row)
    }

    async fn create_target(
        &self,
        namespace_id: NamespaceId,
        name: &ResourceName,
        definition: &TargetDefinition,
    ) -> Result<TargetRecord, StoreError> {
        let arguments = serde_json::to_value(&definition.arguments).map_err(json_error)?;
        let row = sqlx::query_as::<_, TargetRow>(
            "INSERT INTO crono.targets (namespace_id, name, arguments, inputs)
             SELECT id, $2, $3, $4 FROM crono.namespaces WHERE id = $1
             RETURNING id, namespace_id, name, arguments, inputs, created_at, updated_at,
                       (SELECT name FROM crono.namespaces WHERE id = $1) AS namespace_name",
        )
        .bind(namespace_id.get())
        .bind(name.as_str())
        .bind(arguments)
        .bind(&definition.inputs)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        target_from_row(row)
    }

    async fn list_targets(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(empty_page());
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, TargetRow>(
            "SELECT t.id, t.namespace_id, t.name, t.arguments, t.inputs, t.created_at, t.updated_at,
                    n.name AS namespace_name
             FROM crono.targets t JOIN crono.namespaces n ON n.id = t.namespace_id
             WHERE n.id = $1 AND ($2::text IS NULL OR t.name > $2)
               AND (NOT $3 OR n.id = ANY($4::uuid[]))
             ORDER BY t.name LIMIT $5",
        )
        .bind(namespace_id.get())
        .bind(after)
        .bind(restrict)
        .bind(&ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(rows, limit, target_from_row, |value| {
            value.target.name().to_string()
        })
    }

    async fn get_target(&self, id: TargetId) -> Result<TargetRecord, StoreError> {
        let row = sqlx::query_as::<_, TargetRow>(
            "SELECT t.id, t.namespace_id, t.name, t.arguments, t.inputs, t.created_at, t.updated_at,
                    n.name AS namespace_name
             FROM crono.targets t JOIN crono.namespaces n ON n.id = t.namespace_id
             WHERE t.id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        target_from_row(row)
    }

    async fn update_target(
        &self,
        id: TargetId,
        name: &ResourceName,
        definition: &TargetDefinition,
    ) -> Result<TargetRecord, StoreError> {
        let arguments = serde_json::to_value(&definition.arguments).map_err(json_error)?;
        let row = sqlx::query_as::<_, TargetRow>(
            "WITH changed AS (
                 UPDATE crono.targets
                    SET name = $2, arguments = $3, inputs = $4,
                        updated_at = statement_timestamp()
                  WHERE id = $1
              RETURNING *
             )
             SELECT t.id, t.namespace_id, t.name, t.arguments, t.inputs,
                    t.created_at, t.updated_at, n.name AS namespace_name
               FROM changed t JOIN crono.namespaces n ON n.id = t.namespace_id",
        )
        .bind(id.get())
        .bind(name.as_str())
        .bind(arguments)
        .bind(&definition.inputs)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        target_from_row(row)
    }

    async fn create_target_set(
        &self,
        namespace_id: NamespaceId,
        name: &ResourceName,
        target_ids: &[TargetId],
        inputs: &serde_json::Value,
    ) -> Result<TargetSetRecord, StoreError> {
        let ids: Vec<Uuid> = target_ids.iter().map(|id| id.get()).collect();
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let row = sqlx::query_as::<_, TargetSetRow>(
            "INSERT INTO crono.target_sets (namespace_id, name, inputs)
             SELECT id, $2, $3 FROM crono.namespaces WHERE id = $1
             RETURNING id, namespace_id, name, inputs, created_at, updated_at,
                       (SELECT name FROM crono.namespaces WHERE id = $1) AS namespace_name",
        )
        .bind(namespace_id.get())
        .bind(name.as_str())
        .bind(inputs)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        let targets = target_rows_for_ids(&mut transaction, namespace_id, &ids).await?;
        if targets.len() != ids.len() {
            return Err(StoreError::NotFound);
        }
        sqlx::query(
            "INSERT INTO crono.target_set_members (target_set_id, target_id)
             SELECT $1, target_id FROM unnest($2::uuid[]) AS members(target_id)",
        )
        .bind(row.id)
        .bind(&ids)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        transaction.commit().await.map_err(store_error)?;
        target_set_from_rows(&row, targets)
    }

    async fn list_target_sets(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetSetRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(empty_page());
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, TargetSetRow>(
            "SELECT ts.id, ts.namespace_id, ts.name, ts.inputs, ts.created_at, ts.updated_at,
                    n.name AS namespace_name
               FROM crono.target_sets ts
               JOIN crono.namespaces n ON n.id = ts.namespace_id
              WHERE n.id = $1 AND ($2::text IS NULL OR ts.name > $2)
                AND (NOT $3 OR n.id = ANY($4::uuid[]))
              ORDER BY ts.name LIMIT $5",
        )
        .bind(namespace_id.get())
        .bind(after)
        .bind(restrict)
        .bind(&ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        let has_more = rows.len() > usize::from(limit);
        let mut records = Vec::with_capacity(rows.len().min(usize::from(limit)));
        for row in rows.into_iter().take(usize::from(limit)) {
            records.push(load_target_set_record(&self.pool, row).await?);
        }
        let next_cursor = if has_more {
            records
                .last()
                .map(|record| record.target_set.name().to_string())
        } else {
            None
        };
        Ok(Page {
            items: records,
            next_cursor,
        })
    }

    async fn get_target_set(&self, id: TargetSetId) -> Result<TargetSetRecord, StoreError> {
        let row = sqlx::query_as::<_, TargetSetRow>(
            "SELECT ts.id, ts.namespace_id, ts.name, ts.inputs, ts.created_at, ts.updated_at,
                    n.name AS namespace_name
               FROM crono.target_sets ts
               JOIN crono.namespaces n ON n.id = ts.namespace_id
              WHERE ts.id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        load_target_set_record(&self.pool, row).await
    }

    async fn update_target_set(
        &self,
        id: TargetSetId,
        name: &ResourceName,
        target_ids: &[TargetId],
        inputs: &serde_json::Value,
    ) -> Result<TargetSetRecord, StoreError> {
        let ids: Vec<Uuid> = target_ids.iter().map(|target_id| target_id.get()).collect();
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let row = sqlx::query_as::<_, TargetSetRow>(
            "WITH changed AS (
                 UPDATE crono.target_sets
                    SET name = $2, inputs = $3, updated_at = statement_timestamp()
                  WHERE id = $1
              RETURNING *
             )
             SELECT ts.id, ts.namespace_id, ts.name, ts.inputs, ts.created_at,
                    ts.updated_at, n.name AS namespace_name
               FROM changed ts JOIN crono.namespaces n ON n.id = ts.namespace_id",
        )
        .bind(id.get())
        .bind(name.as_str())
        .bind(inputs)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        let namespace_id = NamespaceId::new(row.namespace_id);
        let targets = target_rows_for_ids(&mut transaction, namespace_id, &ids).await?;
        if targets.len() != ids.len() {
            return Err(StoreError::NotFound);
        }
        sqlx::query("DELETE FROM crono.target_set_members WHERE target_set_id = $1")
            .bind(id.get())
            .execute(&mut *transaction)
            .await
            .map_err(store_error)?;
        sqlx::query(
            "INSERT INTO crono.target_set_members (target_set_id, target_id)
             SELECT $1, target_id FROM unnest($2::uuid[]) AS members(target_id)",
        )
        .bind(id.get())
        .bind(&ids)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        transaction.commit().await.map_err(store_error)?;
        target_set_from_rows(&row, targets)
    }

    async fn create_schedule(&self, schedule: &NewSchedule) -> Result<ScheduleRecord, StoreError> {
        let schedule_type = if schedule.cron_expression.is_some() {
            "cron"
        } else {
            "once"
        };
        let (target_id, target_set_id) = selection_ids(schedule.target);
        let row = sqlx::query_as::<_, ScheduleRow>(
            "WITH inserted AS (
                 INSERT INTO crono.schedules (
                     namespace_id, job_id, target_id, target_set_id, inputs, name, schedule_type,
                     cron_expression, execute_at, timezone, next_run_at,
                     misfire_policy, misfire_grace_seconds, catchup_policy,
                     max_catchup_runs, max_catchup_age_seconds
                 )
                 VALUES ($1, $3, $4, $5, $6, $2, $7, $8, $9, $10, $11,
                         $12, $13, $14, $15, $16)
                 RETURNING *
             )
             SELECT s.id, s.namespace_id, s.job_id, s.target_id, s.target_set_id, s.inputs,
                    s.name, s.schedule_type,
                    s.cron_expression, s.execute_at, s.timezone, s.enabled, s.next_run_at,
                    s.last_run_at, s.misfire_policy, s.misfire_grace_seconds,
                    s.catchup_policy, s.max_catchup_runs, s.max_catchup_age_seconds,
                    s.revision, s.created_at, s.updated_at,
                    n.name AS namespace_name, j.name AS job_name,
                    COALESCE(t.name, ts.name) AS target_name
               FROM inserted s
               JOIN crono.namespaces n ON n.id = s.namespace_id
               JOIN crono.jobs j ON j.id = s.job_id
               LEFT JOIN crono.targets t ON t.id = s.target_id
               LEFT JOIN crono.target_sets ts ON ts.id = s.target_set_id",
        )
        .bind(schedule.namespace_id.get())
        .bind(schedule.name.as_str())
        .bind(schedule.job_id.get())
        .bind(target_id)
        .bind(target_set_id)
        .bind(&schedule.inputs)
        .bind(schedule_type)
        .bind(&schedule.cron_expression)
        .bind(schedule.execute_at)
        .bind(&schedule.timezone)
        .bind(schedule.next_run_at)
        .bind(misfire_name(schedule.misfire_policy))
        .bind(schedule.misfire_grace_seconds.map(i64::from))
        .bind(catchup_name(schedule.catchup_policy))
        .bind(i32::from(schedule.max_catchup_runs))
        .bind(i64::from(schedule.max_catchup_age_seconds))
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        schedule_from_row(&row)
    }

    async fn list_schedules(
        &self,
        namespace_id: NamespaceId,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<ScheduleRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(empty_page());
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, ScheduleRow>(
            "SELECT s.id, s.namespace_id, s.job_id, s.target_id, s.target_set_id, s.inputs,
                    s.name, s.schedule_type,
                    s.cron_expression, s.execute_at, s.timezone, s.enabled, s.next_run_at,
                    s.last_run_at, s.misfire_policy, s.misfire_grace_seconds,
                    s.catchup_policy, s.max_catchup_runs, s.max_catchup_age_seconds,
                    s.revision, s.created_at, s.updated_at,
                    n.name AS namespace_name, j.name AS job_name,
                    COALESCE(t.name, ts.name) AS target_name
             FROM crono.schedules s
             JOIN crono.namespaces n ON n.id = s.namespace_id
             JOIN crono.jobs j ON j.id = s.job_id
             LEFT JOIN crono.targets t ON t.id = s.target_id
             LEFT JOIN crono.target_sets ts ON ts.id = s.target_set_id
             WHERE n.id = $1 AND ($2::text IS NULL OR s.name > $2)
               AND (NOT $3 OR n.id = ANY($4::uuid[]))
             ORDER BY s.name LIMIT $5",
        )
        .bind(namespace_id.get())
        .bind(after)
        .bind(restrict)
        .bind(&ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(
            rows,
            limit,
            |row| schedule_from_row(&row),
            |value| value.schedule.name.to_string(),
        )
    }

    async fn get_schedule(&self, id: ScheduleId) -> Result<ScheduleRecord, StoreError> {
        let row = sqlx::query_as::<_, ScheduleRow>(
            "SELECT s.id, s.namespace_id, s.job_id, s.target_id, s.target_set_id, s.inputs,
                    s.name, s.schedule_type,
                    s.cron_expression, s.execute_at, s.timezone, s.enabled, s.next_run_at,
                    s.last_run_at, s.misfire_policy, s.misfire_grace_seconds,
                    s.catchup_policy, s.max_catchup_runs, s.max_catchup_age_seconds,
                    s.revision, s.created_at, s.updated_at,
                    n.name AS namespace_name, j.name AS job_name,
                    COALESCE(t.name, ts.name) AS target_name
             FROM crono.schedules s
             JOIN crono.namespaces n ON n.id = s.namespace_id
             JOIN crono.jobs j ON j.id = s.job_id
             LEFT JOIN crono.targets t ON t.id = s.target_id
             LEFT JOIN crono.target_sets ts ON ts.id = s.target_set_id
             WHERE s.id = $1",
        )
        .bind(id.get())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        schedule_from_row(&row)
    }

    async fn set_schedule_enabled(
        &self,
        id: ScheduleId,
        revision: u64,
        enabled: bool,
        next_run_at: Option<OffsetDateTime>,
    ) -> Result<ScheduleRecord, StoreError> {
        let revision = i64::try_from(revision).map_err(|_| StoreError::Internal)?;
        let row = sqlx::query_as::<_, ScheduleRow>(
            "WITH changed AS (
                 UPDATE crono.schedules
                    SET enabled = $3, next_run_at = $4, revision = revision + 1,
                        claim_owner = NULL, claim_expires_at = NULL,
                        updated_at = statement_timestamp()
                  WHERE id = $1 AND revision = $2
                  RETURNING *
             )
             SELECT s.id, s.namespace_id, s.job_id, s.target_id, s.target_set_id, s.inputs,
                    s.name, s.schedule_type,
                    s.cron_expression, s.execute_at, s.timezone, s.enabled, s.next_run_at,
                    s.last_run_at, s.misfire_policy, s.misfire_grace_seconds,
                    s.catchup_policy, s.max_catchup_runs, s.max_catchup_age_seconds,
                    s.revision, s.created_at, s.updated_at,
                    n.name AS namespace_name, j.name AS job_name,
                    COALESCE(t.name, ts.name) AS target_name
               FROM changed s
               JOIN crono.namespaces n ON n.id = s.namespace_id
               JOIN crono.jobs j ON j.id = s.job_id
               LEFT JOIN crono.targets t ON t.id = s.target_id
               LEFT JOIN crono.target_sets ts ON ts.id = s.target_set_id",
        )
        .bind(id.get())
        .bind(revision)
        .bind(enabled)
        .bind(next_run_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::StaleRevision)?;
        schedule_from_row(&row)
    }

    async fn create_runs(
        &self,
        request_id: Uuid,
        job_id: JobId,
        target: TargetSelection,
        inputs: &serde_json::Value,
    ) -> Result<(Vec<RunRecord>, bool), StoreError> {
        if let Some(existing) = find_run_request(&self.pool, request_id).await? {
            compare_run_request(&existing, job_id, target, inputs)?;
            return Ok((runs_for_request(&self.pool, request_id).await?, false));
        }
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let (executions, target_set_inputs) =
            load_selection_executions(&mut transaction, job_id.get(), target).await?;
        let snapshots = executions
            .iter()
            .map(|execution| {
                let run_id = Uuid::now_v7();
                execution_snapshot_with_inputs(
                    run_id,
                    execution,
                    target_set_inputs.as_ref(),
                    inputs,
                    ExecutionTrigger::Manual,
                    None,
                )
                .map(|snapshot| (run_id, snapshot))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|message| {
                error!(%message, "persisted execution could not be rendered");
                StoreError::Internal
            })?;
        let (target_id, target_set_id) = selection_ids(target);
        let namespace_id = executions
            .first()
            .map(|execution| execution.namespace_id)
            .ok_or(StoreError::NotFound)?;
        let inserted_request = sqlx::query(
            "INSERT INTO crono.run_requests (
                 request_id, namespace_id, job_id, target_id, target_set_id, inputs
             ) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(request_id)
        .bind(namespace_id)
        .bind(job_id.get())
        .bind(target_id)
        .bind(target_set_id)
        .bind(inputs)
        .execute(&mut *transaction)
        .await;
        if let Err(error) = inserted_request {
            if is_unique_violation(&error) {
                transaction.rollback().await.map_err(store_error)?;
                let existing = find_run_request(&self.pool, request_id)
                    .await?
                    .ok_or(StoreError::Internal)?;
                compare_run_request(&existing, job_id, target, inputs)?;
                return Ok((runs_for_request(&self.pool, request_id).await?, false));
            }
            return Err(store_error(error));
        }
        let now = OffsetDateTime::now_utc();
        for (execution, (run_id, snapshot)) in executions.iter().zip(snapshots) {
            sqlx::query(
                "INSERT INTO crono.runs (
                     id, request_id, job_id, target_id, queue_id, scheduled_at,
                     execution_snapshot, attempt_count, max_attempts
                 ) VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8)",
            )
            .bind(run_id)
            .bind(request_id)
            .bind(execution.job_id)
            .bind(execution.target_id)
            .bind(execution.queue_id)
            .bind(now)
            .bind(snapshot)
            .bind(execution.max_attempts)
            .execute(&mut *transaction)
            .await
            .map_err(store_error)?;
            create_attempt_and_outbox(&mut transaction, run_id, 1, execution.queue_id, None)
                .await?;
            insert_run_event(&mut transaction, run_id, "created", serde_json::json!({})).await?;
        }
        transaction.commit().await.map_err(store_error)?;
        Ok((runs_for_request(&self.pool, request_id).await?, true))
    }

    async fn list_runs(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(empty_page());
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, RunRow>(
            "SELECT r.id, r.request_id, r.schedule_id, r.job_id, r.target_id, r.status,
                    r.scheduled_at, r.created_at, r.queued_at, r.started_at, r.completed_at,
                    r.attempt_count, r.max_attempts, r.lateness_seconds, r.terminal_reason,
                    jn.name AS job_namespace, j.name AS job_name,
                    tn.name AS target_namespace, t.name AS target_name
             FROM crono.runs r
             JOIN crono.jobs j ON j.id = r.job_id
             JOIN crono.namespaces jn ON jn.id = j.namespace_id
             JOIN crono.targets t ON t.id = r.target_id
             JOIN crono.namespaces tn ON tn.id = t.namespace_id
             WHERE ($1::uuid IS NULL OR r.id < $1)
               AND (NOT $2 OR j.namespace_id = ANY($3::uuid[]))
             ORDER BY r.id DESC LIMIT $4",
        )
        .bind(before)
        .bind(restrict)
        .bind(&ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(rows, limit, run_from_row, |value| {
            value.run.id().get().to_string()
        })
    }

    async fn get_run(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<RunRecord, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Err(StoreError::NotFound);
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let row = sqlx::query_as::<_, RunRow>(
            "SELECT r.id, r.request_id, r.schedule_id, r.job_id, r.target_id, r.status,
                    r.scheduled_at, r.created_at, r.queued_at, r.started_at, r.completed_at,
                    r.attempt_count, r.max_attempts, r.lateness_seconds, r.terminal_reason,
                    jn.name AS job_namespace, j.name AS job_name,
                    tn.name AS target_namespace, t.name AS target_name
             FROM crono.runs r
             JOIN crono.jobs j ON j.id = r.job_id
             JOIN crono.namespaces jn ON jn.id = j.namespace_id
             JOIN crono.targets t ON t.id = r.target_id
             JOIN crono.namespaces tn ON tn.id = t.namespace_id
             WHERE r.id = $1 AND (NOT $2 OR j.namespace_id = ANY($3::uuid[]))",
        )
        .bind(id.get())
        .bind(restrict)
        .bind(&ids)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        run_from_row(row)
    }

    async fn list_run_attempts(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<Vec<RunAttemptRecord>, StoreError> {
        self.get_run(id, visibility).await?;
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, RunAttemptRow>(
            "SELECT a.id, a.attempt, a.status, a.started_at, a.completed_at,
                    a.exit_code, a.stdout_tail, a.stderr_tail, a.error
               FROM crono.run_attempts a
               JOIN crono.runs r ON r.id = a.run_id
               JOIN crono.jobs j ON j.id = r.job_id
              WHERE a.run_id = $1 AND (NOT $2 OR j.namespace_id = ANY($3::uuid[]))
              ORDER BY a.attempt DESC LIMIT 100",
        )
        .bind(id.get())
        .bind(restrict)
        .bind(&ids)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(RunAttemptRecord {
                    id: row.id,
                    attempt: u16::try_from(row.attempt).map_err(|_| StoreError::Internal)?,
                    status: row.status,
                    started_at: row.started_at,
                    completed_at: row.completed_at,
                    exit_code: row.exit_code,
                    stdout_tail: row.stdout_tail,
                    stderr_tail: row.stderr_tail,
                    error: row.error,
                })
            })
            .collect()
    }

    async fn record_worker_heartbeat(
        &self,
        request: &WorkerHeartbeatRequest,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO crono.worker_presence
                (worker_id, session_id, queue_id, concurrency, version)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (worker_id) DO UPDATE SET
                session_id = EXCLUDED.session_id,
                queue_id = EXCLUDED.queue_id,
                concurrency = EXCLUDED.concurrency,
                version = EXCLUDED.version,
                started_at = CASE
                    WHEN crono.worker_presence.session_id <> EXCLUDED.session_id
                    THEN statement_timestamp()
                    ELSE crono.worker_presence.started_at
                END,
                last_seen_at = statement_timestamp()",
        )
        .bind(&request.worker_id)
        .bind(request.session_id)
        .bind(request.queue_id)
        .bind(i32::from(request.concurrency))
        .bind(&request.version)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }

    async fn list_workers(
        &self,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<WorkerRecord>, StoreError> {
        let rows = sqlx::query_as::<_, WorkerRow>(
            "SELECT wp.worker_id, wp.queue_id, q.name AS queue, wp.concurrency, wp.version,
                    wp.started_at, wp.last_seen_at,
                    count(a.id) FILTER (
                        WHERE a.status = 'running'
                          AND a.lease_expires_at > statement_timestamp()
                    ) AS active_executions
               FROM crono.worker_presence wp
               JOIN crono.queues q ON q.id = wp.queue_id
               LEFT JOIN crono.run_attempts a ON a.worker_id = wp.worker_id
              WHERE ($1::text IS NULL OR wp.worker_id > $1)
              GROUP BY wp.worker_id, wp.queue_id, q.name, wp.concurrency, wp.version,
                       wp.started_at, wp.last_seen_at
              ORDER BY wp.worker_id
              LIMIT $2",
        )
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        page(rows, limit, worker_from_row, |worker| {
            worker.worker_id.clone()
        })
    }

    async fn overview(&self, visibility: &VisibilityScope) -> Result<Overview, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(Overview {
                namespaces: 0,
                jobs: 0,
                targets: 0,
                target_sets: 0,
                schedules: 0,
                runs: 0,
            });
        }
        let ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64)>(
            "SELECT
                (SELECT count(*) FROM crono.namespaces n WHERE NOT $1 OR n.id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.jobs j WHERE NOT $1 OR j.namespace_id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.targets t WHERE NOT $1 OR t.namespace_id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.target_sets ts WHERE NOT $1 OR ts.namespace_id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.schedules s WHERE NOT $1 OR s.namespace_id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.runs r JOIN crono.jobs j ON j.id = r.job_id
                    WHERE NOT $1 OR j.namespace_id = ANY($2::uuid[]))",
        )
        .bind(restrict)
        .bind(&ids)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(Overview {
            namespaces: count(row.0)?,
            jobs: count(row.1)?,
            targets: count(row.2)?,
            target_sets: count(row.3)?,
            schedules: count(row.4)?,
            runs: count(row.5)?,
        })
    }

    async fn claim_due_schedules(
        &self,
        owner: Uuid,
        limit: u16,
        lease: Duration,
    ) -> Result<Vec<Schedule>, StoreError> {
        let lease_seconds = duration_seconds(lease)?;
        let rows = sqlx::query_as::<_, ScheduleRow>(
            "WITH candidates AS (
                 SELECT id FROM crono.schedules
                  WHERE enabled = true AND next_run_at <= statement_timestamp()
                    AND (claim_expires_at IS NULL OR claim_expires_at < statement_timestamp())
                  ORDER BY next_run_at, id
                  FOR UPDATE SKIP LOCKED
                  LIMIT $2
             )
             UPDATE crono.schedules s
                SET claim_owner = $1,
                    claim_expires_at = statement_timestamp() + make_interval(secs => $3)
               FROM candidates c
              WHERE s.id = c.id
             RETURNING s.id, s.namespace_id, s.job_id, s.target_id, s.target_set_id, s.inputs,
                       s.name, s.schedule_type,
                       s.cron_expression, s.execute_at, s.timezone, s.enabled, s.next_run_at,
                       s.last_run_at, s.misfire_policy, s.misfire_grace_seconds,
                       s.catchup_policy, s.max_catchup_runs, s.max_catchup_age_seconds,
                       s.revision, s.created_at, s.updated_at,
                       NULL::text AS namespace_name, NULL::text AS job_name,
                       NULL::text AS target_name",
        )
        .bind(owner)
        .bind(i64::from(limit))
        .bind(lease_seconds)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.into_iter().map(|row| schedule_entity(&row)).collect()
    }

    async fn commit_schedule_plan(&self, plan: &SchedulePlan) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let schedule = sqlx::query_as::<_, (Uuid, Option<Uuid>, Option<Uuid>, serde_json::Value)>(
            "SELECT job_id, target_id, target_set_id, inputs FROM crono.schedules
              WHERE id = $1 AND claim_owner = $2 AND claim_expires_at > statement_timestamp()
              FOR UPDATE",
        )
        .bind(plan.schedule_id.get())
        .bind(plan.owner)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::Conflict)?;
        let selection = target_selection(schedule.1, schedule.2)?;
        let (executions, target_set_inputs) =
            load_selection_executions(&mut transaction, schedule.0, selection).await?;
        for occurrence in &plan.occurrences {
            for execution in &executions {
                insert_scheduled_occurrence(
                    &mut transaction,
                    plan.schedule_id,
                    occurrence,
                    execution,
                    target_set_inputs.as_ref(),
                    &schedule.3,
                )
                .await?;
            }
        }
        let updated = sqlx::query(
            "UPDATE crono.schedules
                SET next_run_at = $3,
                    last_run_at = COALESCE($4, last_run_at),
                    enabled = CASE WHEN $5 THEN false ELSE enabled END,
                    claim_owner = NULL, claim_expires_at = NULL,
                    updated_at = statement_timestamp()
              WHERE id = $1 AND claim_owner = $2",
        )
        .bind(plan.schedule_id.get())
        .bind(plan.owner)
        .bind(plan.next_run_at)
        .bind(plan.occurrences.last().map(|value| value.scheduled_at))
        .bind(plan.disable)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        transaction.commit().await.map_err(store_error)
    }

    async fn claim_outbox(
        &self,
        owner: Uuid,
        limit: u16,
        lease: Duration,
    ) -> Result<Vec<OutboxRecord>, StoreError> {
        let lease_seconds = duration_seconds(lease)?;
        let rows = sqlx::query_as::<_, (Uuid, Uuid, Uuid, String, serde_json::Value, i32)>(
            "WITH candidates AS (
                 SELECT id FROM crono.outbox
                  WHERE published_at IS NULL AND cancelled_at IS NULL
                    AND next_attempt_at <= statement_timestamp()
                    AND (dispatch_deadline IS NULL OR dispatch_deadline >= statement_timestamp())
                    AND (claim_expires_at IS NULL OR claim_expires_at < statement_timestamp())
                  ORDER BY next_attempt_at, id
                  FOR UPDATE SKIP LOCKED
                  LIMIT $2
             )
             UPDATE crono.outbox o
                SET claimed_by = $1,
                    claim_expires_at = statement_timestamp() + make_interval(secs => $3)
               FROM candidates c
              WHERE o.id = c.id
             RETURNING o.id, o.run_id, o.attempt_id, o.subject, o.payload, o.attempt_count",
        )
        .bind(owner)
        .bind(i64::from(limit))
        .bind(lease_seconds)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.into_iter()
            .map(
                |(id, run_id, attempt_id, subject, payload, attempt_count)| {
                    let payload = serde_json::to_vec(&payload).map_err(json_error)?;
                    let attempt_count =
                        u32::try_from(attempt_count).map_err(|_| StoreError::Internal)?;
                    Ok(OutboxRecord {
                        id: DispatchId::new(id),
                        run_id: RunId::new(run_id),
                        attempt_id: AttemptId::new(attempt_id),
                        subject,
                        payload,
                        attempt_count,
                    })
                },
            )
            .collect()
    }

    async fn mark_published(
        &self,
        owner: Uuid,
        record: &OutboxRecord,
        stream_sequence: u64,
    ) -> Result<(), StoreError> {
        let sequence = i64::try_from(stream_sequence).map_err(|_| StoreError::Internal)?;
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let updated = sqlx::query(
            "UPDATE crono.outbox
                SET published_at = statement_timestamp(), nats_stream_sequence = $4,
                    claimed_by = NULL, claim_expires_at = NULL, last_error = NULL
              WHERE id = $1 AND run_id = $2 AND claimed_by = $3 AND published_at IS NULL",
        )
        .bind(record.id.get())
        .bind(record.run_id.get())
        .bind(owner)
        .bind(sequence)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        sqlx::query(
            "UPDATE crono.run_attempts
                SET status = 'queued'
              WHERE id = $1 AND status = 'pending_dispatch'",
        )
        .bind(record.attempt_id.get())
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        sqlx::query(
            "UPDATE crono.runs
                SET status = 'queued', queued_at = statement_timestamp()
              WHERE id = $1 AND status = 'pending_dispatch'",
        )
        .bind(record.run_id.get())
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        insert_run_event(
            &mut transaction,
            record.run_id.get(),
            "queued",
            serde_json::json!({ "stream_sequence": sequence }),
        )
        .await?;
        transaction.commit().await.map_err(store_error)
    }

    async fn record_publish_failure(
        &self,
        owner: Uuid,
        dispatch_id: DispatchId,
        message: &str,
        next_attempt_at: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let bounded: String = message.chars().take(1024).collect();
        sqlx::query(
            "UPDATE crono.outbox
                SET attempt_count = attempt_count + 1, last_error = $3,
                    next_attempt_at = $4, claimed_by = NULL, claim_expires_at = NULL
              WHERE id = $1 AND claimed_by = $2 AND published_at IS NULL",
        )
        .bind(dispatch_id.get())
        .bind(owner)
        .bind(bounded)
        .bind(next_attempt_at)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }

    async fn claim_attempt(&self, request: &ClaimRequest) -> Result<ClaimResponse, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let claimed = sqlx::query_scalar::<_, Uuid>(
            "UPDATE crono.run_attempts
                SET status = 'running', worker_id = $3, started_at = statement_timestamp(),
                    heartbeat_at = statement_timestamp(),
                    lease_expires_at = statement_timestamp() + interval '60 seconds'
               FROM crono.runs r
              WHERE crono.run_attempts.id = $1
                AND crono.run_attempts.run_id = $2
                AND crono.run_attempts.status = 'queued'
                AND r.id = crono.run_attempts.run_id
                AND r.queue_id = $4
              RETURNING crono.run_attempts.run_id",
        )
        .bind(request.attempt_id)
        .bind(request.run_id)
        .bind(&request.worker_id)
        .bind(request.queue_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?;
        let Some(run_id) = claimed else {
            transaction.rollback().await.map_err(store_error)?;
            return Ok(ClaimResponse {
                claimed: false,
                lease_seconds: 60,
                execution: None,
            });
        };
        let snapshot = sqlx::query_scalar::<_, serde_json::Value>(
            "UPDATE crono.runs
                SET status = 'running', started_at = COALESCE(started_at, statement_timestamp())
              WHERE id = $1 AND status = 'queued'
              RETURNING execution_snapshot",
        )
        .bind(run_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::Conflict)?;
        insert_run_event(
            &mut transaction,
            run_id,
            "running",
            serde_json::json!({ "worker_id": request.worker_id }),
        )
        .await?;
        transaction.commit().await.map_err(store_error)?;
        let execution = serde_json::from_value(snapshot).map_err(json_error)?;
        Ok(ClaimResponse {
            claimed: true,
            lease_seconds: 60,
            execution: Some(execution),
        })
    }

    async fn renew_lease(&self, request: &LeaseRequest) -> Result<bool, StoreError> {
        let result = sqlx::query(
            "UPDATE crono.run_attempts
                SET heartbeat_at = statement_timestamp(),
                    lease_expires_at = statement_timestamp() + interval '60 seconds'
              WHERE id = $1 AND worker_id = $2 AND status = 'running'
                AND lease_expires_at > statement_timestamp()",
        )
        .bind(request.attempt_id)
        .bind(&request.worker_id)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn complete_attempt(&self, request: &CompletionRequest) -> Result<bool, StoreError> {
        if request.skipped && !request.succeeded {
            return Err(StoreError::Internal);
        }
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let status = if request.skipped {
            "skipped"
        } else if request.succeeded {
            "succeeded"
        } else {
            "failed"
        };
        let run_id = sqlx::query_scalar::<_, Uuid>(
            "UPDATE crono.run_attempts
                SET status = $3, completed_at = statement_timestamp(), lease_expires_at = NULL,
                    exit_code = $4, stdout_tail = $5, stderr_tail = $6, error = $7
              WHERE id = $1 AND worker_id = $2 AND status = 'running'
                AND lease_expires_at > statement_timestamp()
              RETURNING run_id",
        )
        .bind(request.attempt_id)
        .bind(&request.worker_id)
        .bind(status)
        .bind(request.exit_code)
        .bind(bounded_tail(&request.stdout_tail))
        .bind(bounded_tail(&request.stderr_tail))
        .bind(request.error.as_deref().map(bounded_error))
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?;
        let Some(run_id) = run_id else {
            transaction.rollback().await.map_err(store_error)?;
            return Ok(false);
        };
        let retry = sqlx::query_as::<_, RetryRow>(
            "SELECT attempt_count, max_attempts,
                    COALESCE((execution_snapshot->>'idempotent')::boolean, false) AS idempotent,
                    (execution_snapshot->>'retry_initial_seconds')::integer AS retry_initial_seconds,
                    (execution_snapshot->>'retry_max_seconds')::integer AS retry_max_seconds,
                    (execution_snapshot->>'retry_multiplier')::double precision AS retry_multiplier,
                    (execution_snapshot->>'retry_jitter')::double precision AS retry_jitter
               FROM crono.runs WHERE id = $1 FOR UPDATE",
        )
        .bind(run_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(store_error)?;
        let should_retry = !request.succeeded
            && !request.skipped
            && retry.idempotent
            && retry.attempt_count < retry.max_attempts;
        if request.skipped {
            crate::metrics::global().execution_skipped.inc();
        } else if request.succeeded {
            crate::metrics::global().execution_success.inc();
        } else {
            crate::metrics::global().execution_failure.inc();
            if should_retry {
                crate::metrics::global().execution_retry.inc();
            }
        }
        let run_status = if request.skipped {
            "skipped"
        } else if request.succeeded {
            "succeeded"
        } else if should_retry {
            "retry_wait"
        } else {
            "failed"
        };
        let next_retry_at =
            should_retry.then(|| OffsetDateTime::now_utc() + retry_delay(run_id, &retry));
        sqlx::query(
            "UPDATE crono.runs
                SET status = $2,
                    completed_at = CASE WHEN $2 = 'retry_wait' THEN NULL ELSE statement_timestamp() END,
                    next_retry_at = $3, terminal_reason = $4
              WHERE id = $1 AND status = 'running'",
        )
        .bind(run_id)
        .bind(run_status)
        .bind(next_retry_at)
        .bind(if request.skipped {
            Some("dry run".to_string())
        } else {
            request.error.as_deref().map(bounded_error)
        })
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        insert_run_event(
            &mut transaction,
            run_id,
            run_status,
            serde_json::json!({ "attempt_id": request.attempt_id }),
        )
        .await?;
        transaction.commit().await.map_err(store_error)?;
        Ok(true)
    }

    async fn reconcile(&self, limit: u16) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let expired_dispatches = reconcile_expired_dispatches(&mut transaction, limit).await?;
        let due_retries = reconcile_due_retries(&mut transaction, limit).await?;
        let expired_leases = reconcile_expired_leases(&mut transaction, limit).await?;
        let expired_workers = remove_expired_worker_presence(&mut transaction, limit).await?;
        sqlx::query(
            "UPDATE crono.outbox SET claimed_by = NULL, claim_expires_at = NULL
              WHERE published_at IS NULL AND cancelled_at IS NULL
                AND claim_expires_at < statement_timestamp()",
        )
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        transaction.commit().await.map_err(store_error)?;
        u64::try_from(
            expired_leases
                .saturating_add(due_retries)
                .saturating_add(expired_dispatches)
                .saturating_add(expired_workers),
        )
        .map_err(|_| StoreError::Internal)
    }

    async fn ready(&self) -> bool {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }

    async fn metrics_snapshot(&self) -> Result<MetricsSnapshot, StoreError> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            "SELECT
                (SELECT count(*) FROM crono.outbox
                  WHERE published_at IS NULL AND cancelled_at IS NULL),
                COALESCE((SELECT extract(epoch FROM statement_timestamp() - min(created_at))::bigint
                            FROM crono.outbox
                           WHERE published_at IS NULL AND cancelled_at IS NULL), 0),
                (SELECT count(*) FROM crono.runs WHERE status = 'queued'),
                (SELECT count(*) FROM crono.runs WHERE status = 'running'),
                (SELECT count(*) FROM crono.run_attempts
                  WHERE status = 'running' AND lease_expires_at > statement_timestamp())",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(MetricsSnapshot {
            outbox_pending: row.0,
            outbox_oldest_seconds: row.1,
            execution_queued: row.2,
            execution_running: row.3,
            worker_active: row.4,
        })
    }

    async fn monitor_snapshot(&self) -> Result<MonitorSnapshot, StoreError> {
        // The size function walks PostgreSQL's database files. Keep this
        // operator-only query bounded even when a dependency is struggling.
        tokio::time::timeout(Duration::from_secs(3), async {
            let metrics = self.metrics_snapshot().await?;
            let row = sqlx::query_as::<_, (i64, i64, i64, i64, Option<OffsetDateTime>, i64)>(
                "SELECT pg_database_size(current_database())::bigint,
                        COALESCE((SELECT numbackends::bigint FROM pg_stat_database
                                  WHERE datname = current_database()), 0),
                        (SELECT count(*) FROM crono.schedules
                          WHERE enabled = true AND next_run_at IS NOT NULL),
                        (SELECT count(*) FROM crono.schedules
                          WHERE enabled = true AND next_run_at <= statement_timestamp()),
                        (SELECT min(next_run_at) FROM crono.schedules
                          WHERE enabled = true AND next_run_at IS NOT NULL),
                        (SELECT count(*) FROM crono.worker_presence
                          WHERE last_seen_at >= statement_timestamp() - interval '30 seconds')",
            )
            .fetch_one(&self.pool)
            .await
            .map_err(store_error)?;
            let pool_connections = self.pool.size();
            let pool_idle_connections = u32::try_from(self.pool.num_idle())
                .map_err(|_| StoreError::Internal)?
                .min(pool_connections);
            Ok(MonitorSnapshot {
                database_size_bytes: row.0,
                database_connections: row.1,
                pool_connections,
                pool_idle_connections,
                pool_max_connections: MAX_POOL_CONNECTIONS,
                enabled_schedules: row.2,
                due_schedules: row.3,
                earliest_next_run_at: row.4,
                online_workers: row.5,
                metrics,
            })
        })
        .await
        .map_err(|_| StoreError::Unavailable)?
    }
}

async fn remove_expired_worker_presence(
    transaction: &mut Transaction<'_, Postgres>,
    limit: u16,
) -> Result<usize, StoreError> {
    let rows = sqlx::query_scalar::<_, String>(
        "WITH expired AS (
            SELECT worker_id
              FROM crono.worker_presence
             WHERE last_seen_at < statement_timestamp() - interval '7 days'
             ORDER BY last_seen_at
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         )
         DELETE FROM crono.worker_presence wp
          USING expired
          WHERE wp.worker_id = expired.worker_id
         RETURNING wp.worker_id",
    )
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(store_error)?;
    Ok(rows.len())
}

async fn reconcile_expired_dispatches(
    transaction: &mut Transaction<'_, Postgres>,
    limit: u16,
) -> Result<usize, StoreError> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, Uuid)>(
        "SELECT o.id, o.run_id, o.attempt_id
           FROM crono.outbox o
          WHERE o.published_at IS NULL AND o.cancelled_at IS NULL
            AND o.dispatch_deadline < statement_timestamp()
          ORDER BY o.dispatch_deadline, o.id
          FOR UPDATE SKIP LOCKED
          LIMIT $1",
    )
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(store_error)?;
    for (outbox_id, run_id, attempt_id) in &rows {
        sqlx::query(
            "UPDATE crono.outbox
                SET cancelled_at = statement_timestamp(),
                    claimed_by = NULL, claim_expires_at = NULL,
                    last_error = 'misfire dispatch deadline expired'
              WHERE id = $1 AND published_at IS NULL AND cancelled_at IS NULL",
        )
        .bind(outbox_id)
        .execute(&mut **transaction)
        .await
        .map_err(store_error)?;
        sqlx::query(
            "UPDATE crono.run_attempts
                SET status = 'dead', completed_at = statement_timestamp(),
                    error = 'misfire dispatch deadline expired'
              WHERE id = $1 AND status = 'pending_dispatch'",
        )
        .bind(attempt_id)
        .execute(&mut **transaction)
        .await
        .map_err(store_error)?;
        sqlx::query(
            "UPDATE crono.runs
                SET status = 'skipped', completed_at = statement_timestamp(),
                    terminal_reason = 'misfire dispatch deadline expired'
              WHERE id = $1 AND status = 'pending_dispatch'",
        )
        .bind(run_id)
        .execute(&mut **transaction)
        .await
        .map_err(store_error)?;
        insert_run_event(
            transaction,
            *run_id,
            "skipped",
            serde_json::json!({ "reason": "misfire dispatch deadline expired" }),
        )
        .await?;
    }
    Ok(rows.len())
}

async fn reconcile_due_retries(
    transaction: &mut Transaction<'_, Postgres>,
    limit: u16,
) -> Result<usize, StoreError> {
    let rows = sqlx::query_as::<_, (Uuid, i32, Uuid)>(
        "SELECT id, attempt_count, (execution_snapshot->>'queue_id')::uuid
           FROM crono.runs
          WHERE status = 'retry_wait' AND next_retry_at <= statement_timestamp()
          ORDER BY next_retry_at, id
          FOR UPDATE SKIP LOCKED
          LIMIT $1",
    )
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(store_error)?;
    for (run_id, attempt_count, queue_id) in &rows {
        let next_attempt = attempt_count + 1;
        sqlx::query(
            "UPDATE crono.runs SET status = 'pending_dispatch', attempt_count = $2,
                    next_retry_at = NULL, started_at = NULL
              WHERE id = $1 AND status = 'retry_wait'",
        )
        .bind(run_id)
        .bind(next_attempt)
        .execute(&mut **transaction)
        .await
        .map_err(store_error)?;
        create_attempt_and_outbox(transaction, *run_id, next_attempt, *queue_id, None).await?;
    }
    Ok(rows.len())
}

async fn reconcile_expired_leases(
    transaction: &mut Transaction<'_, Postgres>,
    limit: u16,
) -> Result<usize, StoreError> {
    let rows = sqlx::query_as::<_, (Uuid, Uuid, bool, i32, i32, Uuid)>(
        "SELECT a.id, a.run_id,
                COALESCE((r.execution_snapshot->>'idempotent')::boolean, false),
                r.attempt_count, r.max_attempts,
                (r.execution_snapshot->>'queue_id')::uuid
           FROM crono.run_attempts a
           JOIN crono.runs r ON r.id = a.run_id
          WHERE a.status = 'running' AND a.lease_expires_at < statement_timestamp()
          ORDER BY a.lease_expires_at, a.id
          FOR UPDATE OF a, r SKIP LOCKED
          LIMIT $1",
    )
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(store_error)?;
    for (attempt_id, run_id, idempotent, attempt_count, max_attempts, queue_id) in &rows {
        crate::metrics::global().worker_lease_expired.inc();
        let retry = *idempotent && attempt_count < max_attempts;
        sqlx::query(
            "UPDATE crono.run_attempts SET status = $2, completed_at = statement_timestamp(),
                    lease_expires_at = NULL, error = 'worker lease expired'
              WHERE id = $1 AND status = 'running'",
        )
        .bind(attempt_id)
        .bind(if retry { "failed" } else { "unknown" })
        .execute(&mut **transaction)
        .await
        .map_err(store_error)?;
        if retry {
            let next_attempt = attempt_count + 1;
            sqlx::query(
                "UPDATE crono.runs SET status = 'pending_dispatch', attempt_count = $2,
                        next_retry_at = NULL, started_at = NULL
                  WHERE id = $1 AND status = 'running'",
            )
            .bind(run_id)
            .bind(next_attempt)
            .execute(&mut **transaction)
            .await
            .map_err(store_error)?;
            create_attempt_and_outbox(transaction, *run_id, next_attempt, *queue_id, None).await?;
        } else {
            sqlx::query(
                "UPDATE crono.runs SET status = 'unknown',
                        terminal_reason = 'worker lease expired after an ambiguous execution',
                        completed_at = statement_timestamp()
                  WHERE id = $1 AND status = 'running'",
            )
            .bind(run_id)
            .execute(&mut **transaction)
            .await
            .map_err(store_error)?;
        }
    }
    Ok(rows.len())
}

async fn load_execution_ids(
    transaction: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
    target_id: Uuid,
) -> Result<ExecutionRow, StoreError> {
    sqlx::query_as::<_, ExecutionRow>(
        "SELECT j.namespace_id, j.id AS job_id, t.id AS target_id, j.executor, j.queue_id,
                q.name AS queue_name, j.executable,
                j.arguments AS job_arguments, t.arguments AS target_arguments,
                j.inputs AS job_inputs, t.inputs AS target_inputs,
                j.idempotent, j.dry_run, j.max_attempts, j.retry_initial_seconds,
                j.retry_max_seconds, j.retry_multiplier, j.retry_jitter
           FROM crono.jobs j
           JOIN crono.queues q ON q.id = j.queue_id
           JOIN crono.targets t ON t.id = $2 AND t.namespace_id = j.namespace_id
          WHERE j.id = $1",
    )
    .bind(job_id)
    .bind(target_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(store_error)?
    .ok_or(StoreError::NotFound)
}

async fn load_selection_executions(
    transaction: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
    target: TargetSelection,
) -> Result<(Vec<ExecutionRow>, Option<serde_json::Value>), StoreError> {
    match target {
        TargetSelection::Target(target_id) => Ok((
            vec![load_execution_ids(transaction, job_id, target_id.get()).await?],
            None,
        )),
        TargetSelection::TargetSet(target_set_id) => {
            let set = sqlx::query_as::<_, (serde_json::Value,)>(
                "SELECT ts.inputs
                   FROM crono.target_sets ts
                   JOIN crono.jobs j ON j.id = $1 AND j.namespace_id = ts.namespace_id
                  WHERE ts.id = $2
                  FOR SHARE OF ts",
            )
            .bind(job_id)
            .bind(target_set_id.get())
            .fetch_optional(&mut **transaction)
            .await
            .map_err(store_error)?
            .ok_or(StoreError::NotFound)?;
            let target_ids = sqlx::query_scalar::<_, Uuid>(
                "SELECT member.target_id
                   FROM crono.target_set_members member
                   JOIN crono.targets t ON t.id = member.target_id
                  WHERE member.target_set_id = $1
                  ORDER BY t.name, t.id
                  FOR SHARE OF member",
            )
            .bind(target_set_id.get())
            .fetch_all(&mut **transaction)
            .await
            .map_err(store_error)?;
            if target_ids.is_empty() {
                return Err(StoreError::NotFound);
            }
            let mut executions = Vec::with_capacity(target_ids.len());
            for target_id in target_ids {
                executions.push(load_execution_ids(transaction, job_id, target_id).await?);
            }
            Ok((executions, Some(set.0)))
        }
    }
}

fn selection_ids(target: TargetSelection) -> (Option<Uuid>, Option<Uuid>) {
    match target {
        TargetSelection::Target(id) => (Some(id.get()), None),
        TargetSelection::TargetSet(id) => (None, Some(id.get())),
    }
}

fn target_selection(
    target_id: Option<Uuid>,
    target_set_id: Option<Uuid>,
) -> Result<TargetSelection, StoreError> {
    match (target_id, target_set_id) {
        (Some(id), None) => Ok(TargetSelection::Target(TargetId::new(id))),
        (None, Some(id)) => Ok(TargetSelection::TargetSet(TargetSetId::new(id))),
        _ => Err(StoreError::Internal),
    }
}

fn execution_snapshot_with_inputs(
    run_id: Uuid,
    execution: &ExecutionRow,
    target_set_inputs: Option<&serde_json::Value>,
    invocation_inputs: &serde_json::Value,
    trigger: ExecutionTrigger,
    scheduled_at: Option<OffsetDateTime>,
) -> Result<serde_json::Value, String> {
    let mut arguments: Vec<String> = serde_json::from_value(execution.job_arguments.clone())
        .map_err(|error| error.to_string())?;
    let target_arguments: Vec<String> = serde_json::from_value(execution.target_arguments.clone())
        .map_err(|error| error.to_string())?;
    arguments.extend(target_arguments);
    let empty = serde_json::json!({});
    let inputs = merge_inputs(&[
        &execution.job_inputs,
        target_set_inputs.unwrap_or(&empty),
        &execution.target_inputs,
        invocation_inputs,
    ])
    .map_err(|error| error.to_string())?;
    let rendered_arguments =
        render_arguments(&arguments, &inputs).map_err(|error| error.to_string())?;
    let executor = match execution.executor.as_str() {
        "noop" => ApiExecutor::Noop,
        "process" => ApiExecutor::Process,
        value => {
            error!(executor = value, "unsupported persisted executor");
            return Err("unsupported persisted executor".to_owned());
        }
    };
    serde_json::to_value(ExecutionSnapshot {
        executor,
        executable: execution.executable.clone(),
        argument_templates: arguments,
        arguments: rendered_arguments,
        inputs,
        job_id: Some(execution.job_id),
        trigger: Some(trigger),
        scheduled_at,
        idempotency_key: run_id,
        queue_id: execution.queue_id,
        queue: execution.queue_name.clone(),
        idempotent: execution.idempotent,
        dry_run: execution.dry_run,
        retry_initial_seconds: u32::try_from(execution.retry_initial_seconds)
            .map_err(|_| "invalid persisted retry delay".to_owned())?,
        retry_max_seconds: u32::try_from(execution.retry_max_seconds)
            .map_err(|_| "invalid persisted maximum retry delay".to_owned())?,
        retry_multiplier: execution.retry_multiplier,
        retry_jitter: execution.retry_jitter,
    })
    .map_err(|error| error.to_string())
}

async fn create_attempt_and_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    attempt: i32,
    queue_id: Uuid,
    dispatch_deadline: Option<OffsetDateTime>,
) -> Result<(), StoreError> {
    let attempt_id = Uuid::now_v7();
    let dispatch_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO crono.run_attempts (id, run_id, attempt)
         VALUES ($1, $2, $3)",
    )
    .bind(attempt_id)
    .bind(run_id)
    .bind(attempt)
    .execute(&mut **transaction)
    .await
    .map_err(store_error)?;
    let envelope = DispatchEnvelope {
        dispatch_id,
        run_id,
        attempt_id,
        queue_id,
    };
    let payload = serde_json::to_value(envelope).map_err(json_error)?;
    sqlx::query(
        "INSERT INTO crono.outbox (
             id, run_id, attempt_id, subject, payload, dispatch_deadline
         ) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(dispatch_id)
    .bind(run_id)
    .bind(attempt_id)
    .bind(format!("crono.dispatch.{queue_id}"))
    .bind(payload)
    .bind(dispatch_deadline)
    .execute(&mut **transaction)
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn insert_scheduled_occurrence(
    transaction: &mut Transaction<'_, Postgres>,
    schedule_id: ScheduleId,
    occurrence: &crate::application::PlannedOccurrence,
    execution: &ExecutionRow,
    target_set_inputs: Option<&serde_json::Value>,
    schedule_inputs: &serde_json::Value,
) -> Result<(), StoreError> {
    let run_id = Uuid::now_v7();
    let empty_inputs = serde_json::json!({});
    let scheduled_at = occurrence.scheduled_at;
    let rendered = scheduled_snapshot(
        run_id,
        execution,
        target_set_inputs,
        schedule_inputs,
        scheduled_at,
    );
    let (snapshot, status, reason) = match (occurrence.execute, rendered) {
        (true, Ok(snapshot)) => (snapshot, "pending_dispatch", None),
        (false, Ok(snapshot)) => (snapshot, "skipped", occurrence.reason.clone()),
        (_, Err(message)) => (
            serde_json::json!({
                "render_error": &message,
                "inputs": merge_inputs(&[
                    &execution.job_inputs,
                    target_set_inputs.unwrap_or(&empty_inputs),
                    &execution.target_inputs,
                    schedule_inputs,
                ]).unwrap_or_else(|_| serde_json::json!({})),
            }),
            "failed",
            Some(format!(
                "execution template could not be rendered: {message}"
            )),
        ),
    };
    let attempt_count = i32::from(status == "pending_dispatch");
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO crono.runs (
             id, schedule_id, job_id, target_id, queue_id, scheduled_at, status,
             execution_snapshot, attempt_count, max_attempts, lateness_seconds,
             terminal_reason, completed_at
         ) VALUES (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
             CASE WHEN $7 IN ('skipped', 'failed') THEN statement_timestamp() ELSE NULL END
         )
         ON CONFLICT (schedule_id, scheduled_at, target_id)
             WHERE schedule_id IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(run_id)
    .bind(schedule_id.get())
    .bind(execution.job_id)
    .bind(execution.target_id)
    .bind(execution.queue_id)
    .bind(occurrence.scheduled_at)
    .bind(status)
    .bind(snapshot)
    .bind(attempt_count)
    .bind(execution.max_attempts)
    .bind(i64::try_from(occurrence.lateness_seconds).unwrap_or(i64::MAX))
    .bind(&reason)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(store_error)?;
    if inserted.is_none() {
        return Ok(());
    }
    if status == "pending_dispatch" {
        let deadline = occurrence.dispatch_deadline;
        create_attempt_and_outbox(transaction, run_id, 1, execution.queue_id, deadline).await?;
    }
    let event = match status {
        "pending_dispatch" => "created",
        "failed" => "failed",
        _ => "skipped",
    };
    insert_run_event(
        transaction,
        run_id,
        event,
        serde_json::json!({ "lateness_seconds": occurrence.lateness_seconds }),
    )
    .await?;
    sqlx::query(
        "INSERT INTO crono.schedule_events (schedule_id, event_type, scheduled_at, detail)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(schedule_id.get())
    .bind(event)
    .bind(occurrence.scheduled_at)
    .bind(serde_json::json!({
        "run_id": run_id,
        "lateness_seconds": occurrence.lateness_seconds,
        "reason": occurrence.reason,
        "target_id": execution.target_id,
        "terminal_reason": reason,
    }))
    .execute(&mut **transaction)
    .await
    .map_err(store_error)?;
    Ok(())
}

/// Preserve the schedule origin in an immutable execution snapshot.
fn scheduled_snapshot(
    run_id: Uuid,
    execution: &ExecutionRow,
    target_set_inputs: Option<&serde_json::Value>,
    schedule_inputs: &serde_json::Value,
    scheduled_at: OffsetDateTime,
) -> Result<serde_json::Value, String> {
    execution_snapshot_with_inputs(
        run_id,
        execution,
        target_set_inputs,
        schedule_inputs,
        ExecutionTrigger::Schedule,
        Some(scheduled_at),
    )
}

async fn insert_run_event(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    event_type: &str,
    detail: serde_json::Value,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO crono.run_events (run_id, event_type, detail)
         VALUES ($1, $2, $3)",
    )
    .bind(run_id)
    .bind(event_type)
    .bind(detail)
    .execute(&mut **transaction)
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn find_run_request(
    pool: &PgPool,
    request_id: Uuid,
) -> Result<Option<RunRequestRow>, StoreError> {
    sqlx::query_as::<_, RunRequestRow>(
        "SELECT job_id, target_id, target_set_id, inputs
           FROM crono.run_requests
          WHERE request_id = $1",
    )
    .bind(request_id)
    .fetch_optional(pool)
    .await
    .map_err(store_error)
}

async fn runs_for_request(pool: &PgPool, request_id: Uuid) -> Result<Vec<RunRecord>, StoreError> {
    sqlx::query_as::<_, RunRow>(
        "SELECT r.id, r.request_id, r.schedule_id, r.job_id, r.target_id, r.status,
                r.scheduled_at, r.created_at, r.queued_at, r.started_at, r.completed_at,
                r.attempt_count, r.max_attempts, r.lateness_seconds, r.terminal_reason,
                jn.name AS job_namespace, j.name AS job_name,
                tn.name AS target_namespace, t.name AS target_name
         FROM crono.runs r
         JOIN crono.jobs j ON j.id = r.job_id
         JOIN crono.namespaces jn ON jn.id = j.namespace_id
         JOIN crono.targets t ON t.id = r.target_id
         JOIN crono.namespaces tn ON tn.id = t.namespace_id
         WHERE r.request_id = $1
         ORDER BY t.name, r.id",
    )
    .bind(request_id)
    .fetch_all(pool)
    .await
    .map_err(store_error)?
    .into_iter()
    .map(run_from_row)
    .collect()
}

fn compare_run_request(
    existing: &RunRequestRow,
    job_id: JobId,
    target: TargetSelection,
    inputs: &serde_json::Value,
) -> Result<(), StoreError> {
    let (target_id, target_set_id) = selection_ids(target);
    if existing.job_id == job_id.get()
        && existing.target_id == target_id
        && existing.target_set_id == target_set_id
        && existing.inputs == *inputs
    {
        return Ok(());
    }
    Err(StoreError::IdempotencyConflict)
}

fn namespace_from_row(
    (id, name, created_at): (Uuid, String, OffsetDateTime),
) -> Result<Namespace, StoreError> {
    Ok(Namespace::new(
        NamespaceId::new(id),
        NamespaceName::parse(&name).map_err(invalid_database_name)?,
        created_at,
    ))
}

fn queue_from_row(row: QueueRow) -> Result<Queue, StoreError> {
    Ok(Queue::new(
        QueueId::new(row.id),
        QueueName::parse(&row.name).map_err(invalid_database_name)?,
        row.description,
        row.enabled,
        row.system,
        row.created_at,
        row.updated_at,
    ))
}

fn job_from_row(row: JobRow) -> Result<JobRecord, StoreError> {
    let executor = parse_executor(&row.executor)?;
    let arguments = serde_json::from_value(row.arguments).map_err(json_error)?;
    let max_attempts = u16::try_from(row.max_attempts).map_err(|_| StoreError::Internal)?;
    let retry_initial_seconds =
        u32::try_from(row.retry_initial_seconds).map_err(|_| StoreError::Internal)?;
    let retry_max_seconds =
        u32::try_from(row.retry_max_seconds).map_err(|_| StoreError::Internal)?;
    Ok(JobRecord {
        namespace: NamespaceName::parse(&row.namespace_name).map_err(invalid_database_name)?,
        queue_name: QueueName::parse(&row.queue_name).map_err(invalid_database_name)?,
        job: Job::new(JobData {
            id: JobId::new(row.id),
            namespace_id: NamespaceId::new(row.namespace_id),
            name: ResourceName::parse(&row.name).map_err(invalid_database_name)?,
            executor,
            queue_id: QueueId::new(row.queue_id),
            executable: row.executable,
            arguments,
            inputs: row.inputs,
            idempotent: row.idempotent,
            dry_run: row.dry_run,
            max_attempts,
            retry_initial_seconds,
            retry_max_seconds,
            retry_multiplier: row.retry_multiplier,
            retry_jitter: row.retry_jitter,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }),
    })
}

fn target_from_row(row: TargetRow) -> Result<TargetRecord, StoreError> {
    let arguments = serde_json::from_value(row.arguments).map_err(json_error)?;
    Ok(TargetRecord {
        namespace: NamespaceName::parse(&row.namespace_name).map_err(invalid_database_name)?,
        target: Target::new(
            TargetId::new(row.id),
            NamespaceId::new(row.namespace_id),
            ResourceName::parse(&row.name).map_err(invalid_database_name)?,
            arguments,
            row.inputs,
            row.created_at,
            row.updated_at,
        ),
    })
}

async fn target_rows_for_ids(
    transaction: &mut Transaction<'_, Postgres>,
    namespace_id: NamespaceId,
    ids: &[Uuid],
) -> Result<Vec<TargetRow>, StoreError> {
    sqlx::query_as::<_, TargetRow>(
        "SELECT t.id, t.namespace_id, t.name, t.arguments, t.inputs, t.created_at, t.updated_at,
                n.name AS namespace_name
           FROM crono.targets t
           JOIN crono.namespaces n ON n.id = t.namespace_id
          WHERE t.namespace_id = $1 AND t.id = ANY($2::uuid[])
          ORDER BY t.name",
    )
    .bind(namespace_id.get())
    .bind(ids)
    .fetch_all(&mut **transaction)
    .await
    .map_err(store_error)
}

async fn load_target_set_record(
    pool: &PgPool,
    row: TargetSetRow,
) -> Result<TargetSetRecord, StoreError> {
    let targets = sqlx::query_as::<_, TargetRow>(
        "SELECT t.id, t.namespace_id, t.name, t.arguments, t.inputs, t.created_at, t.updated_at,
                n.name AS namespace_name
           FROM crono.target_set_members member
           JOIN crono.targets t ON t.id = member.target_id
           JOIN crono.namespaces n ON n.id = t.namespace_id
          WHERE member.target_set_id = $1
          ORDER BY t.name",
    )
    .bind(row.id)
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    target_set_from_rows(&row, targets)
}

fn target_set_from_rows(
    row: &TargetSetRow,
    target_rows: Vec<TargetRow>,
) -> Result<TargetSetRecord, StoreError> {
    let namespace_id = NamespaceId::new(row.namespace_id);
    let mut target_set = TargetSet::new(
        TargetSetId::new(row.id),
        namespace_id,
        ResourceName::parse(&row.name).map_err(invalid_database_name)?,
        row.inputs.clone(),
        row.created_at,
        row.updated_at,
    );
    let mut targets = Vec::with_capacity(target_rows.len());
    for target_row in target_rows {
        let record = target_from_row(target_row)?;
        target_set
            .add_target(&record.target)
            .map_err(|_| StoreError::Internal)?;
        targets.push(record.target);
    }
    Ok(TargetSetRecord {
        namespace: NamespaceName::parse(&row.namespace_name).map_err(invalid_database_name)?,
        target_set,
        targets,
    })
}

fn schedule_entity(row: &ScheduleRow) -> Result<Schedule, StoreError> {
    let timing = match row.schedule_type.as_str() {
        "cron" => ScheduleTiming::Cron {
            expression: row.cron_expression.clone().ok_or(StoreError::Internal)?,
            timezone: row.timezone.clone(),
        },
        "once" => ScheduleTiming::Once {
            execute_at: row.execute_at.ok_or(StoreError::Internal)?,
        },
        _ => return Err(StoreError::Internal),
    };
    Ok(Schedule {
        id: ScheduleId::new(row.id),
        namespace_id: NamespaceId::new(row.namespace_id),
        name: ResourceName::parse(&row.name).map_err(invalid_database_name)?,
        job_id: JobId::new(row.job_id),
        target: target_selection(row.target_id, row.target_set_id)?,
        inputs: row.inputs.clone(),
        timing,
        enabled: row.enabled,
        next_run_at: row.next_run_at,
        last_run_at: row.last_run_at,
        misfire_policy: parse_misfire(&row.misfire_policy)?,
        misfire_grace_seconds: row
            .misfire_grace_seconds
            .map(u32::try_from)
            .transpose()
            .map_err(|_| StoreError::Internal)?,
        catchup_policy: parse_catchup(&row.catchup_policy)?,
        max_catchup_runs: u16::try_from(row.max_catchup_runs).map_err(|_| StoreError::Internal)?,
        max_catchup_age_seconds: u32::try_from(row.max_catchup_age_seconds)
            .map_err(|_| StoreError::Internal)?,
        revision: u64::try_from(row.revision).map_err(|_| StoreError::Internal)?,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn schedule_from_row(row: &ScheduleRow) -> Result<ScheduleRecord, StoreError> {
    let namespace = row.namespace_name.as_deref().unwrap_or("unknown");
    let job_name = row.job_name.as_deref().unwrap_or("unknown");
    let target_name = row.target_name.as_deref().unwrap_or("unknown");
    let schedule = schedule_entity(row)?;
    Ok(ScheduleRecord {
        schedule,
        namespace: NamespaceName::parse(namespace).map_err(invalid_database_name)?,
        job_name: ResourceName::parse(job_name).map_err(invalid_database_name)?,
        target_name: ResourceName::parse(target_name).map_err(invalid_database_name)?,
    })
}

fn run_from_row(row: RunRow) -> Result<RunRecord, StoreError> {
    let status = parse_run_status(&row.status)?;
    Ok(RunRecord {
        run: Run::new(RunData {
            id: RunId::new(row.id),
            request_id: row.request_id,
            schedule_id: row.schedule_id.map(ScheduleId::new),
            job_id: JobId::new(row.job_id),
            target_id: TargetId::new(row.target_id),
            status,
            scheduled_at: row.scheduled_at,
            created_at: row.created_at,
            queued_at: row.queued_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
            attempt_count: u16::try_from(row.attempt_count).map_err(|_| StoreError::Internal)?,
            max_attempts: u16::try_from(row.max_attempts).map_err(|_| StoreError::Internal)?,
            lateness_seconds: u64::try_from(row.lateness_seconds)
                .map_err(|_| StoreError::Internal)?,
            terminal_reason: row.terminal_reason,
        }),
        job_namespace: NamespaceName::parse(&row.job_namespace).map_err(invalid_database_name)?,
        job_name: ResourceName::parse(&row.job_name).map_err(invalid_database_name)?,
        target_namespace: NamespaceName::parse(&row.target_namespace)
            .map_err(invalid_database_name)?,
        target_name: ResourceName::parse(&row.target_name).map_err(invalid_database_name)?,
    })
}

fn worker_from_row(row: WorkerRow) -> Result<WorkerRecord, StoreError> {
    Ok(WorkerRecord {
        worker_id: row.worker_id,
        queue_id: QueueId::new(row.queue_id),
        queue: row.queue,
        concurrency: u16::try_from(row.concurrency).map_err(|_| StoreError::Internal)?,
        version: row.version,
        started_at: row.started_at,
        last_seen_at: row.last_seen_at,
        active_executions: u64::try_from(row.active_executions)
            .map_err(|_| StoreError::Internal)?,
    })
}

fn parse_run_status(value: &str) -> Result<RunStatus, StoreError> {
    match value {
        "pending_dispatch" => Ok(RunStatus::PendingDispatch),
        "queued" => Ok(RunStatus::Queued),
        "running" => Ok(RunStatus::Running),
        "retry_wait" => Ok(RunStatus::RetryWait),
        "succeeded" => Ok(RunStatus::Succeeded),
        "failed" => Ok(RunStatus::Failed),
        "dead" => Ok(RunStatus::Dead),
        "skipped" => Ok(RunStatus::Skipped),
        "cancelled" => Ok(RunStatus::Cancelled),
        "unknown" => Ok(RunStatus::Unknown),
        other => {
            error!(status = other, "unsupported persisted Run status");
            Err(StoreError::Internal)
        }
    }
}

fn parse_executor(value: &str) -> Result<ExecutorKind, StoreError> {
    match value {
        "noop" => Ok(ExecutorKind::Noop),
        "process" => Ok(ExecutorKind::Process),
        _ => Err(StoreError::Internal),
    }
}

const fn executor_name(value: ExecutorKind) -> &'static str {
    match value {
        ExecutorKind::Noop => "noop",
        ExecutorKind::Process => "process",
    }
}

const fn misfire_name(value: MisfirePolicy) -> &'static str {
    match value {
        MisfirePolicy::RunLate => "run_late",
        MisfirePolicy::Skip => "skip",
        MisfirePolicy::GracePeriod => "grace_period",
    }
}

fn parse_misfire(value: &str) -> Result<MisfirePolicy, StoreError> {
    match value {
        "run_late" => Ok(MisfirePolicy::RunLate),
        "skip" => Ok(MisfirePolicy::Skip),
        "grace_period" => Ok(MisfirePolicy::GracePeriod),
        _ => Err(StoreError::Internal),
    }
}

const fn catchup_name(value: CatchupPolicy) -> &'static str {
    match value {
        CatchupPolicy::Skip => "skip",
        CatchupPolicy::RunOnce => "run_once",
        CatchupPolicy::CatchUp => "catch_up",
    }
}

fn parse_catchup(value: &str) -> Result<CatchupPolicy, StoreError> {
    match value {
        "skip" => Ok(CatchupPolicy::Skip),
        "run_once" => Ok(CatchupPolicy::RunOnce),
        "catch_up" => Ok(CatchupPolicy::CatchUp),
        _ => Err(StoreError::Internal),
    }
}

fn page<R, T, F, C>(
    mut rows: Vec<R>,
    limit: u16,
    mapper: F,
    cursor: C,
) -> Result<Page<T>, StoreError>
where
    F: Fn(R) -> Result<T, StoreError>,
    C: Fn(&T) -> String,
{
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(mapper)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if has_more {
        items.last().map(cursor)
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}

const fn empty_page<T>() -> Page<T> {
    Page {
        items: Vec::new(),
        next_cursor: None,
    }
}

fn count(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Internal)
}

fn duration_seconds(value: Duration) -> Result<i32, StoreError> {
    i32::try_from(value.as_secs()).map_err(|_| StoreError::Internal)
}

fn retry_delay(run_id: Uuid, retry: &RetryRow) -> time::Duration {
    let exponent = retry.attempt_count.saturating_sub(1);
    let initial = f64::from(retry.retry_initial_seconds);
    let maximum = f64::from(retry.retry_max_seconds);
    let base = (initial * retry.retry_multiplier.powi(exponent)).min(maximum);
    let seed = run_id.as_bytes().last().copied().unwrap_or(128);
    let unit = (f64::from(seed) / 255.0).mul_add(2.0, -1.0);
    time::Duration::seconds_f64(base.mul_add(retry.retry_jitter * unit, base))
}

fn bounded_tail(value: &str) -> String {
    const LIMIT: usize = 65_536;

    if value.len() <= LIMIT {
        return value.to_owned();
    }
    let mut start = value.len() - LIMIT;
    while !value.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    value.get(start..).unwrap_or_default().to_owned()
}

fn bounded_error(value: &str) -> String {
    value.chars().take(1024).collect()
}

fn invalid_database_name(error: crate::domain::NameError) -> StoreError {
    error!(%error, "database contains an invalid canonical name");
    StoreError::Internal
}

fn json_error(error: serde_json::Error) -> StoreError {
    error!(%error, "database JSON contract is invalid");
    drop(error);
    StoreError::Internal
}

fn store_error(error: sqlx::Error) -> StoreError {
    if is_unique_violation(&error) {
        return StoreError::Conflict;
    }
    if is_foreign_key_violation(&error) {
        return StoreError::InUse;
    }
    match error {
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_) => {
            StoreError::Unavailable
        }
        other => {
            error!(error = %other, "PostgreSQL operation failed");
            StoreError::Internal
        }
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| database.code().as_deref() == Some("23505"))
}

fn is_foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| matches!(database.code().as_deref(), Some("23001" | "23503")))
}

#[cfg(test)]
mod dry_run_snapshot_tests {
    use super::{ExecutionRow, scheduled_snapshot};
    use anyhow::Result;
    use crono_api::{ExecutionSnapshot, ExecutionTrigger};
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn scheduled_snapshots_keep_job_dry_run_setting() -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let execution = ExecutionRow {
            namespace_id: Uuid::now_v7(),
            job_id: Uuid::now_v7(),
            target_id: Uuid::now_v7(),
            executor: "process".to_string(),
            queue_id: Uuid::now_v7(),
            queue_name: "default".to_string(),
            executable: Some("/bin/echo".to_string()),
            job_arguments: serde_json::json!(["hello {{ name }}"]),
            target_arguments: serde_json::json!([]),
            job_inputs: serde_json::json!({"name": "world"}),
            target_inputs: serde_json::json!({}),
            idempotent: false,
            dry_run: true,
            max_attempts: 1,
            retry_initial_seconds: 1,
            retry_max_seconds: 60,
            retry_multiplier: 2.0,
            retry_jitter: 0.2,
        };
        let snapshot = scheduled_snapshot(
            Uuid::now_v7(),
            &execution,
            None,
            &serde_json::json!({}),
            now,
        )
        .map_err(anyhow::Error::msg)?;
        let snapshot: ExecutionSnapshot = serde_json::from_value(snapshot)?;
        assert!(snapshot.dry_run);
        assert_eq!(snapshot.arguments, ["hello world"]);
        assert_eq!(snapshot.trigger, Some(ExecutionTrigger::Schedule));
        assert_eq!(snapshot.scheduled_at, Some(now));
        Ok(())
    }
}
