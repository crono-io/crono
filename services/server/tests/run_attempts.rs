//! Verify Attempt output ordering and `RunRead` visibility against PostgreSQL.

use anyhow::Result;
use async_trait::async_trait;
use crono_api::OutputSnapshotRequest;
use crono_server::{
    application::{
        Application, ApplicationError, AuthorizationError, Authorizer, Capability,
        ControlPlaneStore, DevelopmentIdentity, JobDefinition, RequestContext, ResourceScope,
        StoreError, TargetDefinition, VisibilityScope,
    },
    domain::{ExecutorKind, NamespaceId, NamespaceName, QueueName, ResourceName, RunId},
    infrastructure::PostgresStore,
};
use sqlx::PgPool;
use std::{collections::BTreeSet, env, sync::Arc};
use uuid::Uuid;

struct RunReadAuthorizer {
    allowed_run: Uuid,
    visibility: VisibilityScope,
}

#[async_trait]
impl Authorizer for RunReadAuthorizer {
    async fn authorize(
        &self,
        _context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        if capability == Capability::RunRead
            && matches!(resource, ResourceScope::Run(id) if *id == self.allowed_run)
        {
            Ok(())
        } else {
            Err(AuthorizationError::Forbidden)
        }
    }

    async fn visibility(
        &self,
        _context: &RequestContext,
        capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        if capability == Capability::RunRead {
            Ok(self.visibility.clone())
        } else {
            Err(AuthorizationError::Forbidden)
        }
    }
}

/// Insert a Run with two distinct Attempt outputs for visibility checks.
async fn create_run_with_attempts(
    store: &PostgresStore,
    pool: &PgPool,
) -> Result<(RunId, NamespaceId)> {
    let suffix = Uuid::now_v7().simple().to_string();
    let name = NamespaceName::parse(&format!("attempt-{}", suffix.get(20..).unwrap_or("test")))?;
    let namespace = store.create_namespace(&name).await?;
    let queue = store
        .get_queue_by_name(&QueueName::parse("default")?)
        .await?;
    let job = store
        .create_job(
            namespace.id(),
            &ResourceName::parse("job")?,
            &JobDefinition {
                executor: ExecutorKind::Noop,
                queue_id: queue.id(),
                executable: None,
                shell_command: None,
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
                idempotent: false,
                dry_run: false,
                max_attempts: 2,
                retry_initial_seconds: 1,
                retry_max_seconds: 1,
                retry_multiplier: 1.0,
                retry_jitter: 0.0,
            },
        )
        .await?;
    let target = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("target")?,
            &TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let (run, _) = store
        .create_run(Uuid::now_v7(), job.job.id(), target.target.id())
        .await?;
    let run_id = run.run.id();
    sqlx::query("UPDATE crono.run_attempts SET stdout_tail = 'first' WHERE run_id = $1")
        .bind(run_id.get())
        .execute(pool)
        .await?;
    sqlx::query("UPDATE crono.runs SET attempt_count = 2 WHERE id = $1")
        .bind(run_id.get())
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT INTO crono.run_attempts (run_id, attempt, status, stdout_tail)
         VALUES ($1, 2, 'succeeded', 'second')",
    )
    .bind(run_id.get())
    .execute(pool)
    .await?;
    Ok((run_id, namespace.id()))
}

