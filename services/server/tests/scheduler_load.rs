//! Opt-in bounded scheduler load tooling backed by real PostgreSQL.
//!
//! The workload bulk-loads due one-shot Schedules, runs four ordinary scheduler
//! loops, and reports durable Run/outbox creation throughput. It defaults to
//! 100,000 occurrences and never starts one Tokio task per Schedule.

use anyhow::{Context, Result, bail};
use crono_server::{
    application::ControlPlaneStore, infrastructure::PostgresStore, scheduler::run_scheduler,
};
use sqlx::PgPool;
use std::{env, sync::Arc, time::Duration};
use tokio::time;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DATABASE_URL: &str = "postgres://crono_runtime:change-me@127.0.0.1:5432/crono";

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "creates 100,000 durable occurrences; run with just load-test"]
async fn creates_bounded_due_backlog() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL").unwrap_or_else(|_| DATABASE_URL.into());
    let total: u32 = env::var("CRONO_LOAD_RUNS")
        .map_or_else(|_| Ok(100_000), |value| value.parse())
        .context("invalid CRONO_LOAD_RUNS")?;
    if !(1..=1_000_000).contains(&total) {
        bail!("CRONO_LOAD_RUNS must be between 1 and 1000000");
    }

    let pool = PgPool::connect(&database_url).await?;
    let namespace = format!("load-{}", Uuid::now_v7().simple());
    let namespace_id: Uuid =
        sqlx::query_scalar("INSERT INTO crono.namespaces (name) VALUES ($1) RETURNING id")
            .bind(&namespace)
            .fetch_one(&pool)
            .await?;
    let job_id: Uuid = sqlx::query_scalar(
        "INSERT INTO crono.jobs (namespace_id, name, executor, queue, idempotent)
         VALUES ($1, 'load', 'noop', 'load', true) RETURNING id",
    )
    .bind(namespace_id)
    .fetch_one(&pool)
    .await?;
    let target_id: Uuid = sqlx::query_scalar(
        "INSERT INTO crono.targets (namespace_id, name) VALUES ($1, 'local') RETURNING id",
    )
    .bind(namespace_id)
    .fetch_one(&pool)
    .await?;
    sqlx::query(
        "INSERT INTO crono.schedules (
             namespace_id, job_id, target_id, name, schedule_type,
             execute_at, next_run_at, misfire_policy
         )
         SELECT $1, $2, $3, 'load-' || lpad(value::text, 7, '0'), 'once',
                statement_timestamp() - interval '1 second',
                statement_timestamp() - interval '1 second', 'run_late'
           FROM generate_series(1, $4::bigint) AS value",
    )
    .bind(namespace_id)
    .bind(job_id)
    .bind(target_id)
    .bind(i64::from(total))
    .execute(&pool)
    .await?;

    let store: Arc<dyn ControlPlaneStore> = Arc::new(PostgresStore::connect(&database_url).await?);
    let cancellation = CancellationToken::new();
    let schedulers = (0..4)
        .map(|_| {
            tokio::spawn(run_scheduler(
                Arc::clone(&store),
                cancellation.child_token(),
            ))
        })
        .collect::<Vec<_>>();
    let started = std::time::Instant::now();
    loop {
        let created: i64 = sqlx::query_scalar("SELECT count(*) FROM crono.runs WHERE job_id = $1")
            .bind(job_id)
            .fetch_one(&pool)
            .await?;
        if created == i64::from(total) {
            break;
        }
        if started.elapsed() > Duration::from_secs(600) {
            bail!("scheduler load run exceeded ten minutes at {created}/{total}");
        }
        time::sleep(Duration::from_millis(100)).await;
    }
    cancellation.cancel();
    for scheduler in schedulers {
        scheduler.await?;
    }

    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), count(DISTINCT (schedule_id, scheduled_at)),
                (SELECT count(*) FROM crono.outbox o
                  JOIN crono.runs r ON r.id = o.run_id WHERE r.job_id = $1)
           FROM crono.runs WHERE job_id = $1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        counts,
        (i64::from(total), i64::from(total), i64::from(total))
    );
    let elapsed = started.elapsed().as_secs_f64();
    let throughput = f64::from(total) / elapsed;
    eprintln!(
        "created {total} durable Runs and outbox rows in {elapsed:.3}s ({throughput:.0} intents/s)"
    );
    Ok(())
}
