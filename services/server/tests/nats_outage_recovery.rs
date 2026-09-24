//! Real PostgreSQL and NATS recovery test.
//!
//! The ignored test intentionally stops the named development NATS container.
//! Run it through `just integration-test` so the canonical schema and both
//! services are available. It verifies that scheduling proceeds during the
//! outage and that only eligible persisted work executes after reconnect.

use ::time::{Duration as TimeDuration, OffsetDateTime};
use anyhow::{Context, Result, anyhow, bail};
use async_nats::jetstream::{self, consumer::pull};
use crono_api::{ClaimRequest, ClaimResponse, CompletionRequest, DispatchEnvelope};
use crono_server::{
    application::{ControlPlaneStore, JobDefinition, NewSchedule, TargetDefinition},
    domain::{
        CatchupPolicy, ExecutorKind, JobId, MisfirePolicy, NamespaceId, NamespaceName, QueueName,
        ResourceName, TargetId, TargetSelection,
    },
    infrastructure::{
        DispatcherConfig, NatsPublisher, PostgresStore, run_dispatcher, run_worker_control,
    },
    reconciliation::run_reconciler,
    scheduler::run_scheduler,
};
use futures_util::StreamExt;
use sqlx::PgPool;
use std::{env, sync::Arc, time::Duration};
use tokio::{process::Command, time};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DATABASE_URL: &str = "postgres://crono_runtime:change-me@127.0.0.1:5432/crono";
const NATS_URL: &str = "nats://127.0.0.1:4222";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "stops the real crono-nats container; run with just integration-test"]
// Keeping the destructive outage phases in one serialized test prevents two
// test processes from racing the shared development NATS container.
#[allow(clippy::too_many_lines)]
async fn nats_outage_and_publisher_crash_preserve_logical_execution() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL").unwrap_or_else(|_| DATABASE_URL.into());
    let nats_url = env::var("CRONO_TEST_NATS_URL").unwrap_or_else(|_| NATS_URL.into());
    container("start").await?;

    let postgres = PostgresStore::connect(&database_url).await?;
    let store: Arc<dyn ControlPlaneStore> = Arc::new(postgres);
    let pool = PgPool::connect(&database_url).await?;
    let publisher = NatsPublisher::new(&nats_url);
    let cancellation = CancellationToken::new();
    let manager = tokio::spawn({
        let publisher = publisher.clone();
        let cancellation = cancellation.child_token();
        async move { publisher.run_connection_manager(cancellation).await }
    });
    wait_for_transport(&publisher).await?;
    container("stop").await?;

    let suffix = Uuid::now_v7().simple().to_string();
    let namespace =
        NamespaceName::parse(&format!("outage-{}", suffix.get(..12).unwrap_or("test")))?;
    let queue_name = QueueName::parse(&format!("outage-{}", suffix.get(..12).unwrap_or("test")))?;
    let job = ResourceName::parse("noop")?;
    let unsafe_job = ResourceName::parse("unsafe-noop")?;
    let target = ResourceName::parse("local")?;
    let namespace_record = store.create_namespace(&namespace).await?;
    let queue_record = store.create_queue(&queue_name, None).await?;
    let queue_id = queue_record.id();
    let queue_routing_id = queue_id.get().to_string();
    let namespace_id = namespace_record.id();
    let job_record = store
        .create_job(
            namespace_id,
            &job,
            &JobDefinition {
                executor: ExecutorKind::Noop,
                queue_id,
                executable: None,
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
                idempotent: true,
                max_attempts: 2,
                retry_initial_seconds: 1,
                retry_max_seconds: 10,
                retry_multiplier: 2.0,
                retry_jitter: 0.2,
            },
        )
        .await?;
    let unsafe_job_record = store
        .create_job(
            namespace_id,
            &unsafe_job,
            &JobDefinition {
                executor: ExecutorKind::Noop,
                queue_id,
                executable: None,
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
                idempotent: false,
                max_attempts: 2,
                retry_initial_seconds: 1,
                retry_max_seconds: 10,
                retry_multiplier: 2.0,
                retry_jitter: 0.2,
            },
        )
        .await?;
    let target_record = store
        .create_target(
            namespace_id,
            &target,
            &TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let job_id = job_record.job.id();
    let unsafe_job_id = unsafe_job_record.job.id();
    let target_id = target_record.target.id();

    let scheduler = tokio::spawn(run_scheduler(
        Arc::clone(&store),
        cancellation.child_token(),
    ));
    let reconciler = tokio::spawn(run_reconciler(
        Arc::clone(&store),
        cancellation.child_token(),
    ));
    let dispatcher_cancellation = cancellation.child_token();
    let dispatcher = tokio::spawn(run_dispatcher(
        Arc::clone(&store),
        publisher.clone(),
        DispatcherConfig::default(),
        dispatcher_cancellation.clone(),
    ));
    let control = tokio::spawn(run_worker_control(
        Arc::clone(&store),
        publisher.clone(),
        cancellation.child_token(),
    ));

    let now = OffsetDateTime::now_utc();
    create_once(
        &store,
        namespace_id,
        job_id,
        target_id,
        "late",
        now - TimeDuration::seconds(31),
        MisfirePolicy::RunLate,
        None,
    )
    .await?;
    create_once(
        &store,
        namespace_id,
        job_id,
        target_id,
        "skip",
        now - TimeDuration::seconds(31),
        MisfirePolicy::Skip,
        None,
    )
    .await?;
    create_once(
        &store,
        namespace_id,
        job_id,
        target_id,
        "grace",
        now + TimeDuration::seconds(1),
        MisfirePolicy::GracePeriod,
        Some(2),
    )
    .await?;

    time::sleep(Duration::from_secs(7)).await;
    let before = statuses(&pool, namespace.as_str()).await?;
    assert!(
        before
            .iter()
            .any(|(name, status)| name == "late" && status == "pending_dispatch")
    );
    assert!(
        before
            .iter()
            .any(|(name, status)| name == "skip" && status == "skipped")
    );
    assert!(
        before
            .iter()
            .any(|(name, status)| name == "grace" && status == "skipped")
    );

    container("start").await?;
    wait_for_transport(&publisher).await?;
    execute_one(&nats_url, &queue_routing_id).await?;
    wait_for_status(&pool, namespace.as_str(), "late", "succeeded").await?;

    dispatcher_cancellation.cancel();
    dispatcher.await?;
    verify_publisher_crash_window(
        &store,
        &pool,
        &publisher,
        &nats_url,
        job_id,
        target_id,
        &queue_routing_id,
    )
    .await?;
    verify_worker_crash_windows(
        &store,
        &pool,
        &publisher,
        &nats_url,
        job_id,
        unsafe_job_id,
        target_id,
        &queue_routing_id,
    )
    .await?;

    cancellation.cancel();
    for task in [scheduler, reconciler, control, manager] {
        task.await?;
    }
    Ok(())
}

