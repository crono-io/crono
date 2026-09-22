//! PostgreSQL implementation of the control-plane store.
//!
//! Catalog writes preserve normalized Namespace relationships and create the
//! first immutable no-op Job version in one transaction. Run creation pins a
//! Job version and Target, then commits its `JetStream` payload to the outbox in
//! that same transaction. The asynchronous dispatcher is the only component
//! that changes a committed Run from `pending_dispatch` to `dispatched`.

use crate::{
    application::{
        ControlPlaneStore, JobRecord, OutboxRecord, Overview, Page, RunRecord, StoreError,
        TargetRecord, VisibilityScope,
    },
    domain::{
        DispatchId, ExecutorKind, Job, JobId, JobVersion, JobVersionId, Namespace, NamespaceId,
        NamespaceName, QueueName, ResourceName, Run, RunId, RunStatus, Target, TargetId,
    },
};
use async_trait::async_trait;
use crono_api::DispatchEnvelope;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::time::Duration;
use time::OffsetDateTime;
use tracing::error;
use uuid::Uuid;

type NamespaceRow = (Uuid, String, OffsetDateTime);
type JobRow = (
    Uuid,
    Uuid,
    String,
    OffsetDateTime,
    String,
    Uuid,
    i32,
    String,
    String,
    OffsetDateTime,
);
type TargetRow = (Uuid, Uuid, String, OffsetDateTime, String);
type RunRow = (
    Uuid,
    Uuid,
    Uuid,
    Uuid,
    String,
    OffsetDateTime,
    Option<OffsetDateTime>,
    String,
    String,
    String,
    String,
);