/// Remove only the records created by this test after visibility assertions.
async fn remove_run_fixture(
    pool: &PgPool,
    run_id: RunId,
    namespace_id: NamespaceId,
    foreign_id: NamespaceId,
) -> Result<()> {
    let mut transaction = pool.begin().await?;
    sqlx::query("DELETE FROM crono.outbox WHERE run_id = $1")
        .bind(run_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.run_events WHERE run_id = $1")
        .bind(run_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.run_attempts WHERE run_id = $1")
        .bind(run_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.runs WHERE id = $1")
        .bind(run_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.run_requests WHERE namespace_id = $1")
        .bind(namespace_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.jobs WHERE namespace_id = $1")
        .bind(namespace_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.targets WHERE namespace_id = $1")
        .bind(namespace_id.get())
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.namespaces WHERE id = $1 OR id = $2")
        .bind(namespace_id.get())
        .bind(foreign_id.get())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires an initialized CRONO_TEST_DATABASE_URL"]
async fn attempt_output_is_ordered_and_scoped_to_authorized_runs() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL")?;
    let store = PostgresStore::connect(&database_url).await?;
    let pool = PgPool::connect(&database_url).await?;
    let (run_id, namespace_id) = create_run_with_attempts(&store, &pool).await?;

    let visible = VisibilityScope::Namespaces(BTreeSet::from([namespace_id]));
    let attempts = store.list_run_attempts(run_id, &visible).await?;
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts.first().map(|item| item.attempt), Some(2));
    assert_eq!(
        attempts
            .first()
            .and_then(|item| item.stdout_tail.as_deref()),
        Some("second")
    );
    assert_eq!(
        attempts.get(1).and_then(|item| item.stdout_tail.as_deref()),
        Some("first")
    );
    assert_eq!(
        store
            .list_run_attempts(run_id, &VisibilityScope::None)
            .await,
        Err(StoreError::NotFound)
    );
    let foreign_name = format!("foreign-{}", Uuid::now_v7().simple());
    let foreign = store
        .create_namespace(&NamespaceName::parse(&foreign_name)?)
        .await?;
    assert_eq!(
        store
            .list_run_attempts(
                run_id,
                &VisibilityScope::Namespaces(BTreeSet::from([foreign.id()])),
            )
            .await,
        Err(StoreError::NotFound)
    );

    let store: Arc<dyn ControlPlaneStore> = Arc::new(store);
    let app = Application::new(
        Arc::clone(&store),
        Arc::new(RunReadAuthorizer {
            allowed_run: run_id.get(),
            visibility: visible.clone(),
        }),
    );
    let context = DevelopmentIdentity.context();
    assert_eq!(
        app.list_run_attempts(&context, run_id.get()).await?.len(),
        2
    );
    let denied = Application::new(
        store,
        Arc::new(RunReadAuthorizer {
            allowed_run: Uuid::now_v7(),
            visibility: visible,
        }),
    );
    assert!(matches!(
        denied.list_run_attempts(&context, run_id.get()).await,
        Err(ApplicationError::Authorization(
            AuthorizationError::Forbidden
        ))
    ));
    remove_run_fixture(&pool, run_id, namespace_id, foreign.id()).await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires an initialized CRONO_TEST_DATABASE_URL"]
async fn live_output_requires_lease_and_monotonic_sequence() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL")?;
    let store = PostgresStore::connect(&database_url).await?;
    let pool = PgPool::connect(&database_url).await?;
    let (run_id, namespace_id) = create_run_with_attempts(&store, &pool).await?;
    let attempt_id: Uuid = sqlx::query_scalar(
        "UPDATE crono.run_attempts
            SET status = 'running', worker_id = 'live-test-worker',
                lease_expires_at = statement_timestamp() + interval '1 minute'
          WHERE run_id = $1 AND attempt = 1 RETURNING id",
    )
    .bind(run_id.get())
    .fetch_one(&pool)
    .await?;
    let request = OutputSnapshotRequest {
        attempt_id,
        worker_id: "live-test-worker".to_string(),
        sequence: 1,
        stdout_tail: "started\n".to_string(),
        stderr_tail: "warning\n".to_string(),
    };
    assert!(store.record_attempt_output(&request).await?);
    let visible = VisibilityScope::Namespaces(BTreeSet::from([namespace_id]));
    let attempts = store.list_run_attempts(run_id, &visible).await?;
    assert!(
        attempts
            .iter()
            .any(|item| item.stdout_tail.as_deref() == Some("started\n"))
    );
    let stale = OutputSnapshotRequest {
        stdout_tail: "older\n".to_string(),
        ..request.clone()
    };
    assert!(!store.record_attempt_output(&stale).await?);
    let wrong_worker = OutputSnapshotRequest {
        worker_id: "other-worker".to_string(),
        sequence: 2,
        ..request.clone()
    };
    assert!(!store.record_attempt_output(&wrong_worker).await?);
    let current: Option<String> =
        sqlx::query_scalar("SELECT stdout_tail FROM crono.run_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(current.as_deref(), Some("started\n"));
    sqlx::query("UPDATE crono.run_attempts SET status = 'succeeded' WHERE id = $1")
        .bind(attempt_id)
        .execute(&pool)
        .await?;
    let late = OutputSnapshotRequest {
        sequence: 2,
        ..request
    };
    assert!(!store.record_attempt_output(&late).await?);
    remove_run_fixture(&pool, run_id, namespace_id, namespace_id).await?;
    Ok(())
}
