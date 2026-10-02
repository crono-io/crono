//! Bounded repair and Workflow completion-event processing.
//!
//! Every five seconds a pass repairs leases/retries, then consumes durable
//! Workflow completion events. Only invocations with queued events are visited;
//! DAGs are not rescanned continuously. Transactions and row locks make both
//! paths safe on multiple servers and preserve progress across process restarts.

use crate::application::ControlPlaneStore;
use std::{sync::Arc, time::Duration};
use tokio::time::{self, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Reconcile stale durable state until shutdown.
pub async fn run_reconciler(store: Arc<dyn ControlPlaneStore>, cancellation: CancellationToken) {
    let mut interval = time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                info!("reconciliation loop stopped");
                return;
            }
            _ = interval.tick() => {
                match store.reconcile(100).await {
                    Ok(repaired) if repaired > 0 => info!(repaired, "reconciled execution and workflow state"),
                    Ok(_) => {}
                    Err(error) => warn!(%error, "reconciliation pass failed"),
                }
            }
        }
    }
}
