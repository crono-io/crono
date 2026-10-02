//! Workflow HTTP contracts expose immutable graphs and ordinary child Run links.
//!
//! Definitions refer to Jobs by UUID and edges by canonical names local to the
//! graph. Invocation responses include the launch snapshot, so edits cannot
//! change a consumer's reconstruction. Outputs and Attempts stay on Run routes.

use crate::ExecutionTarget;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Typed AND dependency; Failure includes failed/dead/unknown Run outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DependencyCondition {
    Success,
    Failure,
    Always,
}

/// Reference to one existing Job; node names must be unique in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowNodeRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    pub job_id: Uuid,
}

/// Canonical node-name endpoints; duplicate pairs and self edges are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowEdgeRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub from: String,
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub to: String,
    pub condition: DependencyCondition,
}

/// Bounded DAG definition: 1–64 nodes, at most 256 edges, same-Namespace Jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateWorkflowRequest {
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    #[cfg_attr(feature = "openapi", schema(max_length = 500))]
    pub description: Option<String>,
    #[cfg_attr(feature = "openapi", schema(min_items = 1, max_items = 64))]
    pub nodes: Vec<WorkflowNodeRequest>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(max_items = 256))]
    pub edges: Vec<WorkflowEdgeRequest>,
}

/// Replace a definition only if its current revision still matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UpdateWorkflowRequest {
    #[cfg_attr(feature = "openapi", schema(minimum = 1))]
    pub revision: u64,
    #[cfg_attr(
        feature = "openapi",
        schema(pattern = "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", max_length = 63)
    )]
    pub name: String,
    #[cfg_attr(feature = "openapi", schema(max_length = 500))]
    pub description: Option<String>,
    #[cfg_attr(feature = "openapi", schema(min_items = 1, max_items = 64))]
    pub nodes: Vec<WorkflowNodeRequest>,
    #[serde(default)]
    #[cfg_attr(feature = "openapi", schema(max_items = 256))]
    pub edges: Vec<WorkflowEdgeRequest>,
}

/// Idempotent invocation; ordinary input precedence and Target Set fan-out apply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StartWorkflowRequest {
    pub request_id: Uuid,
    pub target: ExecutionTarget,
    #[serde(default = "crate::default_inputs")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
}

/// Identity of a node in this particular definition revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowNodeResource {
    pub id: Uuid,
    pub name: String,
    pub job_id: Uuid,
}

/// Editable catalog graph, or the immutable copy embedded in an invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowResource {
    pub id: Uuid,
    pub namespace_id: Uuid,
    pub namespace: String,
    pub name: String,
    pub description: Option<String>,
    pub revision: u64,
    pub nodes: Vec<WorkflowNodeResource>,
    pub edges: Vec<WorkflowEdgeRequest>,
    pub created_at: String,
    pub updated_at: String,
}

/// Workflow result is derived after all nodes terminate; failures remain failures after rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum WorkflowRunState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Aggregate node state, including ambiguity and dependency skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum WorkflowNodeRunState {
    Pending,
    Ready,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    Unknown,
}

/// Follow this ordinary Run for its Attempts, bounded output, and worker identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowChildRunResource {
    pub target_id: Uuid,
    pub run_id: Uuid,
}

/// One logical node invocation; a Target Set produces several ordinary child Runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowNodeRunResource {
    pub id: Uuid,
    pub workflow_node_id: Uuid,
    pub name: String,
    pub job_id: Uuid,
    pub state: WorkflowNodeRunState,
    pub runs: Vec<WorkflowChildRunResource>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// Immutable launch context and current DAG execution, without duplicated Attempt output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WorkflowRunResource {
    pub id: Uuid,
    pub request_id: Uuid,
    pub workflow: WorkflowResource,
    pub target: ExecutionTarget,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub inputs: serde_json::Value,
    pub state: WorkflowRunState,
    pub cancellation_requested: bool,
    pub nodes: Vec<WorkflowNodeRunResource>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}
