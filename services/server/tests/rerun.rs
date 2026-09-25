//! Verify that a repeat is a new, single-Target dispatch from stored execution data.
//!
//! The test uses an isolated initialized PostgreSQL database and deliberately
//! changes the Job and Target after the source Run was created. Neither those
//! changes nor a Target Set's other members may alter the copied snapshot.

use anyhow::Result;
use crono_api::{ExecutionSnapshot, ExecutionTrigger};
use crono_server::{
    application::{
        Application, ApplicationError, ControlPlaneStore, DevelopmentIdentity, JobDefinition,
        PermitAllAuthorizer, RunListFilter, RunRecord, TargetDefinition, VisibilityScope,
    },
    domain::{
        ExecutorKind, JobId, NamespaceId, NamespaceName, QueueId, QueueName, ResourceName, RunId,
        RunStatus, TargetId, TargetSelection, TargetSetId,
    },
    infrastructure::PostgresStore,
};
use sqlx::PgPool;
use std::{env, sync::Arc};
use uuid::Uuid;

struct Fixture {
    namespace_id: NamespaceId,
    job_id: JobId,
    queue_id: QueueId,
    queue_name: QueueName,
    target_a_id: TargetId,
    set_id: TargetSetId,
    sources: Vec<RunRecord>,
    original: JobDefinition,
}

#[tokio::test]
#[ignore = "requires an initialized CRONO_TEST_DATABASE_URL"]
async fn rerun_preserves_snapshot_and_repeats_only_selected_set_member() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL")?;
    let store = PostgresStore::connect(&database_url).await?;
    let pool = PgPool::connect(&database_url).await?;
    let fixture = create_fixture(&store).await?;
    let source = fixture
        .sources
        .iter()
        .find(|run| run.run.target_id() == fixture.target_a_id)
        .ok_or_else(|| anyhow::anyhow!("missing target A run"))?;
    let source_id = source.run.id();
    let original_snapshot = snapshot(&pool, source_id).await?;
    assert_eq!(
        original_snapshot.arguments,
        vec!["hello invocation", "first"]
    );
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let context = DevelopmentIdentity.context();
    assert!(matches!(
        app.rerun_run(&context, source_id.get(), Uuid::now_v7())
            .await,
        Err(ApplicationError::InvalidInput { .. })
    ));

    change_catalog_after_source(&store, &pool, &fixture, source_id).await?;

    let rerun_request_id = Uuid::now_v7();
    let outcome = app
        .rerun_run(&context, source_id.get(), rerun_request_id)
        .await?;
    assert!(outcome.created);
    let rerun = outcome
        .runs
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing re-run"))?;
    assert_ne!(rerun.run.id(), source_id);
    assert_eq!(rerun.run.rerun_of_run_id(), Some(source_id));
    assert_eq!(rerun.run.target_id(), fixture.target_a_id);
    assert_eq!(rerun.run.attempt_count(), 1);
    let repeated_snapshot = snapshot(&pool, rerun.run.id()).await?;
    assert_eq!(repeated_snapshot.executable, original_snapshot.executable);
    assert_eq!(repeated_snapshot.arguments, original_snapshot.arguments);
    assert_eq!(repeated_snapshot.inputs, original_snapshot.inputs);
    assert_eq!(
        repeated_snapshot.retry_initial_seconds,
        original_snapshot.retry_initial_seconds
    );
    assert_eq!(repeated_snapshot.trigger, Some(ExecutionTrigger::Rerun));
    assert_eq!(repeated_snapshot.idempotency_key, rerun.run.id().get());
    assert_eq!(snapshot(&pool, source_id).await?, original_snapshot);
    let duplicate = app
        .rerun_run(&context, source_id.get(), rerun_request_id)
        .await?;
    assert!(!duplicate.created);
    assert_eq!(
        duplicate.runs.first().map(|item| item.run.id()),
        Some(rerun.run.id())
    );
    verify_unrepeatable_runs(
        &app,
        &store,
        &pool,
        &fixture,
        source_id,
        rerun_request_id,
        rerun.run.id(),
    )
    .await?;
    verify_history_filters(&store, &fixture, rerun.run.id()).await?;

    remove_run(&pool, rerun.run.id()).await?;
    for source in &fixture.sources {
        remove_run(&pool, source.run.id()).await?;
    }
    remove_fixture(&pool, &fixture).await?;
    Ok(())
}

