//! Health check handlers for service monitoring.
//!
//! This module exposes three unauthenticated probes:
//! - `/live`: process liveness only, with no dependency checks
//! - `/ready`: process readiness for orchestrators, currently with no external dependencies
//! - `/health`: detailed process status and build identity as JSON
//!
//! Keeping liveness independent from future PostgreSQL and NATS checks prevents transient
//! dependency failures from causing restart loops. Readiness is intentionally process-only until
//! those clients become part of server startup; their bounded checks will be added here when that
//! state exists. The detailed response and `X-App` header expose only build metadata so operators
//! and reverse proxies can identify a deployment without receiving configuration or credentials.

use axum::{
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
    responses((status = 200, description = "Service is ready to receive traffic")),
    tag = "health"
)]
/// Report readiness after successful process startup.
///
/// This is deliberately process-only while the server has no initialized
/// PostgreSQL or NATS clients. Dependency-aware `503` responses belong here
/// once those clients participate in startup.
pub async fn ready() -> StatusCode {
    StatusCode::OK
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
pub async fn health() -> impl IntoResponse {
    let health = Health {
        commit: commit_hash().to_string(),
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let headers = x_app_headers(&health);

    (StatusCode::OK, headers, Json(health))
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
