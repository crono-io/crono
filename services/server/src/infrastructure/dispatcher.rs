//! Bounded transactional-outbox publisher.
//!
//! Multiple instances claim rows with PostgreSQL leases, publish concurrently
//! within a fixed limit, await `JetStream` persistence, and only then mark Runs
//! queued. Failures release claims with exponential backoff and deterministic
//! jitter; they never consume an execution attempt.

use super::NatsPublisher;
use crate::application::{ControlPlaneStore, OutboxRecord};
use anyhow::{Context, Result, bail};
use futures_util::{StreamExt, stream};
use std::{env, sync::Arc, time::Duration};
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::time::{self as tokio_time, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatcherConfig {
    pub batch_size: u16,
    pub max_in_flight: usize,
    pub claim_lease: Duration,
    pub retry_initial: Duration,
    pub retry_max: Duration,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            max_in_flight: 32,
            claim_lease: Duration::from_secs(60),
            retry_initial: Duration::from_secs(1),
            retry_max: Duration::from_secs(60),
        }
    }
}

impl DispatcherConfig {
    /// Load bounded publisher controls from `CRONO_OUTBOX_*` environment variables.
    ///
    /// # Errors
    ///
    /// Returns when a configured value is malformed or outside its safe range.
    pub fn from_env() -> Result<Self> {
        let defaults = Self::default();
        let batch_size = env_value("CRONO_OUTBOX_BATCH_SIZE", defaults.batch_size)?;
        let max_in_flight = env_value("CRONO_OUTBOX_MAX_IN_FLIGHT", defaults.max_in_flight)?;
        let claim_lease_seconds = env_value(
            "CRONO_OUTBOX_CLAIM_LEASE_SECONDS",
            defaults.claim_lease.as_secs(),
        )?;
        let retry_initial_ms = env_value(
            "CRONO_OUTBOX_RETRY_INITIAL_MS",
            u64::try_from(defaults.retry_initial.as_millis()).unwrap_or(1_000),
        )?;
        let retry_max_ms = env_value(
            "CRONO_OUTBOX_RETRY_MAX_MS",
            u64::try_from(defaults.retry_max.as_millis()).unwrap_or(60_000),
        )?;
        if !(1..=1_000).contains(&batch_size)
            || !(1..=1_024).contains(&max_in_flight)
            || !(10..=600).contains(&claim_lease_seconds)
            || !(100..=60_000).contains(&retry_initial_ms)
            || retry_max_ms < retry_initial_ms
            || retry_max_ms > 3_600_000
        {
            bail!("outbox publisher configuration is outside safe bounds");
        }
        Ok(Self {
            batch_size,
            max_in_flight,
            claim_lease: Duration::from_secs(claim_lease_seconds),
            retry_initial: Duration::from_millis(retry_initial_ms),
            retry_max: Duration::from_millis(retry_max_ms),
        })
    }
}

/// Deliver committed outbox rows until process shutdown is requested.
pub async fn run_dispatcher(
    store: Arc<dyn ControlPlaneStore>,
    publisher: NatsPublisher,
    config: DispatcherConfig,
    cancellation: CancellationToken,
) {
    let owner = Uuid::now_v7();
    let mut interval = tokio_time::interval(Duration::from_millis(100));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                info!("execution dispatch loop stopped");
                return;
            }
            _ = interval.tick() => dispatch_batch(Arc::clone(&store), &publisher, owner, config).await,
        }
    }
}

async fn dispatch_batch(
    store: Arc<dyn ControlPlaneStore>,
    publisher: &NatsPublisher,
    owner: Uuid,
    config: DispatcherConfig,
) {
    let records = match store
        .claim_outbox(owner, config.batch_size, config.claim_lease)
        .await
    {
        Ok(records) => records,
        Err(error) => {
            warn!(%error, "failed to claim execution outbox rows");
            return;
        }
    };
    stream::iter(records)
        .for_each_concurrent(config.max_in_flight, |record| {
            publish_one(Arc::clone(&store), publisher.clone(), owner, record, config)
        })
        .await;
}

async fn publish_one(
    store: Arc<dyn ControlPlaneStore>,
    publisher: NatsPublisher,
    owner: Uuid,
    record: OutboxRecord,
    config: DispatcherConfig,
) {
    let started = std::time::Instant::now();
    match publisher
        .publish(
            record.subject.clone(),
            record.payload.clone(),
            record.attempt_id.get(),
        )
        .await
    {
        Ok(sequence) => {
            crate::metrics::global().outbox_publish.inc();
            crate::metrics::global()
                .outbox_publish_latency
                .observe(started.elapsed().as_secs_f64());
            if let Err(error) = store.mark_published(owner, &record, sequence).await {
                error!(
                    %error,
                    dispatch_id = %record.id.get(),
                    run_id = %record.run_id.get(),
                    "JetStream acknowledged dispatch but PostgreSQL update failed; safe republication will follow"
                );
            }
        }
        Err(publish_error) => {
            crate::metrics::global().outbox_publish_failure.inc();
            let next_attempt_at = OffsetDateTime::now_utc() + retry_delay(&record, config);
            warn!(
                error = %publish_error,
                dispatch_id = %record.id.get(),
                "execution dispatch remains durable in PostgreSQL"
            );
            if let Err(store_error) = store
                .record_publish_failure(
                    owner,
                    record.id,
                    &publish_error.to_string(),
                    next_attempt_at,
                )
                .await
            {
                error!(%store_error, dispatch_id = %record.id.get(), "failed to release outbox claim");
            }
        }
    }
}

fn retry_delay(record: &OutboxRecord, config: DispatcherConfig) -> TimeDuration {
    let exponent = record.attempt_count.min(6);
    let multiplier = 1_u64 << exponent;
    let base = u64::try_from(config.retry_initial.as_millis())
        .unwrap_or(1_000)
        .saturating_mul(multiplier)
        .min(u64::try_from(config.retry_max.as_millis()).unwrap_or(60_000));
    let jitter_seed = record.id.get().as_bytes().last().copied().unwrap_or(20);
    let jitter_percent = i64::from(jitter_seed % 41) - 20;
    let millis = i64::try_from(base).unwrap_or(60_000);
    let jitter = millis.saturating_mul(jitter_percent) / 100;
    let minimum = i64::try_from(config.retry_initial.as_millis()).unwrap_or(1_000) * 4 / 5;
    let maximum = i64::try_from(config.retry_max.as_millis()).unwrap_or(60_000);
    TimeDuration::milliseconds((millis + jitter).clamp(minimum, maximum))
}

fn env_value<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    env::var(name).map_or_else(
        |_| Ok(default),
        |value| value.parse().with_context(|| format!("invalid {name}")),
    )
}

#[cfg(test)]
mod tests {
    use super::{DispatcherConfig, retry_delay};
    use crate::{
        application::OutboxRecord,
        domain::{AttemptId, DispatchId, RunId},
    };
    use uuid::Uuid;

    #[test]
    fn dispatch_backoff_is_bounded_and_grows() {
        let mut record = OutboxRecord {
            id: DispatchId::new(Uuid::from_u128(255)),
            run_id: RunId::new(Uuid::from_u128(1)),
            attempt_id: AttemptId::new(Uuid::from_u128(2)),
            subject: "crono.dispatch.default".to_string(),
            payload: Vec::new(),
            attempt_count: 0,
        };
        let config = DispatcherConfig::default();
        let first = retry_delay(&record, config);
        record.attempt_count = 20;
        let capped = retry_delay(&record, config);
        assert!(first.whole_milliseconds() >= 800);
        assert!(capped.whole_seconds() <= 60);
        assert!(capped > first);
    }
}
