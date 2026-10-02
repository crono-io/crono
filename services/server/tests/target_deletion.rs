//! Verify authorized Target deletion against PostgreSQL's reference constraints.
//!
//! Tests use isolated Namespace names in the throwaway test database. Referenced
//! Targets must survive failed deletion with their membership and history intact;
//! deletion permission is checked before even the protected Target is read.

mod support;

use anyhow::Result;
use async_trait::async_trait;
use crono_server::{
    application::{
        Application, ApplicationError, AuthorizationError, Authorizer, Capability,
        ControlPlaneStore, DevelopmentIdentity, JobDefinition, JobRecord, PermitAllAuthorizer,
        RequestContext, ResourceScope, StoreError, TargetDefinition, VisibilityScope,
    },
    domain::{ExecutorKind, JobId, NamespaceId, NamespaceName, QueueName, ResourceName, TargetId},
    infrastructure::{DatabasePoolConfig, PostgresStore},
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn deleting_unused_target_removes_it_and_releases_its_name() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let namespace = store
        .create_namespace(&NamespaceName::parse(&format!(
            "delete-{}",
            Uuid::now_v7().simple()
        ))?)
        .await?;
    // A Target named default outside the starter Namespace is ordinary data.
    let name = ResourceName::parse("default")?;
    let definition = TargetDefinition {
        arguments: Vec::new(),
        inputs: serde_json::json!({}),
    };
    let target = store
        .create_target(namespace.id(), &name, &definition)
        .await?;
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    app.delete_target(&context, target.target.id().get())
        .await?;
    assert!(matches!(
        store.get_target(target.target.id()).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .list_targets(namespace.id(), &VisibilityScope::All, 25, None)
            .await?
            .items,
        []
    );
    assert!(matches!(
        app.delete_target(&context, target.target.id().get()).await,
        Err(ApplicationError::NotFound)
    ));
    let replacement = store
        .create_target(namespace.id(), &name, &definition)
        .await?;
    assert_ne!(replacement.target.id(), target.target.id());
    app.delete_target(&context, replacement.target.id().get())
        .await?;
    Ok(())
}

/// Reject the expected resource-specific capability without performing any I/O.
struct DenyDeletion(TargetId);

#[async_trait]
impl Authorizer for DenyDeletion {
    async fn authorize(
        &self,
        _context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        assert_eq!(capability, Capability::TargetDelete);
        assert_eq!(resource, &ResourceScope::Target(self.0));
        Err(AuthorizationError::Forbidden)
    }

    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        Err(AuthorizationError::Forbidden)
    }
}