async fn verify_history_filters(
    store: &PostgresStore,
    fixture: &Fixture,
    rerun_id: RunId,
) -> Result<()> {
    let visible = store
        .list_runs(
            &VisibilityScope::All,
            RunListFilter {
                namespace_id: Some(fixture.namespace_id),
                ..RunListFilter::default()
            },
            10,
            None,
        )
        .await?;
    assert_eq!(visible.items.len(), 3);
    let set_members = store
        .list_runs(
            &VisibilityScope::All,
            RunListFilter {
                target_set_id: Some(fixture.set_id),
                ..RunListFilter::default()
            },
            10,
            None,
        )
        .await?;
    assert_eq!(set_members.items.len(), 2);
    let succeeded = store
        .list_runs(
            &VisibilityScope::All,
            RunListFilter {
                namespace_id: Some(fixture.namespace_id),
                status: Some(RunStatus::Succeeded),
                ..RunListFilter::default()
            },
            10,
            None,
        )
        .await?;
    assert_eq!(succeeded.items.len(), 1);
    assert_eq!(
        store
            .list_run_events(rerun_id, &VisibilityScope::All)
            .await?
            .first()
            .map(|event| event.event_type.as_str()),
        Some("created")
    );
    assert_eq!(
        store
            .list_run_events(rerun_id, &VisibilityScope::None)
            .await
            .err(),
        Some(crono_server::application::StoreError::NotFound)
    );
    Ok(())
}

async fn verify_unrepeatable_runs(
    app: &Application,
    store: &PostgresStore,
    pool: &PgPool,
    fixture: &Fixture,
    source_id: RunId,
    request_id: Uuid,
    rerun_id: RunId,
) -> Result<()> {
    store
        .update_queue(fixture.queue_id, &fixture.queue_name, None, false)
        .await?;
    let duplicate = app
        .rerun_run(&DevelopmentIdentity.context(), source_id.get(), request_id)
        .await?;
    assert!(!duplicate.created);
    assert_eq!(
        duplicate.runs.first().map(|run| run.run.id()),
        Some(rerun_id)
    );
    assert!(matches!(
        app.rerun_run(
            &DevelopmentIdentity.context(),
            source_id.get(),
            Uuid::now_v7()
        )
        .await,
        Err(ApplicationError::InvalidInput { .. })
    ));
    store
        .update_queue(fixture.queue_id, &fixture.queue_name, None, true)
        .await?;
    sqlx::query("UPDATE crono.runs SET execution_snapshot = '{}'::jsonb WHERE id = $1")
        .bind(source_id.get())
        .execute(pool)
        .await?;
    assert!(matches!(
        app.rerun_run(
            &DevelopmentIdentity.context(),
            source_id.get(),
            Uuid::now_v7()
        )
        .await,
        Err(ApplicationError::InvalidInput { .. })
    ));
    Ok(())
}

async fn change_catalog_after_source(
    store: &PostgresStore,
    pool: &PgPool,
    fixture: &Fixture,
    source_id: RunId,
) -> Result<()> {
    sqlx::query("UPDATE crono.runs SET status = 'succeeded', started_at = statement_timestamp(), completed_at = statement_timestamp() WHERE id = $1")
        .bind(source_id.get()).execute(pool).await?;
    let changed = JobDefinition {
        executable: Some("/bin/false".to_string()),
        arguments: vec!["changed".to_string()],
        inputs: serde_json::json!({"name":"new"}),
        ..fixture.original.clone()
    };
    store
        .update_job(fixture.job_id, &ResourceName::parse("source")?, &changed)
        .await?;
    store
        .update_target(
            fixture.target_a_id,
            &ResourceName::parse("a")?,
            &TargetDefinition {
                arguments: vec!["changed".to_string()],
                inputs: serde_json::json!({"destination":"new"}),
            },
        )
        .await?;
    Ok(())
}

