//! HTTP translation for Workflow definitions and immutable invocation history.
//!
//! Every route enters the existing authorization-enforcing Application facade.
//! No handler evaluates dependencies, queries PostgreSQL, parses credentials, or
//! dispatches workers. Graphs and ordinary child Run links expose execution
//! progress while detailed output remains protected by normal `RunRead` checks.

use super::control_plane::{PageQuery, domain_target, map_page, timestamp};
use crate::{
    api::{
        error::ApiError,
        extract::{ApiJson, ApiPath, ApiQuery},
        state::AppState,
    },
    application::{RequestContext, WorkflowInput, WorkflowRecord, WorkflowRunRecord},
    domain::{DependencyCondition as DomainCondition, JobId, WorkflowNodeState, WorkflowState},
};
use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use crono_api::{
    CreateWorkflowRequest, DependencyCondition, ExecutionTarget, Page, StartWorkflowRequest,
    UpdateWorkflowRequest, WorkflowChildRunResource, WorkflowEdgeRequest, WorkflowNodeResource,
    WorkflowNodeRunResource, WorkflowNodeRunState, WorkflowResource, WorkflowRunResource,
    WorkflowRunState,
};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

/// Newest-first invocation pagination, matching ordinary Run history.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct WorkflowRunQuery {
    #[param(minimum = 1, maximum = 100)]
    limit: Option<u16>,
    before: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/namespaces/{namespace_id}/workflows",
    params(("namespace_id" = Uuid, Path)), request_body = CreateWorkflowRequest,
    responses((status = 201, description = "Validated DAG created.", body = WorkflowResource),
    (status = 404, description = "Namespace or Job not found.", body = crono_api::ErrorEnvelope),
    (status = 409, description = "Workflow name already exists.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn create_workflow(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<CreateWorkflowRequest>,
) -> Result<(StatusCode, Json<WorkflowResource>), ApiError> {
    let record = state
        .application()
        .create_workflow(&context, namespace_id, definition_input(request))
        .await?;
    Ok((StatusCode::CREATED, Json(workflow_resource(&record)?)))
}

#[utoipa::path(get, path = "/api/namespaces/{namespace_id}/workflows", params(("namespace_id" = Uuid, Path), PageQuery),
    responses((status = 200, description = "Visible Workflow definitions.", body = Page<WorkflowResource>),
    (status = 404, description = "Namespace not found.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn list_workflows(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<WorkflowResource>>, ApiError> {
    let page = state
        .application()
        .list_workflows(&context, namespace_id, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |record| workflow_resource(&record))?))
}