#[tokio::test]
async fn deletion_authorizes_before_protection_or_existence_checks() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let pool = PgPool::connect(&database_url).await?;
    let id: Uuid = sqlx::query_scalar(
        "SELECT t.id FROM crono.targets t JOIN crono.namespaces n ON n.id = t.namespace_id
         WHERE n.name = 'default' AND t.name = 'default'",
    )
    .fetch_one(&pool)
    .await?;
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    for target_id in [id, Uuid::now_v7()] {
        let app = Application::new(
            Arc::new(store.clone()),
            Arc::new(DenyDeletion(TargetId::new(target_id))),
        );
        assert!(matches!(
            app.delete_target(&context, target_id).await,
            Err(ApplicationError::Authorization(
                AuthorizationError::Forbidden
            ))
        ));
    }
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    assert!(matches!(
        app.delete_target(&context, id).await,
        Err(ApplicationError::InvalidInput { field: None, .. })
    ));
    assert!(store.get_target(TargetId::new(id)).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn deletion_preserves_target_sets_schedules_requests_and_run_history() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let pool = PgPool::connect(&database_url).await?;
    let namespace = store
        .create_namespace(&NamespaceName::parse(&format!(
            "in-use-{}",
            Uuid::now_v7().simple()
        ))?)
        .await?;
    let target = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("destination")?,
            &TargetDefinition {
                arguments: vec!["original".to_string()],
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let id = target.target.id();
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    let set = store
        .create_target_set(
            namespace.id(),
            &ResourceName::parse("group")?,
            &[id],
            &serde_json::json!({}),
        )
        .await?;
    assert!(matches!(
        app.delete_target(&context, id.get()).await,
        Err(ApplicationError::InUse)
    ));
    assert_eq!(
        store.get_target_set(set.target_set.id()).await?.targets,
        vec![target.target.clone()]
    );
    sqlx::query("DELETE FROM crono.target_set_members WHERE target_set_id = $1")
        .bind(set.target_set.id().get())
        .execute(&pool)
        .await?;

    let job = create_noop_job(&store, namespace.id()).await?;
    verify_schedule_reference(&pool, &app, namespace.id(), job.job.id(), id).await?;

    let request_id = Uuid::now_v7();
    let (run, _) = store.create_run(request_id, job.job.id(), id).await?;
    // Remove the request reference to prove Run history independently blocks deletion.
    sqlx::query("UPDATE crono.runs SET request_id = NULL WHERE id = $1")
        .bind(run.run.id().get())
        .execute(&pool)
        .await?;
    sqlx::query("DELETE FROM crono.run_requests WHERE request_id = $1")
        .bind(request_id)
        .execute(&pool)
        .await?;
    assert!(matches!(
        app.delete_target(&context, id.get()).await,
        Err(ApplicationError::InUse)
    ));
    assert_eq!(
        store
            .get_run(run.run.id(), &VisibilityScope::All)
            .await?
            .run
            .target_id(),
        id
    );
    assert_eq!(store.get_target(id).await?.target.arguments(), ["original"]);
    remove_run(&pool, run.run.id().get()).await?;

    sqlx::query("INSERT INTO crono.run_requests (request_id, namespace_id, job_id, target_id) VALUES ($1, $2, $3, $4)")
        .bind(request_id).bind(namespace.id().get()).bind(job.job.id().get()).bind(id.get()).execute(&pool).await?;
    assert!(matches!(
        app.delete_target(&context, id.get()).await,
        Err(ApplicationError::InUse)
    ));
    let retained_target: Uuid =
        sqlx::query_scalar("SELECT target_id FROM crono.run_requests WHERE request_id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(retained_target, id.get());
    sqlx::query("DELETE FROM crono.run_requests WHERE request_id = $1")
        .bind(request_id)
        .execute(&pool)
        .await?;
    app.delete_target(&context, id.get()).await?;
    Ok(())
}

/// Remove only this test's Run and dispatch records so remaining references can be isolated.
async fn remove_run(pool: &PgPool, run_id: Uuid) -> Result<()> {
    for statement in [
        "DELETE FROM crono.outbox WHERE run_id = $1",
        "DELETE FROM crono.run_events WHERE run_id = $1",
        "DELETE FROM crono.run_attempts WHERE run_id = $1",
    ] {
        sqlx::query(statement).bind(run_id).execute(pool).await?;
    }
    sqlx::query("DELETE FROM crono.runs WHERE id = $1")
        .bind(run_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Supply a minimal execution definition without changing the shared default Queue.
async fn create_noop_job(store: &PostgresStore, namespace_id: NamespaceId) -> Result<JobRecord> {
    let queue = store
        .get_queue_by_name(&QueueName::parse("default")?)
        .await?;
    Ok(store
        .create_job(
            namespace_id,
            &ResourceName::parse("task")?,
            &JobDefinition {
                executor: ExecutorKind::Noop,
                queue_id: queue.id(),
                executable: None,
                shell_command: None,
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
                idempotent: true,
                dry_run: false,
                max_attempts: 1,
                retry_initial_seconds: 1,
                retry_max_seconds: 60,
                retry_multiplier: 2.0,
                retry_jitter: 0.2,
            },
        )
        .await?)
}

/// Prove that even a disabled Schedule retains its destination after a rejected deletion.
async fn verify_schedule_reference(
    pool: &PgPool,
    app: &Application,
    namespace_id: NamespaceId,
    job_id: JobId,
    target_id: TargetId,
) -> Result<()> {
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    let schedule_id: Uuid = sqlx::query_scalar(
        "INSERT INTO crono.schedules (namespace_id, job_id, target_id, name, schedule_type, execute_at, enabled)
         VALUES ($1, $2, $3, 'once', 'once', statement_timestamp() + interval '1 day', false) RETURNING id",
    ).bind(namespace_id.get()).bind(job_id.get()).bind(target_id.get()).fetch_one(pool).await?;
    assert!(matches!(
        app.delete_target(&context, target_id.get()).await,
        Err(ApplicationError::InUse)
    ));
    let retained_target: Uuid =
        sqlx::query_scalar("SELECT target_id FROM crono.schedules WHERE id = $1")
            .bind(schedule_id)
            .fetch_one(pool)
            .await?;
    assert_eq!(retained_target, target_id.get());
    sqlx::query("DELETE FROM crono.schedules WHERE id = $1")
        .bind(schedule_id)
        .execute(pool)
        .await?;
    Ok(())
}
