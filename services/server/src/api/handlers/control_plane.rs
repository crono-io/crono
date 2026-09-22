//! Namespace, Job, Target, Run, and overview HTTP endpoints.
//!
//! Handlers translate JSON and paths into the authorization-enforcing
//! application facade. They never access PostgreSQL or caller-provided claims
//! directly. Public records use RFC 3339 timestamps and qualified names so the
//! independently deployed browser client does not depend on server internals.

use crate::{
    api::{error::ApiError, state::AppState},
    application::{JobRecord, Page as ApplicationPage, RequestContext, RunRecord, TargetRecord},
    domain::{ExecutorKind as DomainExecutor, Namespace, RunStatus as DomainRunStatus},
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use crono_api::{
    CreateJobRequest, CreateNamespaceRequest, CreateRunRequest, CreateTargetRequest, ExecutorKind,
    JobResource, JobVersionResource, NamespaceResource, OverviewResource, Page, RunResource,
    RunStatus, TargetResource,
};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use utoipa::IntoParams;
use uuid::Uuid;

#[derive(Debug, Deserialize, IntoParams)]
pub struct PageQuery {
    /// Maximum resources to return, from 1 through 100.
    limit: Option<u16>,
    /// Opaque continuation cursor returned by a previous request.
    after: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct RunPageQuery {
    /// Maximum resources to return, from 1 through 100.
    limit: Option<u16>,
    /// UUID cursor returned by a previous Run request.
    before: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/v1/namespaces",
    request_body = CreateNamespaceRequest,
    responses((status = 201, body = NamespaceResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_namespace(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<CreateNamespaceRequest>,
) -> Result<(StatusCode, Json<NamespaceResource>), ApiError> {
    let namespace = state
        .application()
        .create_namespace(&context, &request.name)
        .await?;
    Ok((StatusCode::CREATED, Json(namespace_resource(&namespace)?)))
}

#[utoipa::path(
    get,
    path = "/api/v1/namespaces",
    params(PageQuery),
    responses((status = 200, body = Page<NamespaceResource>)),
    tag = "control-plane"
)]
pub async fn list_namespaces(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<NamespaceResource>>, ApiError> {
    let page = state
        .application()
        .list_namespaces(&context, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| namespace_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/v1/namespaces/{namespace}",
    params(("namespace" = String, Path, description = "Canonical Namespace name")),
    responses((status = 200, body = NamespaceResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_namespace(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
) -> Result<Json<NamespaceResource>, ApiError> {
    let namespace = state
        .application()
        .get_namespace(&context, &namespace)
        .await?;
    Ok(Json(namespace_resource(&namespace)?))
}

#[utoipa::path(
    post,
    path = "/api/v1/namespaces/{namespace}/jobs",
    params(("namespace" = String, Path)),
    request_body = CreateJobRequest,
    responses((status = 201, body = JobResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
    Json(request): Json<CreateJobRequest>,
) -> Result<(StatusCode, Json<JobResource>), ApiError> {
    let job = state
        .application()
        .create_job(
            &context,
            &namespace,
            &request.name,
            request.queue.as_deref(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(job_resource(&job)?)))
}

#[utoipa::path(
    get,
    path = "/api/v1/namespaces/{namespace}/jobs",
    params(("namespace" = String, Path), PageQuery),
    responses((status = 200, body = Page<JobResource>)),
    tag = "control-plane"
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<JobResource>>, ApiError> {
    let page = state
        .application()
        .list_jobs(&context, &namespace, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| job_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/v1/namespaces/{namespace}/jobs/{job}",
    params(("namespace" = String, Path), ("job" = String, Path)),
    responses((status = 200, body = JobResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path((namespace, job)): Path<(String, String)>,
) -> Result<Json<JobResource>, ApiError> {
    let job = state
        .application()
        .get_job(&context, &namespace, &job)
        .await?;
    Ok(Json(job_resource(&job)?))
}

#[utoipa::path(
    post,
    path = "/api/v1/namespaces/{namespace}/targets",
    params(("namespace" = String, Path)),
    request_body = CreateTargetRequest,
    responses((status = 201, body = TargetResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
    Json(request): Json<CreateTargetRequest>,
) -> Result<(StatusCode, Json<TargetResource>), ApiError> {
    let target = state
        .application()
        .create_target(&context, &namespace, &request.name)
        .await?;
    Ok((StatusCode::CREATED, Json(target_resource(&target)?)))
}

#[utoipa::path(
    get,
    path = "/api/v1/namespaces/{namespace}/targets",
    params(("namespace" = String, Path), PageQuery),
    responses((status = 200, body = Page<TargetResource>)),
    tag = "control-plane"
)]
pub async fn list_targets(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<TargetResource>>, ApiError> {
    let page = state
        .application()
        .list_targets(&context, &namespace, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| target_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/v1/namespaces/{namespace}/targets/{target}",
    params(("namespace" = String, Path), ("target" = String, Path)),
    responses((status = 200, body = TargetResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path((namespace, target)): Path<(String, String)>,
) -> Result<Json<TargetResource>, ApiError> {
    let target = state
        .application()
        .get_target(&context, &namespace, &target)
        .await?;
    Ok(Json(target_resource(&target)?))
}

#[utoipa::path(
    post,
    path = "/api/v1/runs",
    request_body = CreateRunRequest,
    responses((status = 201, body = RunResource), (status = 200, body = RunResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<CreateRunRequest>,
) -> Result<(StatusCode, Json<RunResource>), ApiError> {
    let outcome = state
        .application()
        .create_run(&context, request.request_id, &request.job, &request.target)
        .await?;
    let status = if outcome.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(run_resource(&outcome.run)?)))
}

#[utoipa::path(
    get,
    path = "/api/v1/runs",
    params(RunPageQuery),
    responses((status = 200, body = Page<RunResource>)),
    tag = "control-plane"
)]
pub async fn list_runs(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<RunPageQuery>,
) -> Result<Json<Page<RunResource>>, ApiError> {
    let page = state
        .application()
        .list_runs(&context, query.limit, query.before)
        .await?;
    Ok(Json(map_page(page, |item| run_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/v1/runs/{run_id}",
    params(("run_id" = Uuid, Path)),
    responses((status = 200, body = RunResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(run_id): Path<Uuid>,
) -> Result<Json<RunResource>, ApiError> {
    let run = state.application().get_run(&context, run_id).await?;
    Ok(Json(run_resource(&run)?))
}

#[utoipa::path(
    get,
    path = "/api/v1/overview",
    responses((status = 200, body = OverviewResource)),
    tag = "control-plane"
)]
pub async fn overview(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
) -> Result<Json<OverviewResource>, ApiError> {
    let overview = state.application().overview(&context).await?;
    Ok(Json(OverviewResource {
        namespaces: overview.namespaces,
        jobs: overview.jobs,
        targets: overview.targets,
        runs: overview.runs,
    }))
}

fn namespace_resource(namespace: &Namespace) -> Result<NamespaceResource, ApiError> {
    Ok(NamespaceResource {
        id: namespace.id().get(),
        name: namespace.name().to_string(),
        created_at: timestamp(namespace.created_at())?,
    })
}

fn job_resource(record: &JobRecord) -> Result<JobResource, ApiError> {
    let executor = match record.version.executor() {
        DomainExecutor::Noop => ExecutorKind::Noop,
    };
    Ok(JobResource {
        id: record.job.id().get(),
        namespace: record.namespace.to_string(),
        name: record.job.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.job.name()),
        version: JobVersionResource {
            id: record.version.id().get(),
            number: record.version.number(),
            executor,
            queue: record.version.queue().to_string(),
            created_at: timestamp(record.version.created_at())?,
        },
        created_at: timestamp(record.job.created_at())?,
    })
}

fn target_resource(record: &TargetRecord) -> Result<TargetResource, ApiError> {
    Ok(TargetResource {
        id: record.target.id().get(),
        namespace: record.namespace.to_string(),
        name: record.target.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.target.name()),
        created_at: timestamp(record.target.created_at())?,
    })
}

fn run_resource(record: &RunRecord) -> Result<RunResource, ApiError> {
    let status = match record.run.status() {
        DomainRunStatus::PendingDispatch => RunStatus::PendingDispatch,
        DomainRunStatus::Dispatched => RunStatus::Dispatched,
    };
    Ok(RunResource {
        id: record.run.id().get(),
        request_id: record.run.request_id(),
        job: format!("{}/{}", record.job_namespace, record.job_name),
        target: format!("{}/{}", record.target_namespace, record.target_name),
        job_version_id: record.run.job_version_id().get(),
        status,
        created_at: timestamp(record.run.created_at())?,
        dispatched_at: record.run.dispatched_at().map(timestamp).transpose()?,
    })
}

fn timestamp(value: time::OffsetDateTime) -> Result<String, ApiError> {
    value.format(&Rfc3339).map_err(|error| {
        tracing::error!(%error, "failed to encode a persisted timestamp");
        ApiError::from(crate::application::ApplicationError::Internal)
    })
}

fn map_page<T, U, F>(page: ApplicationPage<T>, mapper: F) -> Result<Page<U>, ApiError>
where
    F: Fn(T) -> Result<U, ApiError>,
{
    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(mapper)
            .collect::<Result<Vec<_>, _>>()?,
        next_cursor: page.next_cursor,
    })
}
