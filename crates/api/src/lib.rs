//! Public transport contracts shared by Crono API clients and the server.
//!
//! These types describe JSON payloads only. They intentionally contain no
//! domain behavior, persistence details, authentication claims, or server
//! dependencies, allowing independently deployed clients to share the wire
//! contract without crossing the control-plane boundary.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One bounded page of public API resources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Page<T> {
    /// Resources in stable server-defined order.
    pub items: Vec<T>,
    /// Opaque cursor for the next page, or `None` at the end.
    pub next_cursor: Option<String>,
}

/// Request to create a Namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateNamespaceRequest {
    /// Canonical Namespace name.
    pub name: String,
}

/// Public Namespace representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NamespaceResource {
    pub id: Uuid,
    pub name: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Request to create a Job and its immutable first version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateJobRequest {
    pub name: String,
    /// Dispatch queue; omitted values use `default`.
    pub queue: Option<String>,
}

/// Supported immutable executor definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ExecutorKind {
    /// Development definition that is dispatchable but performs no work.
    Noop,
}

/// Immutable Job definition selected by a Run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct JobVersionResource {
    pub id: Uuid,
    pub number: u32,
    pub executor: ExecutorKind,
    pub queue: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Public Job identity and current immutable version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct JobResource {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    pub version: JobVersionResource,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Request to create an immutable identity-only Target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateTargetRequest {
    pub name: String,
}

/// Public Target representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TargetResource {
    pub id: Uuid,
    pub namespace: String,
    pub name: String,
    pub qualified_name: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

/// Request to create an idempotent Run from canonical qualified names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateRunRequest {
    pub request_id: Uuid,
    pub job: String,
    pub target: String,
}

/// Persisted Run dispatch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum RunStatus {
    PendingDispatch,
    Dispatched,
}

/// Public Run representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RunResource {
    pub id: Uuid,
    pub request_id: Uuid,
    pub job: String,
    pub target: String,
    pub job_version_id: Uuid,
    pub status: RunStatus,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 dispatch timestamp.
    pub dispatched_at: Option<String>,
}

/// Exact counts visible to the current request principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OverviewResource {
    pub namespaces: u64,
    pub jobs: u64,
    pub targets: u64,
    pub runs: u64,
}

/// Stable JSON error envelope returned by every application endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

/// Machine-readable code and safe human-facing error detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

/// Versioned durable message published for one committed Run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DispatchEnvelope {
    pub schema_version: u16,
    pub dispatch_id: Uuid,
    pub run_id: Uuid,
    pub job_version_id: Uuid,
    pub target_id: Uuid,
    pub queue: String,
}
