//! Authorized, browser-safe operational snapshot.
//!
//! The operator capability is checked before any metric is sampled. PostgreSQL
//! owns shared scheduling and execution state; local poll timestamps and the
//! `JetStream` probe describe only the API instance answering this request.
//! Database sampling failures produce absent values rather than stale success
//! indicators, while detailed errors remain in server diagnostics.

use crate::{
    api::{error::ApiError, state::AppState},
    application::{ApplicationError, MonitorSnapshot, RequestContext},
};
use axum::{
    Json,
    extract::{Extension, State},
};
use crono_api::{
    DatabaseMonitorResource, InstanceMonitorResource, MonitorResource, PipelineMonitorResource,
};
use std::time::Duration;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[utoipa::path(
    get,
    path = "/api/monitor",
    responses(
        (status = 200, description = "Operator monitoring snapshot for this API instance.", body = MonitorResource),
        (status = 403, description = "The caller lacks the MonitorRead capability.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
/// Return an operator-authorized snapshot without disclosing execution data.
pub async fn monitor(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
) -> Result<Json<MonitorResource>, ApiError> {
    let snapshot = state.application().monitor_snapshot(&context).await?;
    let (database_available, nats_available) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(3), state.ready()),
        tokio::time::timeout(Duration::from_secs(3), state.nats_ready()),
    );
    let database_available = database_available.unwrap_or(false);
    let nats_available = nats_available.unwrap_or(false);
    let sampled_at = timestamp(OffsetDateTime::now_utc())?;
    let snapshot = if database_available { snapshot } else { None };
    let resource = monitor_resource(snapshot, database_available, nats_available, sampled_at)?;
    Ok(Json(resource))
}

/// Keep dependency health and absent database values independent in the wire response.
fn monitor_resource(
    snapshot: Option<MonitorSnapshot>,
    database_available: bool,
    nats_available: bool,
    sampled_at: String,
) -> Result<MonitorResource, ApiError> {
    let (database, pipeline) = snapshot.map_or(Ok((None, None)), |snapshot| {
        Ok::<_, ApiError>((
            Some(database_resource(snapshot)?),
            Some(pipeline_resource(snapshot)?),
        ))
    })?;
    Ok(MonitorResource {
        sampled_at,
        database_available,
        nats_available,
        database,
        pipeline,
        instance: InstanceMonitorResource {
            scheduler_last_poll_at: poll_timestamp(
                crate::metrics::global()
                    .scheduler_last_poll_unix_seconds
                    .get(),
            )?,
            publisher_last_poll_at: poll_timestamp(
                crate::metrics::global().outbox_last_poll_unix_seconds.get(),
            )?,
        },
    })
}

/// Convert non-negative database units without silently wrapping SQL values.
fn database_resource(snapshot: MonitorSnapshot) -> Result<DatabaseMonitorResource, ApiError> {
    Ok(DatabaseMonitorResource {
        size_bytes: nonnegative(snapshot.database_size_bytes)?,
        connections: nonnegative(snapshot.database_connections)?,
        pool_connections: snapshot.pool_connections,
        pool_idle_connections: snapshot.pool_idle_connections,
        pool_max_connections: snapshot.pool_max_connections,
    })
}

/// Project only aggregate scheduling and execution fields into the public DTO.
fn pipeline_resource(snapshot: MonitorSnapshot) -> Result<PipelineMonitorResource, ApiError> {
    Ok(PipelineMonitorResource {
        enabled_schedules: nonnegative(snapshot.enabled_schedules)?,
        due_schedules: nonnegative(snapshot.due_schedules)?,
        earliest_next_run_at: snapshot.earliest_next_run_at.map(timestamp).transpose()?,
        outbox_pending: nonnegative(snapshot.metrics.outbox_pending)?,
        outbox_oldest_seconds: nonnegative(snapshot.metrics.outbox_oldest_seconds)?,
        runs_queued: nonnegative(snapshot.metrics.execution_queued)?,
        runs_running: nonnegative(snapshot.metrics.execution_running)?,
        active_worker_leases: nonnegative(snapshot.metrics.worker_active)?,
        online_workers: nonnegative(snapshot.online_workers)?,
    })
}

/// Treat an impossible negative count as an internal failure.
fn nonnegative(value: i64) -> Result<u64, ApiError> {
    u64::try_from(value).map_err(|_| ApiError::from(ApplicationError::Internal))
}

/// Encode wall-clock sample times in the API's RFC 3339 convention.
fn timestamp(value: OffsetDateTime) -> Result<String, ApiError> {
    value
        .format(&Rfc3339)
        .map_err(|_| ApiError::from(ApplicationError::Internal))
}

/// Omit a local poll time until the loop has successfully touched PostgreSQL.
fn poll_timestamp(unix_seconds: i64) -> Result<Option<String>, ApiError> {
    if unix_seconds == 0 {
        return Ok(None);
    }
    let value = OffsetDateTime::from_unix_timestamp(unix_seconds)
        .map_err(|_| ApiError::from(ApplicationError::Internal))?;
    timestamp(value).map(Some)
}

#[cfg(test)]
mod tests {
    use super::{database_resource, monitor_resource, pipeline_resource, poll_timestamp};
    use crate::application::{MetricsSnapshot, MonitorSnapshot};

    #[test]
    fn snapshot_mapping_preserves_units_and_unavailable_poll() {
        let snapshot = MonitorSnapshot {
            database_size_bytes: 1_024,
            database_connections: 3,
            pool_connections: 2,
            pool_idle_connections: 1,
            pool_max_connections: 20,
            enabled_schedules: 4,
            due_schedules: 1,
            earliest_next_run_at: None,
            online_workers: 2,
            metrics: MetricsSnapshot {
                outbox_pending: 5,
                outbox_oldest_seconds: 12,
                execution_queued: 6,
                execution_running: 1,
                worker_active: 1,
            },
        };
        let database = database_resource(snapshot).ok();
        let pipeline = pipeline_resource(snapshot).ok();
        assert_eq!(database.as_ref().map(|value| value.size_bytes), Some(1_024));
        assert_eq!(pipeline.as_ref().map(|value| value.due_schedules), Some(1));
        assert_eq!(pipeline.as_ref().map(|value| value.outbox_pending), Some(5));
        assert!(poll_timestamp(0).is_ok_and(|value| value.is_none()));
    }

    #[test]
    fn unavailable_database_sample_keeps_transport_status_without_stale_values() {
        let resource = monitor_resource(None, false, true, "2026-09-25T00:00:00Z".to_string());
        assert!(resource.is_ok_and(|value| {
            !value.database_available
                && value.nats_available
                && value.database.is_none()
                && value.pipeline.is_none()
        }));
    }
}