// The explicit resources make cross-namespace and cross-queue mistakes visible
// in this protocol test instead of hiding them in global test state.
#[allow(clippy::too_many_arguments)]
async fn verify_worker_crash_windows(
    store: &Arc<dyn ControlPlaneStore>,
    pool: &PgPool,
    publisher: &NatsPublisher,
    nats_url: &str,
    job_id: JobId,
    unsafe_job_id: JobId,
    target_id: TargetId,
    queue: &str,
) -> Result<()> {
    let (before_execution, created) = store.create_run(Uuid::now_v7(), job_id, target_id).await?;
    assert!(created);
    publish_next(store, publisher).await?;
    let crashed_attempt = claim_then_nak(nats_url, queue, "crash-before-execution").await?;
    sqlx::query(
        "UPDATE crono.run_attempts
            SET lease_expires_at = statement_timestamp() - interval '1 second'
          WHERE id = $1",
    )
    .bind(crashed_attempt)
    .execute(pool)
    .await?;
    store.reconcile(100).await?;
    retire_completed_redelivery(nats_url, queue, "expired-lease-redelivery").await?;
    publish_next(store, publisher).await?;
    execute_one(nats_url, queue).await?;
    let attempts: (String, i64) = sqlx::query_as(
        "SELECT r.status, count(a.id)
           FROM crono.runs r JOIN crono.run_attempts a ON a.run_id = r.id
          WHERE r.id = $1 GROUP BY r.status",
    )
    .bind(before_execution.run.id().get())
    .fetch_one(pool)
    .await?;
    assert_eq!(attempts, ("succeeded".to_string(), 2));

    let (before_ack, created) = store.create_run(Uuid::now_v7(), job_id, target_id).await?;
    assert!(created);
    publish_next(store, publisher).await?;
    complete_then_nak(nats_url, queue, "crash-before-ack").await?;
    retire_completed_redelivery(nats_url, queue, "redelivery-worker").await?;
    let state: (String, i64) = sqlx::query_as(
        "SELECT r.status, count(a.id)
           FROM crono.runs r JOIN crono.run_attempts a ON a.run_id = r.id
          WHERE r.id = $1 GROUP BY r.status",
    )
    .bind(before_ack.run.id().get())
    .fetch_one(pool)
    .await?;
    assert_eq!(state, ("succeeded".to_string(), 1));

    let (ambiguous, created) = store
        .create_run(Uuid::now_v7(), unsafe_job_id, target_id)
        .await?;
    assert!(created);
    publish_next(store, publisher).await?;
    let attempt = claim_then_nak(nats_url, queue, "non-idempotent-crash").await?;
    sqlx::query(
        "UPDATE crono.run_attempts
            SET lease_expires_at = statement_timestamp() - interval '1 second'
          WHERE id = $1",
    )
    .bind(attempt)
    .execute(pool)
    .await?;
    store.reconcile(100).await?;
    retire_completed_redelivery(nats_url, queue, "unknown-redelivery").await?;
    let state: (String, i64) = sqlx::query_as(
        "SELECT r.status, count(a.id)
           FROM crono.runs r JOIN crono.run_attempts a ON a.run_id = r.id
          WHERE r.id = $1 GROUP BY r.status",
    )
    .bind(ambiguous.run.id().get())
    .fetch_one(pool)
    .await?;
    assert_eq!(state, ("unknown".to_string(), 1));
    Ok(())
}