#[utoipa::path(get, path = "/api/workflows/{workflow_id}", params(("workflow_id" = Uuid, Path)),
    responses((status = 200, description = "Workflow definition.", body = WorkflowResource),
    (status = 404, description = "Workflow not found or hidden.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn get_workflow(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<WorkflowResource>, ApiError> {
    Ok(Json(workflow_resource(
        &state.application().get_workflow(&context, id).await?,
    )?))
}

#[utoipa::path(put, path = "/api/workflows/{workflow_id}", params(("workflow_id" = Uuid, Path)), request_body = UpdateWorkflowRequest,
    responses((status = 200, description = "Replaced graph; existing invocations are unchanged.", body = WorkflowResource),
    (status = 404, description = "Workflow or Job not found.", body = crono_api::ErrorEnvelope),
    (status = 409, description = "Name or revision conflicts.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn update_workflow(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateWorkflowRequest>,
) -> Result<Json<WorkflowResource>, ApiError> {
    let input = definition_input(CreateWorkflowRequest {
        name: request.name,
        description: request.description,
        nodes: request.nodes,
        edges: request.edges,
    });
    Ok(Json(workflow_resource(
        &state
            .application()
            .update_workflow(&context, id, request.revision, input)
            .await?,
    )?))
}

#[utoipa::path(delete, path = "/api/workflows/{workflow_id}", params(("workflow_id" = Uuid, Path)),
    responses((status = 204, description = "Unused Workflow deleted."),
    (status = 404, description = "Workflow not found.", body = crono_api::ErrorEnvelope),
    (status = 409, description = "Invocation history prevents deletion.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn delete_workflow(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.application().delete_workflow(&context, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/workflows/{workflow_id}/runs", params(("workflow_id" = Uuid, Path)), request_body = StartWorkflowRequest,
    responses((status = 201, description = "Invocation snapshotted and root Runs created atomically.", body = WorkflowRunResource),
    (status = 200, description = "Idempotent replay.", body = WorkflowRunResource),
    (status = 404, description = "Workflow, Job, or execution target not found.", body = crono_api::ErrorEnvelope),
    (status = 409, description = "Request identity or authorized revision conflicts.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn start_workflow(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<StartWorkflowRequest>,
) -> Result<(StatusCode, Json<WorkflowRunResource>), ApiError> {
    let (record, created) = state
        .application()
        .start_workflow(
            &context,
            id,
            request.request_id,
            domain_target(request.target),
            request.inputs,
        )
        .await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(workflow_run_resource(&record)?),
    ))
}

#[utoipa::path(get, path = "/api/workflows/{workflow_id}/runs", params(("workflow_id" = Uuid, Path), WorkflowRunQuery),
    responses((status = 200, description = "Visible invocation history with immutable DAG snapshots.", body = Page<WorkflowRunResource>),
    (status = 404, description = "Workflow not found or hidden.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn list_workflow_runs(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<WorkflowRunQuery>,
) -> Result<Json<Page<WorkflowRunResource>>, ApiError> {
    let page = state
        .application()
        .list_workflow_runs(&context, id, query.limit, query.before)
        .await?;
    Ok(Json(map_page(page, |record| {
        workflow_run_resource(&record)
    })?))
}

#[utoipa::path(get, path = "/api/workflow-runs/{workflow_run_id}", params(("workflow_run_id" = Uuid, Path)),
    responses((status = 200, description = "Immutable graph and current node/child Run states.", body = WorkflowRunResource),
    (status = 404, description = "Invocation not found or hidden.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn get_workflow_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<WorkflowRunResource>, ApiError> {
    Ok(Json(workflow_run_resource(
        &state.application().get_workflow_run(&context, id).await?,
    )?))
}

#[utoipa::path(post, path = "/api/workflow-runs/{workflow_run_id}/cancel", params(("workflow_run_id" = Uuid, Path)),
    responses((status = 200, description = "Pending nodes stopped; active ordinary Runs drain.", body = WorkflowRunResource),
    (status = 404, description = "Invocation not found.", body = crono_api::ErrorEnvelope)), tag = "workflows")]
pub async fn cancel_workflow_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(id): ApiPath<Uuid>,
) -> Result<Json<WorkflowRunResource>, ApiError> {
    Ok(Json(workflow_run_resource(
        &state
            .application()
            .cancel_workflow_run(&context, id)
            .await?,
    )?))
}

/// Translate untrusted graph values without interpreting policy or execution states.
fn definition_input(request: CreateWorkflowRequest) -> WorkflowInput {
    WorkflowInput {
        name: request.name,
        description: request.description,
        nodes: request
            .nodes
            .into_iter()
            .map(|node| (node.name, JobId::new(node.job_id)))
            .collect(),
        edges: request
            .edges
            .into_iter()
            .map(|edge| (edge.from, edge.to, domain_condition(edge.condition)))
            .collect(),
    }
}
const fn domain_condition(value: DependencyCondition) -> DomainCondition {
    match value {
        DependencyCondition::Success => DomainCondition::Success,
        DependencyCondition::Failure => DomainCondition::Failure,
        DependencyCondition::Always => DomainCondition::Always,
    }
}
const fn api_condition(value: DomainCondition) -> DependencyCondition {
    match value {
        DomainCondition::Success => DependencyCondition::Success,
        DomainCondition::Failure => DependencyCondition::Failure,
        DomainCondition::Always => DependencyCondition::Always,
    }
}

/// Expose only an authorized catalog graph or already-authorized invocation snapshot.
fn workflow_resource(record: &WorkflowRecord) -> Result<WorkflowResource, ApiError> {
    let nodes = record
        .definition
        .nodes
        .iter()
        .map(|node| {
            Ok(WorkflowNodeResource {
                id: record
                    .node_ids
                    .get(node.name.as_str())
                    .ok_or(crate::application::ApplicationError::Internal)?
                    .get(),
                name: node.name.as_str().to_owned(),
                job_id: node.job_id.get(),
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(WorkflowResource {
        id: record.id.get(),
        namespace_id: record.namespace_id.get(),
        namespace: record.namespace.as_str().to_owned(),
        name: record.definition.name.as_str().to_owned(),
        description: record.definition.description.clone(),
        revision: record.revision,
        nodes,
        edges: record
            .definition
            .edges
            .iter()
            .map(|edge| WorkflowEdgeRequest {
                from: edge.from.as_str().to_owned(),
                to: edge.to.as_str().to_owned(),
                condition: api_condition(edge.condition),
            })
            .collect(),
        created_at: timestamp(record.created_at)?,
        updated_at: timestamp(record.updated_at)?,
    })
}

/// Link to normal Runs; never duplicate stdout/stderr or bypass their independent read policy.
fn workflow_run_resource(record: &WorkflowRunRecord) -> Result<WorkflowRunResource, ApiError> {
    let nodes = record
        .nodes
        .iter()
        .map(|node| {
            Ok(WorkflowNodeRunResource {
                id: node.id.get(),
                workflow_node_id: node.workflow_node_id.get(),
                name: node.name.as_str().to_owned(),
                job_id: node.job_id.get(),
                state: node_run_state(node.state),
                runs: node
                    .runs
                    .iter()
                    .map(|child| WorkflowChildRunResource {
                        target_id: child.target_id.get(),
                        run_id: child.run_id.get(),
                    })
                    .collect(),
                started_at: node.started_at.map(timestamp).transpose()?,
                finished_at: node.finished_at.map(timestamp).transpose()?,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    let target = match record.target {
        crate::domain::TargetSelection::Target(id) => ExecutionTarget::Target { id: id.get() },
        crate::domain::TargetSelection::TargetSet(id) => {
            ExecutionTarget::TargetSet { id: id.get() }
        }
    };
    Ok(WorkflowRunResource {
        id: record.id.get(),
        request_id: record.request_id,
        workflow: workflow_resource(&record.workflow)?,
        target,
        inputs: record.inputs.clone(),
        state: run_state(record.state),
        cancellation_requested: record.cancellation_requested,
        nodes,
        created_at: timestamp(record.created_at)?,
        started_at: record.started_at.map(timestamp).transpose()?,
        finished_at: record.finished_at.map(timestamp).transpose()?,
    })
}
const fn run_state(value: WorkflowState) -> WorkflowRunState {
    match value {
        WorkflowState::Pending => WorkflowRunState::Pending,
        WorkflowState::Running => WorkflowRunState::Running,
        WorkflowState::Succeeded => WorkflowRunState::Succeeded,
        WorkflowState::Failed => WorkflowRunState::Failed,
        WorkflowState::Cancelled => WorkflowRunState::Cancelled,
    }
}
const fn node_run_state(value: WorkflowNodeState) -> WorkflowNodeRunState {
    match value {
        WorkflowNodeState::Pending => WorkflowNodeRunState::Pending,
        WorkflowNodeState::Ready => WorkflowNodeRunState::Ready,
        WorkflowNodeState::Running => WorkflowNodeRunState::Running,
        WorkflowNodeState::Succeeded => WorkflowNodeRunState::Succeeded,
        WorkflowNodeState::Failed => WorkflowNodeRunState::Failed,
        WorkflowNodeState::Skipped => WorkflowNodeRunState::Skipped,
        WorkflowNodeState::Cancelled => WorkflowNodeRunState::Cancelled,
        WorkflowNodeState::Unknown => WorkflowNodeRunState::Unknown,
    }
}
