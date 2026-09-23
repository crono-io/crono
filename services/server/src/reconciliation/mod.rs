//! Bounded repair loop for expired leases and abandoned claims.
//!
//! This is a safety net rather than the dispatch hot path. Each pass delegates
//! an indexed, bounded transaction to PostgreSQL and can run on every server.

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
                    Ok(repaired) if repaired > 0 => info!(repaired, "reconciled stale execution state"),
                    Ok(_) => {}
                    Err(error) => warn!(%error, "reconciliation pass failed"),
                }
            }
        }
    }
}