async fn publish_next(store: &Arc<dyn ControlPlaneStore>, publisher: &NatsPublisher) -> Result<()> {
    let owner = Uuid::now_v7();
    let record = store
        .claim_outbox(owner, 1, Duration::from_secs(60))
        .await?
        .into_iter()
        .next()
        .context("expected a pending outbox row")?;
    let sequence = publisher
        .publish(
            record.subject.clone(),
            record.payload.clone(),
            record.attempt_id.get(),
        )
        .await?;
    store.mark_published(owner, &record, sequence).await?;
    Ok(())
}

async fn claim_then_nak(nats_url: &str, queue: &str, worker_id: &str) -> Result<Uuid> {
    let (client, message, envelope) = receive_dispatch(nats_url, queue).await?;
    let claim = claim(&client, &envelope, worker_id).await?;
    if !claim.claimed {
        bail!("fresh dispatch could not be claimed");
    }
    message
        .ack_with(jetstream::message::AckKind::Nak(None))
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    client.flush().await?;
    Ok(envelope.attempt_id)
}

async fn complete_then_nak(nats_url: &str, queue: &str, worker_id: &str) -> Result<()> {
    let (client, message, envelope) = receive_dispatch(nats_url, queue).await?;
    let claim = claim(&client, &envelope, worker_id).await?;
    if !claim.claimed {
        bail!("fresh dispatch could not be claimed");
    }
    complete(&client, envelope.attempt_id, worker_id).await?;
    message
        .ack_with(jetstream::message::AckKind::Nak(None))
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    client.flush().await?;
    Ok(())
}

