//! Health check handlers for service monitoring.
//!
//! This module exposes four unauthenticated operational endpoints:
//! - `/live`: process liveness only, with no dependency checks
//! - `/ready`: PostgreSQL reachability, because durable work cannot be accepted without it
//! - `/health`: build identity plus PostgreSQL and NATS status as JSON
//! - `/metrics`: bounded-cardinality Prometheus metrics in `OpenMetrics` text
//!
//! Keeping liveness independent from dependencies prevents a transient PostgreSQL or NATS
//! outage from causing restart loops. Readiness checks only PostgreSQL: a NATS outage degrades
//! dispatch while execution intent stays durable in the outbox, so `/health` reports NATS as
//! `degraded` instead of withdrawing the instance from traffic. The detailed response and `X-App`
//! header expose only build metadata so operators and reverse proxies can identify a deployment
//! without receiving configuration or credentials.

use super::super::state::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Json},
};
use serde::Serialize;
use tracing::error;
use utoipa::ToSchema;

/// Build identity returned by the detailed health endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// Full source commit hash, or `unknown` for builds without Git metadata.
    pub commit: String,
    /// Cargo package name.
    pub name: String,
    /// Cargo package version.
    pub version: String,
    /// PostgreSQL must be available for readiness.
    pub database: &'static str,
    /// NATS outages degrade dispatch without losing execution intent.
    pub nats: &'static str,
}

#[utoipa::path(
    get,
    path = "/live",
    responses((status = 200, description = "Process is alive")),
    tag = "health"
)]
/// Report process liveness without checking external dependencies.
///
/// Orchestrators can use this probe for restart decisions without turning a
/// transient future PostgreSQL or NATS outage into a restart loop.
pub async fn live() -> StatusCode {
    StatusCode::OK
}

#[utoipa::path(
    get,
    path = "/ready",
    responses(
        (status = 200, description = "PostgreSQL is reachable, so the server can accept durable work."),
        (status = 503, description = "PostgreSQL is unreachable; the server cannot accept work."),
    ),
    tag = "health"
)]
/// Report whether PostgreSQL can accept durable control-plane work.
///
/// NATS is deliberately excluded: dispatch recovers from the outbox once the
/// broker returns, so a broker outage must not remove the instance from traffic.
pub async fn ready(State(state): State<AppState>) -> StatusCode {
    if state.ready().await {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (
            status = 200,
            description = "Service is healthy",
            body = Health,
            headers(
                ("X-App" = String, description = "Service name, version, and short commit")
            )
        )
    ),
    tag = "health"
)]
/// Return detailed process health and build identity.
///
/// The JSON body contains only public package metadata. `X-App` repeats a
/// compact identity for proxies and operational tooling without exposing
/// runtime configuration.
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let (database_ready, nats_ready) = tokio::join!(state.ready(), state.nats_ready());
    let health = Health {
        commit: commit_hash().to_string(),
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        database: if database_ready {
            "available"
        } else {
            "unavailable"
        },
        nats: if nats_ready { "available" } else { "degraded" },
    };
    let headers = x_app_headers(&health);

    (StatusCode::OK, headers, Json(health))
}

#[utoipa::path(
    get,
    path = "/metrics",
    responses(
        (status = 200, description = "Prometheus metrics in OpenMetrics text exposition format.", body = String, content_type = "application/openmetrics-text"),
        (status = 500, description = "Metrics could not be encoded.", body = String, content_type = "text/plain"),
    ),
    tag = "health"
)]
/// Expose bounded-cardinality operational metrics.
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state.refresh_metrics().await;
    match crate::metrics::global().encode() {
        Ok(output) => (
            StatusCode::OK,
            [(
                "content-type",
                "application/openmetrics-text; version=1.0.0; charset=utf-8",
            )],
            output,
        ),
        Err(error) => {
            error!(%error, "failed to encode Prometheus metrics");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "text/plain; charset=utf-8")],
                "metrics encoding failed".to_string(),
            )
        }
    }
}

/// Return the embedded source commit, falling back for source-archive builds.
fn commit_hash() -> &'static str {
    crate::built_info::GIT_COMMIT_HASH.unwrap_or("unknown")
}

/// Build the compact deployment identity header without failing the probe.
///
/// Package metadata should always form a valid header. If an unexpected value
/// does not, the handler logs the error and still returns its healthy response.
fn x_app_headers(health: &Health) -> HeaderMap {
    let short_commit: String = health.commit.chars().take(7).collect();
    let value = format!("{}:{}:{short_commit}", health.name, health.version);

    match HeaderValue::from_str(&value) {
        Ok(value) => {
            let mut headers = HeaderMap::new();
            headers.insert("x-app", value);
            headers
        }
        Err(error) => {
            error!(%error, "failed to build X-App health header");
            HeaderMap::new()
        }
    }
}