/// SQL-backed implementation shared by HTTP requests and the outbox worker.
#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    /// Connect a bounded pool to an already initialized Crono database.
    ///
    /// # Errors
    ///
    /// Returns a safe availability error when PostgreSQL cannot be reached.
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
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
        let row = sqlx::query_as::<_, NamespaceRow>(
            "INSERT INTO crono.namespaces (name) VALUES ($1) RETURNING id, name, created_at",
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
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let namespace_ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, NamespaceRow>(
            "SELECT id, name, created_at FROM crono.namespaces
             WHERE ($1::text IS NULL OR name > $1)
               AND (NOT $2 OR id = ANY($3::uuid[]))
             ORDER BY name LIMIT $4",
        )
        .bind(after)
        .bind(restrict)
        .bind(&namespace_ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        namespace_page(rows, limit)
    }

    async fn get_namespace(&self, name: &NamespaceName) -> Result<Namespace, StoreError> {
        let row = sqlx::query_as::<_, NamespaceRow>(
            "SELECT id, name, created_at FROM crono.namespaces WHERE name = $1",
        )
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        namespace_from_row(row)
    }

    async fn create_job(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
        queue: &QueueName,
    ) -> Result<JobRecord, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let namespace_id = namespace_id(&mut transaction, namespace).await?;
        let job = sqlx::query_as::<_, (Uuid, OffsetDateTime)>(
            "INSERT INTO crono.jobs (namespace_id, name) VALUES ($1, $2)
             RETURNING id, created_at",
        )
        .bind(namespace_id)
        .bind(name.as_str())
        .fetch_one(&mut *transaction)
        .await
        .map_err(store_error)?;
        let version = sqlx::query_as::<_, (Uuid, OffsetDateTime)>(
            "INSERT INTO crono.job_versions (job_id, version, executor, queue)
             VALUES ($1, 1, 'noop', $2) RETURNING id, created_at",
        )
        .bind(job.0)
        .bind(queue.as_str())
        .fetch_one(&mut *transaction)
        .await
        .map_err(store_error)?;
        transaction.commit().await.map_err(store_error)?;
        Ok(JobRecord {
            namespace: namespace.clone(),
            job: Job::new(
                JobId::new(job.0),
                NamespaceId::new(namespace_id),
                name.clone(),
                job.1,
            ),
            version: JobVersion::new(
                JobVersionId::new(version.0),
                JobId::new(job.0),
                1,
                ExecutorKind::Noop,
                queue.clone(),
                version.1,
            ),
        })
    }

    async fn list_jobs(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let namespace_ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, JobRow>(
            "SELECT j.id, j.namespace_id, j.name, j.created_at, n.name,
                    v.id, v.version, v.executor, v.queue, v.created_at
             FROM crono.jobs j
             JOIN crono.namespaces n ON n.id = j.namespace_id
             JOIN LATERAL (
                 SELECT id, version, executor, queue, created_at
                 FROM crono.job_versions WHERE job_id = j.id
                 ORDER BY version DESC LIMIT 1
             ) v ON true
             WHERE n.name = $1 AND ($2::text IS NULL OR j.name > $2)
               AND (NOT $3 OR n.id = ANY($4::uuid[]))
             ORDER BY j.name LIMIT $5",
        )
        .bind(namespace.as_str())
        .bind(after)
        .bind(restrict)
        .bind(&namespace_ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        job_page(rows, limit)
    }

    async fn get_job(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<JobRecord, StoreError> {
        let row = sqlx::query_as::<_, JobRow>(
            "SELECT j.id, j.namespace_id, j.name, j.created_at, n.name,
                    v.id, v.version, v.executor, v.queue, v.created_at
             FROM crono.jobs j
             JOIN crono.namespaces n ON n.id = j.namespace_id
             JOIN LATERAL (
                 SELECT id, version, executor, queue, created_at
                 FROM crono.job_versions WHERE job_id = j.id
                 ORDER BY version DESC LIMIT 1
             ) v ON true
             WHERE n.name = $1 AND j.name = $2",
        )
        .bind(namespace.as_str())
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        job_from_row(row)
    }

    async fn create_target(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<TargetRecord, StoreError> {
        let row = sqlx::query_as::<_, TargetRow>(
            "INSERT INTO crono.targets (namespace_id, name)
             SELECT id, $2 FROM crono.namespaces WHERE name = $1
             RETURNING id, namespace_id, name, created_at, $1",
        )
        .bind(namespace.as_str())
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        target_from_row(row)
    }

    async fn list_targets(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let namespace_ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, TargetRow>(
            "SELECT t.id, t.namespace_id, t.name, t.created_at, n.name
             FROM crono.targets t JOIN crono.namespaces n ON n.id = t.namespace_id
             WHERE n.name = $1 AND ($2::text IS NULL OR t.name > $2)
               AND (NOT $3 OR n.id = ANY($4::uuid[]))
             ORDER BY t.name LIMIT $5",
        )
        .bind(namespace.as_str())
        .bind(after)
        .bind(restrict)
        .bind(&namespace_ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        target_page(rows, limit)
    }

    async fn get_target(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<TargetRecord, StoreError> {
        let row = sqlx::query_as::<_, TargetRow>(
            "SELECT t.id, t.namespace_id, t.name, t.created_at, n.name
             FROM crono.targets t JOIN crono.namespaces n ON n.id = t.namespace_id
             WHERE n.name = $1 AND t.name = $2",
        )
        .bind(namespace.as_str())
        .bind(name.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        target_from_row(row)
    }

    async fn create_run(
        &self,
        request_id: Uuid,
        job_namespace: &NamespaceName,
        job_name: &ResourceName,
        target_namespace: &NamespaceName,
        target_name: &ResourceName,
    ) -> Result<(RunRecord, bool), StoreError> {
        if let Some(existing) = find_run_by_request(&self.pool, request_id).await? {
            return compare_idempotent(
                existing,
                job_namespace,
                job_name,
                target_namespace,
                target_name,
            );
        }

        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let job = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT v.id, v.queue FROM crono.job_versions v
             JOIN crono.jobs j ON j.id = v.job_id
             JOIN crono.namespaces n ON n.id = j.namespace_id
             WHERE n.name = $1 AND j.name = $2 ORDER BY v.version DESC LIMIT 1",
        )
        .bind(job_namespace.as_str())
        .bind(job_name.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        let target_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT t.id FROM crono.targets t
             JOIN crono.namespaces n ON n.id = t.namespace_id
             WHERE n.name = $1 AND t.name = $2",
        )
        .bind(target_namespace.as_str())
        .bind(target_name.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        let run = sqlx::query_as::<_, (Uuid, OffsetDateTime)>(
            "INSERT INTO crono.runs (request_id, job_version_id, target_id)
             VALUES ($1, $2, $3) RETURNING id, created_at",
        )
        .bind(request_id)
        .bind(job.0)
        .bind(target_id)
        .fetch_one(&mut *transaction)
        .await;
        let (run_id, created_at) = match run {
            Ok(row) => row,
            Err(error) if is_unique_violation(&error) => {
                transaction.rollback().await.map_err(store_error)?;
                let existing = find_run_by_request(&self.pool, request_id)
                    .await?
                    .ok_or(StoreError::Internal)?;
                return compare_idempotent(
                    existing,
                    job_namespace,
                    job_name,
                    target_namespace,
                    target_name,
                );
            }
            Err(error) => return Err(store_error(error)),
        };
        insert_outbox(&mut transaction, run_id, job.0, target_id, &job.1).await?;
        transaction.commit().await.map_err(store_error)?;
        Ok((
            RunRecord {
                run: Run::new(
                    RunId::new(run_id),
                    request_id,
                    JobVersionId::new(job.0),
                    TargetId::new(target_id),
                    RunStatus::PendingDispatch,
                    created_at,
                    None,
                ),
                job_namespace: job_namespace.clone(),
                job_name: job_name.clone(),
                target_namespace: target_namespace.clone(),
                target_name: target_name.clone(),
            },
            true,
        ))
    }

    async fn list_runs(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
            });
        }
        let namespace_ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let rows = sqlx::query_as::<_, RunRow>(&format!(
            "{} WHERE ($1::uuid IS NULL OR r.id < $1)
             AND (NOT $2 OR j.namespace_id = ANY($3::uuid[]))
             ORDER BY r.id DESC LIMIT $4",
            run_select()
        ))
        .bind(before)
        .bind(restrict)
        .bind(&namespace_ids)
        .bind(i64::from(limit) + 1)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        run_page(rows, limit)
    }

    async fn get_run(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<RunRecord, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Err(StoreError::NotFound);
        }
        let namespace_ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let row = sqlx::query_as::<_, RunRow>(&format!(
            "{} WHERE r.id = $1 AND (NOT $2 OR j.namespace_id = ANY($3::uuid[]))",
            run_select()
        ))
        .bind(id.get())
        .bind(restrict)
        .bind(&namespace_ids)
        .fetch_optional(&self.pool)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        run_from_row(row)
    }

    async fn overview(&self, visibility: &VisibilityScope) -> Result<Overview, StoreError> {
        if matches!(visibility, VisibilityScope::None) {
            return Ok(Overview {
                namespaces: 0,
                jobs: 0,
                targets: 0,
                runs: 0,
            });
        }
        let namespace_ids = Self::namespace_ids(visibility);
        let restrict = matches!(visibility, VisibilityScope::Namespaces(_));
        let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(
            "SELECT
                (SELECT count(*) FROM crono.namespaces n WHERE NOT $1 OR n.id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.jobs j WHERE NOT $1 OR j.namespace_id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.targets t WHERE NOT $1 OR t.namespace_id = ANY($2::uuid[])),
                (SELECT count(*) FROM crono.runs r JOIN crono.job_versions v ON v.id = r.job_version_id
                    JOIN crono.jobs j ON j.id = v.job_id WHERE NOT $1 OR j.namespace_id = ANY($2::uuid[]))",
        )
        .bind(restrict)
        .bind(&namespace_ids)
        .fetch_one(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(Overview {
            namespaces: count(row.0)?,
            jobs: count(row.1)?,
            targets: count(row.2)?,
            runs: count(row.3)?,
        })
    }

    async fn pending_outbox(&self, limit: u16) -> Result<Vec<OutboxRecord>, StoreError> {
        let rows = sqlx::query_as::<_, (Uuid, Uuid, String, serde_json::Value)>(
            "SELECT id, run_id, subject, payload FROM crono.outbox
             WHERE published_at IS NULL ORDER BY id LIMIT $1",
        )
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.into_iter()
            .map(|(id, run_id, subject, payload)| {
                serde_json::to_vec(&payload)
                    .map(|payload| OutboxRecord {
                        id: DispatchId::new(id),
                        run_id: RunId::new(run_id),
                        subject,
                        payload,
                    })
                    .map_err(|error| {
                        error!(%error, "failed to encode persisted outbox payload");
                        StoreError::Internal
                    })
            })
            .collect()
    }

    async fn mark_published(
        &self,
        dispatch_id: DispatchId,
        run_id: RunId,
        stream_sequence: u64,
    ) -> Result<(), StoreError> {
        let sequence = i64::try_from(stream_sequence).map_err(|error| {
            error!(%error, stream_sequence, "JetStream sequence exceeds PostgreSQL bigint");
            StoreError::Internal
        })?;
        let mut transaction = self.pool.begin().await.map_err(store_error)?;
        let updated = sqlx::query(
            "UPDATE crono.outbox SET published_at = statement_timestamp(), last_error = NULL
             WHERE id = $1 AND run_id = $2 AND published_at IS NULL",
        )
        .bind(dispatch_id.get())
        .bind(run_id.get())
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        if updated.rows_affected() == 0 {
            transaction.rollback().await.map_err(store_error)?;
            return Err(StoreError::NotFound);
        }
        sqlx::query(
            "UPDATE crono.runs SET status = 'dispatched', dispatched_at = statement_timestamp(),
                    nats_stream_sequence = $2 WHERE id = $1 AND status = 'pending_dispatch'",
        )
        .bind(run_id.get())
        .bind(sequence)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
        transaction.commit().await.map_err(store_error)
    }

    async fn record_publish_failure(
        &self,
        dispatch_id: DispatchId,
        message: &str,
    ) -> Result<(), StoreError> {
        let bounded: String = message.chars().take(1024).collect();
        sqlx::query(
            "UPDATE crono.outbox SET attempts = attempts + 1, last_error = $2
             WHERE id = $1 AND published_at IS NULL",
        )
        .bind(dispatch_id.get())
        .bind(bounded)
        .execute(&self.pool)
        .await
        .map_err(store_error)?;
        Ok(())
    }

    async fn ready(&self) -> bool {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .is_ok()
    }
}

async fn namespace_id(
    transaction: &mut Transaction<'_, Postgres>,
    name: &NamespaceName,
) -> Result<Uuid, StoreError> {
    sqlx::query_scalar("SELECT id FROM crono.namespaces WHERE name = $1")
        .bind(name.as_str())
        .fetch_optional(&mut **transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)
}

async fn insert_outbox(
    transaction: &mut Transaction<'_, Postgres>,
    run_id: Uuid,
    job_version_id: Uuid,
    target_id: Uuid,
    queue: &str,
) -> Result<(), StoreError> {
    let dispatch_id = Uuid::now_v7();
    let envelope = DispatchEnvelope {
        schema_version: 1,
        dispatch_id,
        run_id,
        job_version_id,
        target_id,
        queue: queue.to_string(),
    };
    let payload = serde_json::to_value(envelope).map_err(|error| {
        error!(%error, "failed to serialize committed dispatch contract");
        StoreError::Internal
    })?;
    sqlx::query(
        "INSERT INTO crono.outbox (id, run_id, subject, payload)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(dispatch_id)
    .bind(run_id)
    .bind(format!("crono.dispatch.{queue}"))
    .bind(payload)
    .execute(&mut **transaction)
    .await
    .map_err(store_error)?;
    Ok(())
}

fn namespace_from_row(row: NamespaceRow) -> Result<Namespace, StoreError> {
    let (id, name, created_at) = row;
    Ok(Namespace::new(
        NamespaceId::new(id),
        NamespaceName::parse(&name).map_err(invalid_database_name)?,
        created_at,
    ))
}

fn job_from_row(row: JobRow) -> Result<JobRecord, StoreError> {
    let (
        job_id,
        namespace_id,
        job_name,
        job_created_at,
        namespace_name,
        version_id,
        version_number,
        executor,
        queue,
        version_created_at,
    ) = row;
    let version = u32::try_from(version_number).map_err(|error| {
        error!(%error, value = version_number, "invalid persisted Job version");
        StoreError::Internal
    })?;
    if executor != "noop" {
        error!(executor, "unsupported persisted Job executor");
        return Err(StoreError::Internal);
    }
    Ok(JobRecord {
        namespace: NamespaceName::parse(&namespace_name).map_err(invalid_database_name)?,
        job: Job::new(
            JobId::new(job_id),
            NamespaceId::new(namespace_id),
            ResourceName::parse(&job_name).map_err(invalid_database_name)?,
            job_created_at,
        ),
        version: JobVersion::new(
            JobVersionId::new(version_id),
            JobId::new(job_id),
            version,
            ExecutorKind::Noop,
            QueueName::parse(&queue).map_err(invalid_database_name)?,
            version_created_at,
        ),
    })
}

fn target_from_row(row: TargetRow) -> Result<TargetRecord, StoreError> {
    let (target_id, namespace_id, target_name, created_at, namespace_name) = row;
    Ok(TargetRecord {
        namespace: NamespaceName::parse(&namespace_name).map_err(invalid_database_name)?,
        target: Target::new(
            TargetId::new(target_id),
            NamespaceId::new(namespace_id),
            ResourceName::parse(&target_name).map_err(invalid_database_name)?,
            created_at,
        ),
    })
}

fn run_select() -> &'static str {
    "SELECT r.id, r.request_id, r.job_version_id, r.target_id, r.status,
            r.created_at, r.dispatched_at, jn.name, j.name, tn.name, t.name
     FROM crono.runs r
     JOIN crono.job_versions v ON v.id = r.job_version_id
     JOIN crono.jobs j ON j.id = v.job_id
     JOIN crono.namespaces jn ON jn.id = j.namespace_id
     JOIN crono.targets t ON t.id = r.target_id
     JOIN crono.namespaces tn ON tn.id = t.namespace_id"
}

fn run_from_row(row: RunRow) -> Result<RunRecord, StoreError> {
    let (
        run_id,
        request_id,
        job_version_id,
        target_id,
        persisted_status,
        created_at,
        dispatched_at,
        job_namespace,
        job_name,
        target_namespace,
        target_name,
    ) = row;
    let status = match persisted_status.as_str() {
        "pending_dispatch" => RunStatus::PendingDispatch,
        "dispatched" => RunStatus::Dispatched,
        value => {
            error!(status = value, "unsupported persisted Run status");
            return Err(StoreError::Internal);
        }
    };
    Ok(RunRecord {
        run: Run::new(
            RunId::new(run_id),
            request_id,
            JobVersionId::new(job_version_id),
            TargetId::new(target_id),
            status,
            created_at,
            dispatched_at,
        ),
        job_namespace: NamespaceName::parse(&job_namespace).map_err(invalid_database_name)?,
        job_name: ResourceName::parse(&job_name).map_err(invalid_database_name)?,
        target_namespace: NamespaceName::parse(&target_namespace).map_err(invalid_database_name)?,
        target_name: ResourceName::parse(&target_name).map_err(invalid_database_name)?,
    })
}

async fn find_run_by_request(
    pool: &PgPool,
    request_id: Uuid,
) -> Result<Option<RunRecord>, StoreError> {
    sqlx::query_as::<_, RunRow>(&format!("{} WHERE r.request_id = $1", run_select()))
        .bind(request_id)
        .fetch_optional(pool)
        .await
        .map_err(store_error)?
        .map(run_from_row)
        .transpose()
}

fn compare_idempotent(
    existing: RunRecord,
    job_namespace: &NamespaceName,
    job_name: &ResourceName,
    target_namespace: &NamespaceName,
    target_name: &ResourceName,
) -> Result<(RunRecord, bool), StoreError> {
    if &existing.job_namespace == job_namespace
        && &existing.job_name == job_name
        && &existing.target_namespace == target_namespace
        && &existing.target_name == target_name
    {
        Ok((existing, false))
    } else {
        Err(StoreError::IdempotencyConflict)
    }
}

fn namespace_page(mut rows: Vec<NamespaceRow>, limit: u16) -> Result<Page<Namespace>, StoreError> {
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(namespace_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| items.last().map(|item| item.name().to_string()))
        .flatten();
    Ok(Page { items, next_cursor })
}

fn job_page(mut rows: Vec<JobRow>, limit: u16) -> Result<Page<JobRecord>, StoreError> {
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(job_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| items.last().map(|item| item.job.name().to_string()))
        .flatten();
    Ok(Page { items, next_cursor })
}

fn target_page(mut rows: Vec<TargetRow>, limit: u16) -> Result<Page<TargetRecord>, StoreError> {
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(target_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| items.last().map(|item| item.target.name().to_string()))
        .flatten();
    Ok(Page { items, next_cursor })
}

fn run_page(mut rows: Vec<RunRow>, limit: u16) -> Result<Page<RunRecord>, StoreError> {
    let has_more = rows.len() > usize::from(limit);
    if has_more {
        rows.pop();
    }
    let items = rows
        .into_iter()
        .map(run_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let next_cursor = has_more
        .then(|| items.last().map(|item| item.run.id().get().to_string()))
        .flatten();
    Ok(Page { items, next_cursor })
}

fn count(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|error| {
        error!(%error, value, "PostgreSQL returned a negative resource count");
        StoreError::Internal
    })
}

fn invalid_database_name(error: crate::domain::NameError) -> StoreError {
    error!(%error, "database contains a name outside domain invariants");
    StoreError::Internal
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| database.code().as_deref() == Some("23505"))
}

fn store_error(error: sqlx::Error) -> StoreError {
    if is_unique_violation(&error) {
        return StoreError::Conflict;
    }
    let unavailable = matches!(
        &error,
        sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
    );
    error!(%error, "PostgreSQL control-plane operation failed");
    drop(error);
    if unavailable {
        StoreError::Unavailable
    } else {
        StoreError::Internal
    }
}