async fn retire_completed_redelivery(nats_url: &str, queue: &str, worker_id: &str) -> Result<()> {
    let (client, message, envelope) = receive_dispatch(nats_url, queue).await?;
    let claim = claim(&client, &envelope, worker_id).await?;
    if claim.claimed {
        bail!("completed Attempt was claimed again on redelivery");
    }
    message
        .ack()
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    client.flush().await?;
    Ok(())
}

// The explicit resources keep every persisted and transport identity available
// for the publisher crash-window assertions.
#[allow(clippy::too_many_arguments)]
async fn verify_publisher_crash_window(
    store: &Arc<dyn ControlPlaneStore>,
    pool: &PgPool,
    publisher: &NatsPublisher,
    nats_url: &str,
    job_id: JobId,
    target_id: TargetId,
    queue: &str,
) -> Result<()> {
    let (run, created) = store.create_run(Uuid::now_v7(), job_id, target_id).await?;
    assert!(created);

    let first_owner = Uuid::now_v7();
    let first = store
        .claim_outbox(first_owner, 1, Duration::from_secs(60))
        .await?
        .into_iter()
        .next()
        .context("manual Run has no outbox row")?;
    publisher
        .publish(
            first.subject.clone(),
            first.payload.clone(),
            first.attempt_id.get(),
        )
        .await?;

    sqlx::query("UPDATE crono.outbox SET claim_expires_at = statement_timestamp() - interval '1 second' WHERE id = $1")
        .bind(first.id.get())
        .execute(pool)
        .await?;
    store.reconcile(100).await?;

    let second_owner = Uuid::now_v7();
    let second = store
        .claim_outbox(second_owner, 1, Duration::from_secs(60))
        .await?
        .into_iter()
        .next()
        .context("publisher restart did not reclaim the outbox row")?;
    assert_eq!(first.attempt_id, second.attempt_id);
    let sequence = publisher
        .publish(
            second.subject.clone(),
            second.payload.clone(),
            second.attempt_id.get(),
        )
        .await?;
    store
        .mark_published(second_owner, &second, sequence)
        .await?;
    execute_one(nats_url, queue).await?;

    let state: (String, i64) = sqlx::query_as(
        "SELECT r.status, count(a.id)
           FROM crono.runs r
           JOIN crono.run_attempts a ON a.run_id = r.id
          WHERE r.id = $1
          GROUP BY r.status",
    )
    .bind(run.run.id().get())
    .fetch_one(pool)
    .await?;
    assert_eq!(state, ("succeeded".to_string(), 1));
    Ok(())
}

// Each policy input remains explicit so the three outage cases are readable at
// their call sites and cannot inherit mutable fixture defaults.
#[allow(clippy::too_many_arguments)]
async fn create_once(
    store: &Arc<dyn ControlPlaneStore>,
    namespace_id: NamespaceId,
    job_id: JobId,
    target_id: TargetId,
    name: &str,
    execute_at: OffsetDateTime,
    misfire_policy: MisfirePolicy,
    misfire_grace_seconds: Option<u32>,
) -> Result<()> {
    store
        .create_schedule(&NewSchedule {
            namespace_id,
            name: ResourceName::parse(name)?,
            job_id,
            target: TargetSelection::Target(target_id),
            inputs: serde_json::json!({}),
            cron_expression: None,
            execute_at: Some(execute_at),
            timezone: "UTC".to_string(),
            next_run_at: execute_at,
            misfire_policy,
            misfire_grace_seconds,
            catchup_policy: CatchupPolicy::RunOnce,
            max_catchup_runs: 100,
            max_catchup_age_seconds: 86_400,
        })
        .await?;
    Ok(())
}

async fn execute_one(nats_url: &str, queue: &str) -> Result<()> {
    let (client, message, envelope) = receive_dispatch(nats_url, queue).await?;
    let worker_id = "outage-test-worker";
    let claim = claim(&client, &envelope, worker_id).await?;
    if !claim.claimed {
        bail!("recovered dispatch could not be claimed");
    }
    complete(&client, envelope.attempt_id, worker_id).await?;
    message
        .ack()
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    Ok(())
}

