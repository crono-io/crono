//! Verify that Job dry-run policy is snapshotted and completes without execution retries.
//!
//! This test uses an initialized development PostgreSQL database. It advances
//! one Attempt to queued directly so the claim/completion path can be tested
//! without changing NATS state or touching another Run.

use anyhow::Result;
use crono_api::{ClaimRequest, CompletionRequest, ExecutionSnapshot};
use crono_server::{
    application::{ControlPlaneStore, JobDefinition},
    domain::{ExecutorKind, NamespaceName, QueueName, ResourceName},
    infrastructure::{DatabasePoolConfig, PostgresStore},
};
use sqlx::PgPool;
use std::env;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires an initialized CRONO_TEST_DATABASE_URL"]
async fn job_dry_run_is_immutable_per_run_and_completes_as_skipped() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL")?;
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let pool = PgPool::connect(&database_url).await?;
    let suffix = Uuid::now_v7().simple().to_string();
    let namespace_name =
        NamespaceName::parse(&format!("dry-run-{}", suffix.get(..12).unwrap_or("test")))?;
    let namespace = store.create_namespace(&namespace_name).await?;
    let queue = store
        .get_queue_by_name(&QueueName::parse("default")?)
        .await?;
    let job_name = ResourceName::parse("preview")?;
    let dry_definition = JobDefinition {
        executor: ExecutorKind::Process,
        queue_id: queue.id(),
        executable: Some("/bin/echo".to_string()),
        shell_command: None,
        arguments: vec!["hello {{ name }}".to_string()],
        inputs: serde_json::json!({"name": "world"}),
        idempotent: true,
        dry_run: true,
        max_attempts: 2,
        retry_initial_seconds: 1,
        retry_max_seconds: 60,
        retry_multiplier: 2.0,
        retry_jitter: 0.2,
    };
    let job = store
        .create_job(namespace.id(), &job_name, &dry_definition)
        .await?;
    assert!(store.get_job(job.job.id()).await?.job.dry_run());
    let target = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("local")?,
            &crono_server::application::TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let (first, _) = store
        .create_run(Uuid::now_v7(), job.job.id(), target.target.id())
        .await?;
    let mut normal_definition = dry_definition.clone();
    normal_definition.dry_run = false;
    store
        .update_job(job.job.id(), &job_name, &normal_definition)
        .await?;
    let (second, _) = store
        .create_run(Uuid::now_v7(), job.job.id(), target.target.id())
        .await?;
    let first_snapshot: ExecutionSnapshot = serde_json::from_value(
        sqlx::query_scalar("SELECT execution_snapshot FROM crono.runs WHERE id = $1")
            .bind(first.run.id().get())
            .fetch_one(&pool)
            .await?,
    )?;
    let second_snapshot: ExecutionSnapshot = serde_json::from_value(
        sqlx::query_scalar("SELECT execution_snapshot FROM crono.runs WHERE id = $1")
            .bind(second.run.id().get())
            .fetch_one(&pool)
            .await?,
    )?;
    assert!(first_snapshot.dry_run);
    assert!(!second_snapshot.dry_run);
    assert_eq!(first_snapshot.arguments, ["hello world"]);

    assert_skipped_completion(&store, &pool, first.run.id().get(), queue.id().get()).await?;
    cleanup(&pool, namespace.id().get(), job.job.id().get()).await?;
    Ok(())
}

/// Advance a claimed dry-run Attempt and verify its durable terminal state.
async fn assert_skipped_completion(
    store: &PostgresStore,
    pool: &PgPool,
    run_id: Uuid,
    queue_id: Uuid,
) -> Result<()> {
    let attempt_id: Uuid =
        sqlx::query_scalar("SELECT id FROM crono.run_attempts WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(pool)
            .await?;
    sqlx::query("UPDATE crono.run_attempts SET status = 'queued' WHERE id = $1")
        .bind(attempt_id)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE crono.runs SET status = 'queued' WHERE id = $1")
        .bind(run_id)
        .execute(pool)
        .await?;
    let worker_id = "job-dry-run-test";
    let claim = store
        .claim_attempt(&ClaimRequest {
            run_id,
            attempt_id,
            queue_id,
            worker_id: worker_id.to_string(),
        })
        .await?;
    assert!(claim.claimed);
    assert!(claim.execution.is_some_and(|snapshot| snapshot.dry_run));
    assert!(
        store
            .complete_attempt(&CompletionRequest {
                attempt_id,
                worker_id: worker_id.to_string(),
                succeeded: true,
                skipped: true,
                exit_code: None,
                stdout_tail: "DRY RUN (not executed): \"/bin/echo\" \"hello world\"".to_string(),
                stderr_tail: String::new(),
                error: None,
            })
            .await?
    );
    let (run_status, reason, retry_at): (String, Option<String>, Option<time::OffsetDateTime>) =
        sqlx::query_as(
            "SELECT status, terminal_reason, next_retry_at FROM crono.runs WHERE id = $1",
        )
        .bind(run_id)
        .fetch_one(pool)
        .await?;
    assert_eq!(run_status, "skipped");
    assert_eq!(reason.as_deref(), Some("dry run"));
    assert!(retry_at.is_none());
    let (attempt_status, output): (String, Option<String>) =
        sqlx::query_as("SELECT status, stdout_tail FROM crono.run_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_one(pool)
            .await?;
    assert_eq!(attempt_status, "skipped");
    assert!(output.is_some_and(|value| value.contains("hello world")));

    Ok(())
}

/// Remove only the resources created by this test, preserving shared Queue state.
async fn cleanup(pool: &PgPool, namespace_id: Uuid, job_id: Uuid) -> Result<()> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        "DELETE FROM crono.outbox WHERE run_id IN (SELECT id FROM crono.runs WHERE job_id = $1)",
    )
    .bind(job_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM crono.run_events WHERE run_id IN (SELECT id FROM crono.runs WHERE job_id = $1)")
        .bind(job_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.run_attempts WHERE run_id IN (SELECT id FROM crono.runs WHERE job_id = $1)")
        .bind(job_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.runs WHERE job_id = $1")
        .bind(job_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.run_requests WHERE namespace_id = $1")
        .bind(namespace_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.targets WHERE namespace_id = $1")
        .bind(namespace_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.jobs WHERE namespace_id = $1")
        .bind(namespace_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("DELETE FROM crono.namespaces WHERE id = $1")
        .bind(namespace_id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}