async fn create_fixture(store: &PostgresStore) -> Result<Fixture> {
    let suffix = Uuid::now_v7().simple().to_string();
    let namespace = store
        .create_namespace(&NamespaceName::parse(&format!(
            "rerun-{}",
            suffix.get(..12).unwrap_or("test")
        ))?)
        .await?;
    let queue_name = QueueName::parse(&format!("rerunq-{}", suffix.get(..12).unwrap_or("test")))?;
    let queue = store.create_queue(&queue_name, None).await?;
    let original = JobDefinition {
        executor: ExecutorKind::Process,
        queue_id: queue.id(),
        executable: Some("/bin/echo".to_string()),
        shell_command: None,
        arguments: vec!["hello {{ name }}".to_string()],
        inputs: serde_json::json!({"name":"job"}),
        idempotent: true,
        dry_run: false,
        max_attempts: 2,
        retry_initial_seconds: 2,
        retry_max_seconds: 30,
        retry_multiplier: 2.0,
        retry_jitter: 0.1,
    };
    let job = store
        .create_job(namespace.id(), &ResourceName::parse("source")?, &original)
        .await?;
    let target_a = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("a")?,
            &TargetDefinition {
                arguments: vec!["{{ destination }}".to_string()],
                inputs: serde_json::json!({"destination":"first"}),
            },
        )
        .await?;
    let target_b = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("b")?,
            &TargetDefinition {
                arguments: vec!["{{ destination }}".to_string()],
                inputs: serde_json::json!({"destination":"second"}),
            },
        )
        .await?;
    let set = store
        .create_target_set(
            namespace.id(),
            &ResourceName::parse("both")?,
            &[target_a.target.id(), target_b.target.id()],
            &serde_json::json!({}),
        )
        .await?;
    let (sources, created) = store
        .create_runs(
            Uuid::now_v7(),
            job.job.id(),
            TargetSelection::TargetSet(set.target_set.id()),
            &serde_json::json!({"name":"invocation"}),
        )
        .await?;
    assert!(created);
    assert_eq!(sources.len(), 2);
    Ok(Fixture {
        namespace_id: namespace.id(),
        job_id: job.job.id(),
        queue_id: queue.id(),
        queue_name,
        target_a_id: target_a.target.id(),
        set_id: set.target_set.id(),
        sources,
        original,
    })
}

async fn remove_fixture(pool: &PgPool, fixture: &Fixture) -> Result<()> {
    sqlx::query("DELETE FROM crono.run_requests WHERE namespace_id = $1")
        .bind(fixture.namespace_id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.target_set_members WHERE target_set_id = $1")
        .bind(fixture.set_id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.target_sets WHERE id = $1")
        .bind(fixture.set_id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.jobs WHERE namespace_id = $1")
        .bind(fixture.namespace_id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.targets WHERE namespace_id = $1")
        .bind(fixture.namespace_id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.namespaces WHERE id = $1")
        .bind(fixture.namespace_id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.queues WHERE id = $1")
        .bind(fixture.queue_id.get())
        .execute(pool)
        .await?;
    Ok(())
}

async fn snapshot(pool: &PgPool, id: RunId) -> Result<ExecutionSnapshot> {
    let value: serde_json::Value =
        sqlx::query_scalar("SELECT execution_snapshot FROM crono.runs WHERE id = $1")
            .bind(id.get())
            .fetch_one(pool)
            .await?;
    Ok(serde_json::from_value(value)?)
}

async fn remove_run(pool: &PgPool, id: RunId) -> Result<()> {
    sqlx::query("DELETE FROM crono.outbox WHERE run_id = $1")
        .bind(id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.run_events WHERE run_id = $1")
        .bind(id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.run_attempts WHERE run_id = $1")
        .bind(id.get())
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM crono.runs WHERE id = $1")
        .bind(id.get())
        .execute(pool)
        .await?;
    Ok(())
}