async fn receive_dispatch(
    nats_url: &str,
    queue: &str,
) -> Result<(async_nats::Client, jetstream::Message, DispatchEnvelope)> {
    let client = async_nats::connect(nats_url).await?;
    let stream = jetstream::new(client.clone())
        .get_stream("CRONO_DISPATCH")
        .await?;
    let durable = format!("outage-test-{queue}");
    let consumer = stream
        .get_or_create_consumer(
            &durable,
            pull::Config {
                durable_name: Some(durable.clone()),
                filter_subject: format!("crono.dispatch.{queue}"),
                ..pull::Config::default()
            },
        )
        .await?;
    let mut messages = consumer.fetch().max_messages(1).messages().await?;
    let delivery = time::timeout(Duration::from_secs(10), messages.next())
        .await?
        .context("no dispatch arrived after NATS recovery")?;
    let message = delivery.map_err(|error| anyhow!(error.to_string()))?;
    let envelope: DispatchEnvelope = serde_json::from_slice(&message.payload)?;
    Ok((client, message, envelope))
}

async fn claim(
    client: &async_nats::Client,
    envelope: &DispatchEnvelope,
    worker_id: &str,
) -> Result<ClaimResponse> {
    request(
        client,
        &format!("crono.worker.control.claim.{worker_id}"),
        &ClaimRequest {
            run_id: envelope.run_id,
            attempt_id: envelope.attempt_id,
            queue_id: envelope.queue_id,
            worker_id: worker_id.to_string(),
        },
    )
    .await
}

async fn complete(client: &async_nats::Client, attempt_id: Uuid, worker_id: &str) -> Result<()> {
    let completed: bool = request(
        client,
        &format!("crono.worker.control.complete.{worker_id}"),
        &CompletionRequest {
            attempt_id,
            worker_id: worker_id.to_string(),
            succeeded: true,
            exit_code: Some(0),
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: None,
        },
    )
    .await?;
    if !completed {
        bail!("recovered dispatch completion was rejected");
    }
    Ok(())
}

async fn request<T, R>(client: &async_nats::Client, subject: &str, value: &T) -> Result<R>
where
    T: serde::Serialize,
    R: serde::de::DeserializeOwned,
{
    let response = time::timeout(
        Duration::from_secs(5),
        client.request(subject.to_string(), serde_json::to_vec(value)?.into()),
    )
    .await??;
    Ok(serde_json::from_slice(&response.payload)?)
}

async fn wait_for_transport(publisher: &NatsPublisher) -> Result<()> {
    for _ in 0..30 {
        if publisher.ready().await {
            return Ok(());
        }
        time::sleep(Duration::from_millis(500)).await;
    }
    bail!("JetStream transport did not become ready")
}

async fn wait_for_status(
    pool: &PgPool,
    namespace: &str,
    schedule: &str,
    wanted: &str,
) -> Result<()> {
    for _ in 0..40 {
        let rows = statuses(pool, namespace).await?;
        if rows
            .iter()
            .any(|(name, status)| name == schedule && status == wanted)
        {
            return Ok(());
        }
        time::sleep(Duration::from_millis(250)).await;
    }
    bail!("schedule {schedule} did not reach {wanted}")
}

async fn statuses(pool: &PgPool, namespace: &str) -> Result<Vec<(String, String)>> {
    Ok(sqlx::query_as(
        "SELECT s.name, r.status
           FROM crono.runs r
           JOIN crono.schedules s ON s.id = r.schedule_id
           JOIN crono.namespaces n ON n.id = s.namespace_id
          WHERE n.name = $1
          ORDER BY s.name",
    )
    .bind(namespace)
    .fetch_all(pool)
    .await?)
}

async fn container(operation: &str) -> Result<()> {
    let status = Command::new("podman")
        .args([operation, "crono-nats"])
        .status()
        .await
        .context("failed to invoke podman")?;
    if !status.success() {
        bail!("podman {operation} crono-nats failed");
    }
    Ok(())
}
