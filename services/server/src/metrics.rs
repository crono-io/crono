//! Bounded-cardinality Prometheus metrics.
//!
//! Metrics describe subsystem health and throughput only. Resource UUIDs stay
//! in structured logs so the registry cannot grow with user-controlled data.

use prometheus_client::{
    encoding::text::encode,
    metrics::{
        counter::Counter,
        gauge::Gauge,
        histogram::{Histogram, exponential_buckets},
    },
    registry::Registry,
};
use std::sync::OnceLock;

pub struct Metrics {
    registry: Registry,
    pub scheduler_due: Counter,
    pub scheduler_misfire: Counter,
    pub scheduler_skipped: Counter,
    pub outbox_publish: Counter,
    pub outbox_publish_failure: Counter,
    pub outbox_publish_latency: Histogram,
    pub outbox_pending: Gauge,
    pub outbox_oldest_seconds: Gauge,
    pub execution_queued: Gauge,
    pub execution_running: Gauge,
    pub execution_lateness: Histogram,
    pub execution_success: Counter,
    pub execution_failure: Counter,
    pub execution_retry: Counter,
    pub worker_lease_expired: Counter,
    pub worker_active: Gauge,
    pub nats_connected: Gauge,
}

impl Metrics {
    fn new() -> Self {
        let mut registry = Registry::default();
        let scheduler_due = Counter::default();
        let scheduler_misfire = Counter::default();
        let scheduler_skipped = Counter::default();
        let outbox_publish = Counter::default();
        let outbox_publish_failure = Counter::default();
        let outbox_publish_latency = Histogram::new(exponential_buckets(0.001, 2.0, 16));
        let outbox_pending = Gauge::default();
        let outbox_oldest_seconds = Gauge::default();
        let execution_queued = Gauge::default();
        let execution_running = Gauge::default();
        let execution_lateness = Histogram::new(exponential_buckets(1.0, 2.0, 24));
        let execution_success = Counter::default();
        let execution_failure = Counter::default();
        let execution_retry = Counter::default();
        let worker_lease_expired = Counter::default();
        let worker_active = Gauge::default();
        let nats_connected = Gauge::default();
        register_scheduler(
            &mut registry,
            &scheduler_due,
            &scheduler_misfire,
            &scheduler_skipped,
        );
        register_outbox(
            &mut registry,
            &outbox_publish,
            &outbox_publish_failure,
            &outbox_publish_latency,
            &outbox_pending,
            &outbox_oldest_seconds,
        );
        register_execution(
            &mut registry,
            &execution_queued,
            &execution_running,
            &execution_lateness,
            &execution_success,
            &execution_failure,
            &execution_retry,
        );
        register_worker(
            &mut registry,
            &worker_lease_expired,
            &worker_active,
            &nats_connected,
        );
        Self {
            registry,
            scheduler_due,
            scheduler_misfire,
            scheduler_skipped,
            outbox_publish,
            outbox_publish_failure,
            outbox_publish_latency,
            outbox_pending,
            outbox_oldest_seconds,
            execution_queued,
            execution_running,
            execution_lateness,
            execution_success,
            execution_failure,
            execution_retry,
            worker_lease_expired,
            worker_active,
            nats_connected,
        }
    }

    /// Encode the current registry in `OpenMetrics` text format.
    ///
    /// # Errors
    ///
    /// Returns when formatting the in-memory registry fails.
    pub fn encode(&self) -> Result<String, std::fmt::Error> {
        let mut output = String::new();
        encode(&mut output, &self.registry)?;
        Ok(output)
    }
}

fn register_scheduler(
    registry: &mut Registry,
    due: &Counter,
    misfire: &Counter,
    skipped: &Counter,
) {
    registry.register(
        "crono_scheduler_due",
        "Due occurrences observed",
        due.clone(),
    );
    registry.register(
        "crono_scheduler_misfire",
        "Late occurrences observed",
        misfire.clone(),
    );
    registry.register(
        "crono_scheduler_skipped",
        "Occurrences skipped",
        skipped.clone(),
    );
}

fn register_outbox(
    registry: &mut Registry,
    publish: &Counter,
    failure: &Counter,
    latency: &Histogram,
    pending: &Gauge,
    oldest: &Gauge,
) {
    registry.register(
        "crono_outbox_publish",
        "Outbox messages acknowledged",
        publish.clone(),
    );
    registry.register(
        "crono_outbox_publish_failure",
        "Outbox publication failures",
        failure.clone(),
    );
    registry.register(
        "crono_outbox_publish_latency_seconds",
        "JetStream publish latency",
        latency.clone(),
    );
    registry.register(
        "crono_outbox_pending",
        "Unpublished durable outbox messages",
        pending.clone(),
    );
    registry.register(
        "crono_outbox_oldest_seconds",
        "Age of the oldest pending outbox message",
        oldest.clone(),
    );
}

fn register_execution(
    registry: &mut Registry,
    queued: &Gauge,
    running: &Gauge,
    lateness: &Histogram,
    success: &Counter,
    failure: &Counter,
    retry: &Counter,
) {
    registry.register(
        "crono_execution_queued",
        "Queued logical executions",
        queued.clone(),
    );
    registry.register(
        "crono_execution_running",
        "Running logical executions",
        running.clone(),
    );
    registry.register(
        "crono_execution_lateness_seconds",
        "Scheduled execution lateness",
        lateness.clone(),
    );
    registry.register(
        "crono_execution_success",
        "Successful executions",
        success.clone(),
    );
    registry.register(
        "crono_execution_failure",
        "Failed executions",
        failure.clone(),
    );
    registry.register(
        "crono_execution_retry",
        "Execution retries created",
        retry.clone(),
    );
}

fn register_worker(
    registry: &mut Registry,
    lease_expired: &Counter,
    active: &Gauge,
    connected: &Gauge,
) {
    registry.register(
        "crono_worker_lease_expired",
        "Expired worker leases",
        lease_expired.clone(),
    );
    registry.register(
        "crono_worker_active",
        "Workers holding valid execution leases",
        active.clone(),
    );
    registry.register(
        "crono_nats_connected",
        "NATS connection state",
        connected.clone(),
    );
}

/// Return the process-wide bounded metric registry.
#[must_use]
pub fn global() -> &'static Metrics {
    static METRICS: OnceLock<Metrics> = OnceLock::new();
    METRICS.get_or_init(Metrics::new)
}
