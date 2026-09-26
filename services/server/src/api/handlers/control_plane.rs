//! Namespace, Queue, Job, Target, Run, and overview HTTP endpoints.
//!
//! Handlers translate JSON and paths into the authorization-enforcing
//! application facade. They never access PostgreSQL or caller-provided claims
//! directly. Public records use RFC 3339 timestamps and qualified names so the
//! independently deployed browser client does not depend on server internals.

use crate::{
    api::{
        error::ApiError,
        extract::{ApiJson, ApiPath, ApiQuery},
        state::AppState,
    },
    application::{
        CreateJobInput, CreateQueueInput, CreateScheduleInput, JobRecord, Page as ApplicationPage,
        RequestContext, RunAttemptRecord, RunEventRecord, RunListFilter, RunRecord, ScheduleRecord,
        TargetRecord, TargetSetRecord, UpdateQueueInput, WorkerRecord,
    },
    domain::{
        CatchupPolicy as DomainCatchup, ExecutorKind as DomainExecutor, JobId,
        MisfirePolicy as DomainMisfire, Namespace, NamespaceId, Queue,
        RunStatus as DomainRunStatus, ScheduleTiming, TargetId, TargetSelection, TargetSetId,
    },
};
use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use crono_api::{
    AttemptStatus, CatchupPolicy, CreateJobRequest, CreateNamespaceRequest, CreateQueueRequest,
    CreateRunRequest, CreateScheduleRequest, CreateTargetRequest, CreateTargetSetRequest,
    ExecutionTarget, ExecutionTargetResource, ExecutorKind, JobResource, MisfirePolicy,
    NamespaceResource, OverviewResource, Page, QueueResource, RerunRequest, RunAttemptResource,
    RunBatchResource, RunEventResource, RunResource, RunStatus, RunTriggerSource, ScheduleResource,
    TargetReference, TargetResource, TargetSetResource, UpdateJobRequest, UpdateQueueRequest,
    UpdateScheduleRequest, UpdateTargetRequest, UpdateTargetSetRequest, WorkerDetailsResource,
    WorkerResource, WorkerStatus,
};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use utoipa::IntoParams;
use uuid::Uuid;

/// Name-cursor paging parameters.
///
/// Unknown parameters are rejected, matching request bodies, so a mistyped
/// parameter fails loudly instead of silently returning an unfiltered page.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// Maximum resources to return, from 1 through 100.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<u16>,
    /// Opaque continuation cursor returned by a previous request.
    after: Option<String>,
}

/// Run history paging and filters; unknown parameters are rejected like
/// [`PageQuery`], which matters most for mistyped filters.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub struct RunPageQuery {
    /// Maximum resources to return, from 1 through 100.
    #[param(minimum = 1, maximum = 100)]
    limit: Option<u16>,
    /// UUID cursor returned by a previous Run request.
    before: Option<Uuid>,
    namespace_id: Option<Uuid>,
    status: Option<RunStatus>,
    job_id: Option<Uuid>,
    target_id: Option<Uuid>,
    target_set_id: Option<Uuid>,
}

#[utoipa::path(
    post,
    path = "/api/namespaces",
    request_body = CreateNamespaceRequest,
    responses(
        (status = 201, description = "The Namespace was created.", body = NamespaceResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "A Namespace with this name already exists.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_namespace(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiJson(request): ApiJson<CreateNamespaceRequest>,
) -> Result<(StatusCode, Json<NamespaceResource>), ApiError> {
    let namespace = state
        .application()
        .create_namespace(&context, &request.name)
        .await?;
    Ok((StatusCode::CREATED, Json(namespace_resource(&namespace)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces",
    params(PageQuery),
    responses(
        (status = 200, description = "One page of Namespaces.", body = Page<NamespaceResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_namespaces(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<NamespaceResource>>, ApiError> {
    let page = state
        .application()
        .list_namespaces(&context, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| namespace_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}",
    params(("namespace_id" = Uuid, Path, description = "Immutable Namespace ID")),
    responses(
        (status = 200, description = "The requested Namespace.", body = NamespaceResource),
        (status = 404, description = "The Namespace was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_namespace(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
) -> Result<Json<NamespaceResource>, ApiError> {
    let namespace = state
        .application()
        .get_namespace(&context, namespace_id)
        .await?;
    Ok(Json(namespace_resource(&namespace)?))
}

#[utoipa::path(
    post,
    path = "/api/queues",
    request_body = CreateQueueRequest,
    responses(
        (status = 201, description = "The Queue was created.", body = QueueResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "A Queue with this name already exists.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiJson(request): ApiJson<CreateQueueRequest>,
) -> Result<(StatusCode, Json<QueueResource>), ApiError> {
    let queue = state
        .application()
        .create_queue(
            &context,
            CreateQueueInput {
                name: request.name,
                description: request.description,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(queue_resource(&queue)?)))
}

#[utoipa::path(
    get,
    path = "/api/queues",
    params(PageQuery),
    responses(
        (status = 200, description = "One page of Queues.", body = Page<QueueResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_queues(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<QueueResource>>, ApiError> {
    let page = state
        .application()
        .list_queues(&context, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| queue_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/queues/{queue_id}",
    params(("queue_id" = Uuid, Path, description = "Immutable Queue ID")),
    responses(
        (status = 200, description = "The requested Queue.", body = QueueResource),
        (status = 404, description = "The Queue was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(queue_id): ApiPath<Uuid>,
) -> Result<Json<QueueResource>, ApiError> {
    let queue = state.application().get_queue(&context, queue_id).await?;
    Ok(Json(queue_resource(&queue)?))
}

#[utoipa::path(
    put,
    path = "/api/queues/{queue_id}",
    params(("queue_id" = Uuid, Path, description = "Immutable Queue ID")),
    request_body = UpdateQueueRequest,
    responses(
        (status = 200, description = "The updated Queue.", body = QueueResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Queue was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The change conflicts with another Queue name or a newer revision.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn update_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(queue_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateQueueRequest>,
) -> Result<Json<QueueResource>, ApiError> {
    let queue = state
        .application()
        .update_queue(
            &context,
            queue_id,
            UpdateQueueInput {
                name: request.name,
                description: request.description,
                enabled: request.enabled,
            },
        )
        .await?;
    Ok(Json(queue_resource(&queue)?))
}

#[utoipa::path(
    delete,
    path = "/api/queues/{queue_id}",
    params(("queue_id" = Uuid, Path, description = "Immutable Queue ID")),
    responses(
        (status = 204, description = "The Queue was deleted."),
        (status = 400, description = "The system Queue cannot be deleted.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Queue was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The Queue is still referenced by Jobs, Runs, or worker presence records.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn delete_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(queue_id): ApiPath<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.application().delete_queue(&context, queue_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/jobs",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateJobRequest,
    responses(
        (status = 201, description = "The Job was created.", body = JobResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Namespace was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "A Job with this name already exists.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<CreateJobRequest>,
) -> Result<(StatusCode, Json<JobResource>), ApiError> {
    let job = state
        .application()
        .create_job(
            &context,
            namespace_id,
            CreateJobInput {
                name: request.name,
                queue_id: request.queue_id,
                executor: match request.executor {
                    ExecutorKind::Noop => DomainExecutor::Noop,
                    ExecutorKind::Process => DomainExecutor::Process,
                    ExecutorKind::Shell => DomainExecutor::Shell,
                },
                executable: request.executable,
                shell_command: request.shell_command,
                arguments: request.arguments,
                inputs: request.inputs,
                idempotent: request.idempotent,
                dry_run: request.dry_run,
                max_attempts: request.max_attempts,
                retry_initial_seconds: request.retry_initial_seconds,
                retry_max_seconds: request.retry_max_seconds,
                retry_multiplier: request.retry_multiplier,
                retry_jitter: request.retry_jitter,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(job_resource(&job)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}/jobs",
    params(("namespace_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, description = "One page of Jobs.", body = Page<JobResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<JobResource>>, ApiError> {
    let page = state
        .application()
        .list_jobs(&context, namespace_id, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| job_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/jobs/{job_id}",
    params(("job_id" = Uuid, Path)),
    responses(
        (status = 200, description = "The requested Job.", body = JobResource),
        (status = 404, description = "The Job was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(job_id): ApiPath<Uuid>,
) -> Result<Json<JobResource>, ApiError> {
    let job = state.application().get_job(&context, job_id).await?;
    Ok(Json(job_resource(&job)?))
}

#[utoipa::path(
    put,
    path = "/api/jobs/{job_id}",
    params(("job_id" = Uuid, Path)),
    request_body = UpdateJobRequest,
    responses(
        (status = 200, description = "The updated Job.", body = JobResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Job was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The change conflicts with another Job name or a newer revision.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn update_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(job_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateJobRequest>,
) -> Result<Json<JobResource>, ApiError> {
    let job = state
        .application()
        .update_job(
            &context,
            job_id,
            CreateJobInput {
                name: request.name,
                queue_id: request.queue_id,
                executor: domain_executor(request.executor),
                executable: request.executable,
                shell_command: request.shell_command,
                arguments: request.arguments,
                inputs: request.inputs,
                idempotent: request.idempotent,
                dry_run: request.dry_run,
                max_attempts: request.max_attempts,
                retry_initial_seconds: request.retry_initial_seconds,
                retry_max_seconds: request.retry_max_seconds,
                retry_multiplier: request.retry_multiplier,
                retry_jitter: request.retry_jitter,
            },
        )
        .await?;
    Ok(Json(job_resource(&job)?))
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/targets",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateTargetRequest,
    responses(
        (status = 201, description = "The Target was created.", body = TargetResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Namespace was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "A Target with this name already exists.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<CreateTargetRequest>,
) -> Result<(StatusCode, Json<TargetResource>), ApiError> {
    let target = state
        .application()
        .create_target(
            &context,
            namespace_id,
            &request.name,
            request.arguments,
            request.inputs,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(target_resource(&target)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}/targets",
    params(("namespace_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, description = "One page of Targets.", body = Page<TargetResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_targets(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<TargetResource>>, ApiError> {
    let page = state
        .application()
        .list_targets(&context, namespace_id, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| target_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/targets/{target_id}",
    params(("target_id" = Uuid, Path)),
    responses(
        (status = 200, description = "The requested Target.", body = TargetResource),
        (status = 404, description = "The Target was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(target_id): ApiPath<Uuid>,
) -> Result<Json<TargetResource>, ApiError> {
    let target = state.application().get_target(&context, target_id).await?;
    Ok(Json(target_resource(&target)?))
}

#[utoipa::path(
    put,
    path = "/api/targets/{target_id}",
    params(("target_id" = Uuid, Path)),
    request_body = UpdateTargetRequest,
    responses(
        (status = 200, description = "The updated Target.", body = TargetResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Target was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The change conflicts with another Target name or a newer revision.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn update_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(target_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateTargetRequest>,
) -> Result<Json<TargetResource>, ApiError> {
    let target = state
        .application()
        .update_target(
            &context,
            target_id,
            &request.name,
            request.arguments,
            request.inputs,
        )
        .await?;
    Ok(Json(target_resource(&target)?))
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/target-sets",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateTargetSetRequest,
    responses(
        (status = 201, description = "The Target Set was created.", body = TargetSetResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Namespace was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "A Target Set with this name already exists.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_target_set(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<CreateTargetSetRequest>,
) -> Result<(StatusCode, Json<TargetSetResource>), ApiError> {
    let target_set = state
        .application()
        .create_target_set(
            &context,
            namespace_id,
            &request.name,
            request.target_ids,
            request.inputs,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(target_set_resource(&target_set)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}/target-sets",
    params(("namespace_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, description = "One page of Target Sets.", body = Page<TargetSetResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_target_sets(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<TargetSetResource>>, ApiError> {
    let page = state
        .application()
        .list_target_sets(&context, namespace_id, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| target_set_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/target-sets/{target_set_id}",
    params(("target_set_id" = Uuid, Path)),
    responses(
        (status = 200, description = "The requested Target Set.", body = TargetSetResource),
        (status = 404, description = "The Target Set was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_target_set(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(target_set_id): ApiPath<Uuid>,
) -> Result<Json<TargetSetResource>, ApiError> {
    let target_set = state
        .application()
        .get_target_set(&context, target_set_id)
        .await?;
    Ok(Json(target_set_resource(&target_set)?))
}

#[utoipa::path(
    put,
    path = "/api/target-sets/{target_set_id}",
    params(("target_set_id" = Uuid, Path)),
    request_body = UpdateTargetSetRequest,
    responses(
        (status = 200, description = "The updated Target Set.", body = TargetSetResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Target Set was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The change conflicts with another Target Set name or a newer revision.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn update_target_set(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(target_set_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateTargetSetRequest>,
) -> Result<Json<TargetSetResource>, ApiError> {
    let target_set = state
        .application()
        .update_target_set(
            &context,
            target_set_id,
            &request.name,
            request.target_ids,
            request.inputs,
        )
        .await?;
    Ok(Json(target_set_resource(&target_set)?))
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/schedules",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateScheduleRequest,
    responses(
        (status = 201, description = "The Schedule was created.", body = ScheduleResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Namespace was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "A Schedule with this name already exists.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResource>), ApiError> {
    let timing = match (request.cron_expression, request.execute_at) {
        (Some(expression), None) => ScheduleTiming::Cron {
            expression,
            timezone: request.timezone,
        },
        (None, Some(execute_at)) => ScheduleTiming::Once {
            execute_at: time::OffsetDateTime::parse(&execute_at, &Rfc3339).map_err(|_| {
                ApiError::from(crate::application::ApplicationError::invalid(
                    "execute_at",
                    "execute_at must be an RFC 3339 timestamp",
                ))
            })?,
        },
        _ => {
            return Err(ApiError::from(
                crate::application::ApplicationError::invalid_request(
                    "exactly one of cron_expression or execute_at is required",
                ),
            ));
        }
    };
    let schedule = state
        .application()
        .create_schedule(
            &context,
            namespace_id,
            CreateScheduleInput {
                name: request.name,
                job_id: crate::domain::JobId::new(request.job_id),
                target: domain_target(request.target),
                inputs: request.inputs,
                timing,
                misfire_policy: domain_misfire(request.misfire_policy),
                misfire_grace_seconds: request.misfire_grace_seconds,
                catchup_policy: domain_catchup(request.catchup_policy),
                max_catchup_runs: request.max_catchup_runs,
                max_catchup_age_seconds: request.max_catchup_age_seconds,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(schedule_resource(&schedule)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}/schedules",
    params(("namespace_id" = Uuid, Path), PageQuery),
    responses(
        (status = 200, description = "One page of Schedules.", body = Page<ScheduleResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(namespace_id): ApiPath<Uuid>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<ScheduleResource>>, ApiError> {
    let page = state
        .application()
        .list_schedules(&context, namespace_id, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| schedule_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/schedules/{schedule_id}",
    params(("schedule_id" = Uuid, Path)),
    responses(
        (status = 200, description = "The requested Schedule.", body = ScheduleResource),
        (status = 404, description = "The Schedule was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(schedule_id): ApiPath<Uuid>,
) -> Result<Json<ScheduleResource>, ApiError> {
    let record = state
        .application()
        .get_schedule(&context, schedule_id)
        .await?;
    Ok(Json(schedule_resource(&record)?))
}

#[utoipa::path(
    patch,
    path = "/api/schedules/{schedule_id}",
    params(("schedule_id" = Uuid, Path)),
    request_body = UpdateScheduleRequest,
    responses(
        (status = 200, description = "The updated Schedule.", body = ScheduleResource),
        (status = 400, description = "The Schedule cannot be enabled because its recurrence is invalid.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Schedule was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The revision is stale; reload the Schedule and retry.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn update_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(schedule_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<UpdateScheduleRequest>,
) -> Result<Json<ScheduleResource>, ApiError> {
    let record = state
        .application()
        .set_schedule_enabled(&context, schedule_id, request.revision, request.enabled)
        .await?;
    Ok(Json(schedule_resource(&record)?))
}

#[utoipa::path(
    post,
    path = "/api/runs",
    request_body = CreateRunRequest,
    responses(
        (status = 200, description = "Runs already created for this request ID; the request was an idempotent replay.", body = RunBatchResource),
        (status = 201, description = "Runs created for a new request ID.", body = RunBatchResource),
        (status = 400, description = "The request is invalid; `field` names the offending input when known.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The Job, Target, or Target Set was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The request ID was already used with different inputs.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn create_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiJson(request): ApiJson<CreateRunRequest>,
) -> Result<(StatusCode, Json<RunBatchResource>), ApiError> {
    let outcome = state
        .application()
        .create_run(
            &context,
            request.request_id,
            request.job_id,
            domain_target(request.target),
            request.inputs,
        )
        .await?;
    let status = if outcome.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    let runs = outcome
        .runs
        .iter()
        .map(run_resource)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((
        status,
        Json(RunBatchResource {
            request_id: request.request_id,
            runs,
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/runs",
    params(RunPageQuery),
    responses(
        (status = 200, description = "One page of Runs.", body = Page<RunResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_runs(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiQuery(query): ApiQuery<RunPageQuery>,
) -> Result<Json<Page<RunResource>>, ApiError> {
    let page = state
        .application()
        .list_runs(
            &context,
            RunListFilter {
                namespace_id: query.namespace_id.map(NamespaceId::new),
                status: query.status.map(domain_run_status),
                job_id: query.job_id.map(JobId::new),
                target_id: query.target_id.map(TargetId::new),
                target_set_id: query.target_set_id.map(TargetSetId::new),
            },
            query.limit,
            query.before,
        )
        .await?;
    Ok(Json(map_page(page, |item| run_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/runs/{run_id}",
    params(("run_id" = Uuid, Path)),
    responses(
        (status = 200, description = "The requested Run.", body = RunResource),
        (status = 404, description = "The Run was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
pub async fn get_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(run_id): ApiPath<Uuid>,
) -> Result<Json<RunResource>, ApiError> {
    let run = state.application().get_run(&context, run_id).await?;
    Ok(Json(run_resource(&run)?))
}

#[utoipa::path(
    post,
    path = "/api/runs/{run_id}/rerun",
    params(("run_id" = Uuid, Path)),
    request_body = RerunRequest,
    responses(
        (status = 200, description = "The Run already created for this request ID; the request was an idempotent replay.", body = RunResource),
        (status = 201, description = "A new Run repeating the source Run was created.", body = RunResource),
        (status = 400, description = "The source Run cannot be repeated: its outcome is not a confirmed terminal state, it has no stored snapshot, or its Queue is disabled.", body = crono_api::ErrorEnvelope),
        (status = 404, description = "The source Run was not found.", body = crono_api::ErrorEnvelope),
        (status = 409, description = "The request ID was already used for a different Run.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
/// Repeat one terminal Run's stored execution snapshot after authorization checks.
pub async fn rerun_run(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(run_id): ApiPath<Uuid>,
    ApiJson(request): ApiJson<RerunRequest>,
) -> Result<(StatusCode, Json<RunResource>), ApiError> {
    let outcome = state
        .application()
        .rerun_run(&context, run_id, request.request_id)
        .await?;
    let run = outcome
        .runs
        .first()
        .ok_or(crate::application::ApplicationError::Internal)?;
    let status = if outcome.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(run_resource(run)?)))
}

#[utoipa::path(
    get,
    path = "/api/runs/{run_id}/events",
    params(("run_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Server-observed lifecycle events for the Run, oldest first.", body = Vec<RunEventResource>),
        (status = 404, description = "The Run was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
/// Return server-observed lifecycle timestamps after the same `RunRead` decision.
pub async fn list_run_events(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(run_id): ApiPath<Uuid>,
) -> Result<Json<Vec<RunEventResource>>, ApiError> {
    let events = state
        .application()
        .list_run_events(&context, run_id)
        .await?;
    Ok(Json(
        events
            .iter()
            .map(run_event_resource)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/runs/{run_id}/attempts",
    params(("run_id" = Uuid, Path)),
    responses(
        (status = 200, description = "Attempts for the Run with bounded output.", body = Vec<RunAttemptResource>),
        (status = 404, description = "The Run was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
/// Read bounded Attempt output only when the caller can read its Run.
pub async fn list_run_attempts(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(run_id): ApiPath<Uuid>,
) -> Result<Json<Vec<RunAttemptResource>>, ApiError> {
    let attempts = state
        .application()
        .list_run_attempts(&context, run_id)
        .await?;
    Ok(Json(
        attempts
            .iter()
            .map(run_attempt_resource)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}

#[utoipa::path(
    get,
    path = "/api/workers",
    params(PageQuery),
    responses(
        (status = 200, description = "One page of workers.", body = Page<WorkerResource>),
    ),
    tag = "control-plane"
)]
pub async fn list_workers(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> Result<Json<Page<WorkerResource>>, ApiError> {
    let page = state
        .application()
        .list_workers(&context, query.limit, query.after.as_deref())
        .await?;
    let now = time::OffsetDateTime::now_utc();
    Ok(Json(map_page(page, |worker| {
        worker_resource(&worker, now)
    })?))
}

#[utoipa::path(
    get,
    path = "/api/workers/{worker_id}",
    params(("worker_id" = String, Path)),
    responses(
        (status = 200, description = "The worker and its allowlisted diagnostics.", body = WorkerDetailsResource),
        (status = 404, description = "The worker was not found.", body = crono_api::ErrorEnvelope),
    ),
    tag = "control-plane"
)]
/// Read a single worker's safe heartbeat diagnostics under `WorkerRead`.
pub async fn get_worker(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    ApiPath(worker_id): ApiPath<String>,
) -> Result<Json<WorkerDetailsResource>, ApiError> {
    let record = state.application().get_worker(&context, &worker_id).await?;
    Ok(Json(WorkerDetailsResource {
        worker: worker_resource(&record, time::OffsetDateTime::now_utc())?,
        diagnostics: record.diagnostics,
    }))
}

#[utoipa::path(
    get,
    path = "/api/overview",
    responses(
        (status = 200, description = "Resource and Run counts visible to the caller.", body = OverviewResource),
    ),
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
        target_sets: overview.target_sets,
        schedules: overview.schedules,
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

fn queue_resource(queue: &Queue) -> Result<QueueResource, ApiError> {
    Ok(QueueResource {
        id: queue.id().get(),
        name: queue.name().to_string(),
        description: queue.description().map(str::to_string),
        enabled: queue.enabled(),
        system: queue.system(),
        created_at: timestamp(queue.created_at())?,
        updated_at: timestamp(queue.updated_at())?,
    })
}

fn job_resource(record: &JobRecord) -> Result<JobResource, ApiError> {
    let executor = match record.job.executor() {
        DomainExecutor::Noop => ExecutorKind::Noop,
        DomainExecutor::Process => ExecutorKind::Process,
        DomainExecutor::Shell => ExecutorKind::Shell,
    };
    Ok(JobResource {
        id: record.job.id().get(),
        namespace_id: record.job.namespace_id().get(),
        namespace: record.namespace.to_string(),
        name: record.job.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.job.name()),
        executor,
        queue_id: record.job.queue_id().get(),
        queue: record.queue_name.to_string(),
        executable: record.job.executable().map(str::to_string),
        shell_command: record.job.shell_command().map(str::to_string),
        arguments: record.job.arguments().to_vec(),
        inputs: record.job.inputs().clone(),
        idempotent: record.job.idempotent(),
        dry_run: record.job.dry_run(),
        max_attempts: record.job.max_attempts(),
        retry_initial_seconds: record.job.retry_initial_seconds(),
        retry_max_seconds: record.job.retry_max_seconds(),
        retry_multiplier: record.job.retry_multiplier(),
        retry_jitter: record.job.retry_jitter(),
        created_at: timestamp(record.job.created_at())?,
        updated_at: timestamp(record.job.updated_at())?,
    })
}

fn target_resource(record: &TargetRecord) -> Result<TargetResource, ApiError> {
    Ok(TargetResource {
        id: record.target.id().get(),
        namespace_id: record.target.namespace_id().get(),
        namespace: record.namespace.to_string(),
        name: record.target.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.target.name()),
        arguments: record.target.arguments().to_vec(),
        inputs: record.target.inputs().clone(),
        created_at: timestamp(record.target.created_at())?,
        updated_at: timestamp(record.target.updated_at())?,
    })
}

fn target_set_resource(record: &TargetSetRecord) -> Result<TargetSetResource, ApiError> {
    let targets = record
        .targets
        .iter()
        .map(|target| TargetReference {
            id: target.id().get(),
            name: target.name().to_string(),
        })
        .collect();
    Ok(TargetSetResource {
        id: record.target_set.id().get(),
        namespace_id: record.target_set.namespace_id().get(),
        namespace: record.namespace.to_string(),
        name: record.target_set.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.target_set.name()),
        targets,
        inputs: record.target_set.inputs().clone(),
        created_at: timestamp(record.target_set.created_at())?,
        updated_at: timestamp(record.target_set.updated_at())?,
    })
}

fn schedule_resource(record: &ScheduleRecord) -> Result<ScheduleResource, ApiError> {
    let (cron_expression, execute_at, timezone) = match &record.schedule.timing {
        ScheduleTiming::Cron {
            expression,
            timezone,
        } => (Some(expression.clone()), None, timezone.clone()),
        ScheduleTiming::Once { execute_at } => {
            (None, Some(timestamp(*execute_at)?), "UTC".to_string())
        }
    };
    Ok(ScheduleResource {
        id: record.schedule.id.get(),
        namespace_id: record.schedule.namespace_id.get(),
        namespace: record.namespace.to_string(),
        name: record.schedule.name.to_string(),
        job_id: record.schedule.job_id.get(),
        job: format!("{}/{}", record.namespace, record.job_name),
        target: match record.schedule.target {
            TargetSelection::Target(id) => ExecutionTargetResource::Target {
                id: id.get(),
                name: format!("{}/{}", record.namespace, record.target_name),
            },
            TargetSelection::TargetSet(id) => ExecutionTargetResource::TargetSet {
                id: id.get(),
                name: format!("{}/{}", record.namespace, record.target_name),
            },
        },
        inputs: record.schedule.inputs.clone(),
        cron_expression,
        execute_at,
        timezone,
        enabled: record.schedule.enabled,
        next_run_at: record.schedule.next_run_at.map(timestamp).transpose()?,
        last_run_at: record.schedule.last_run_at.map(timestamp).transpose()?,
        misfire_policy: api_misfire(record.schedule.misfire_policy),
        misfire_grace_seconds: record.schedule.misfire_grace_seconds,
        catchup_policy: api_catchup(record.schedule.catchup_policy),
        max_catchup_runs: record.schedule.max_catchup_runs,
        max_catchup_age_seconds: record.schedule.max_catchup_age_seconds,
        revision: record.schedule.revision,
        created_at: timestamp(record.schedule.created_at)?,
        updated_at: timestamp(record.schedule.updated_at)?,
    })
}

const fn domain_misfire(value: MisfirePolicy) -> DomainMisfire {
    match value {
        MisfirePolicy::RunLate => DomainMisfire::RunLate,
        MisfirePolicy::Skip => DomainMisfire::Skip,
        MisfirePolicy::GracePeriod => DomainMisfire::GracePeriod,
    }
}

const fn domain_executor(value: ExecutorKind) -> DomainExecutor {
    match value {
        ExecutorKind::Noop => DomainExecutor::Noop,
        ExecutorKind::Process => DomainExecutor::Process,
        ExecutorKind::Shell => DomainExecutor::Shell,
    }
}

const fn domain_target(value: ExecutionTarget) -> TargetSelection {
    match value {
        ExecutionTarget::Target { id } => TargetSelection::Target(crate::domain::TargetId::new(id)),
        ExecutionTarget::TargetSet { id } => {
            TargetSelection::TargetSet(crate::domain::TargetSetId::new(id))
        }
    }
}

const fn api_misfire(value: DomainMisfire) -> MisfirePolicy {
    match value {
        DomainMisfire::RunLate => MisfirePolicy::RunLate,
        DomainMisfire::Skip => MisfirePolicy::Skip,
        DomainMisfire::GracePeriod => MisfirePolicy::GracePeriod,
    }
}

const fn domain_catchup(value: CatchupPolicy) -> DomainCatchup {
    match value {
        CatchupPolicy::Skip => DomainCatchup::Skip,
        CatchupPolicy::RunOnce => DomainCatchup::RunOnce,
        CatchupPolicy::CatchUp => DomainCatchup::CatchUp,
    }
}

const fn api_catchup(value: DomainCatchup) -> CatchupPolicy {
    match value {
        DomainCatchup::Skip => CatchupPolicy::Skip,
        DomainCatchup::RunOnce => CatchupPolicy::RunOnce,
        DomainCatchup::CatchUp => CatchupPolicy::CatchUp,
    }
}

fn run_resource(record: &RunRecord) -> Result<RunResource, ApiError> {
    let status = match record.run.status() {
        DomainRunStatus::PendingDispatch => RunStatus::PendingDispatch,
        DomainRunStatus::Queued => RunStatus::Queued,
        DomainRunStatus::Running => RunStatus::Running,
        DomainRunStatus::RetryWait => RunStatus::RetryWait,
        DomainRunStatus::Succeeded => RunStatus::Succeeded,
        DomainRunStatus::Failed => RunStatus::Failed,
        DomainRunStatus::Dead => RunStatus::Dead,
        DomainRunStatus::Skipped => RunStatus::Skipped,
        DomainRunStatus::Cancelled => RunStatus::Cancelled,
        DomainRunStatus::Unknown => RunStatus::Unknown,
    };
    Ok(RunResource {
        id: record.run.id().get(),
        request_id: record.run.request_id(),
        schedule_id: record.run.schedule_id().map(crate::domain::ScheduleId::get),
        job_id: record.run.job_id().get(),
        job: format!("{}/{}", record.job_namespace, record.job_name),
        namespace: record.job_namespace.to_string(),
        job_name: record.job_name.to_string(),
        target_id: record.run.target_id().get(),
        target: format!("{}/{}", record.target_namespace, record.target_name),
        target_name: record.target_name.to_string(),
        target_set: record.target_set_name.as_ref().map(ToString::to_string),
        queue: record.queue_name.to_string(),
        trigger_source: if record.run.rerun_of_run_id().is_some() {
            RunTriggerSource::Rerun
        } else if record.run.schedule_id().is_some() {
            RunTriggerSource::Scheduler
        } else {
            RunTriggerSource::Api
        },
        trigger_actor: None,
        rerun_of_run_id: record.run.rerun_of_run_id().map(crate::domain::RunId::get),
        rerunnable: record.run.status().is_repeatable() && record.has_execution_snapshot,
        status,
        scheduled_at: timestamp(record.run.scheduled_at())?,
        created_at: timestamp(record.run.created_at())?,
        triggered_at: timestamp(record.run.created_at())?,
        queued_at: record.run.queued_at().map(timestamp).transpose()?,
        started_at: record.run.started_at().map(timestamp).transpose()?,
        completed_at: record.run.completed_at().map(timestamp).transpose()?,
        duration_ms: record
            .run
            .started_at()
            .zip(record.run.completed_at())
            .and_then(|(start, finish)| u64::try_from((finish - start).whole_milliseconds()).ok()),
        attempt_count: record.run.attempt_count(),
        max_attempts: record.run.max_attempts(),
        lateness_seconds: record.run.lateness_seconds(),
        terminal_reason: record.run.terminal_reason().map(str::to_string),
    })
}

fn run_event_resource(record: &RunEventRecord) -> Result<RunEventResource, ApiError> {
    Ok(RunEventResource {
        event_type: record.event_type.clone(),
        created_at: timestamp(record.created_at)?,
    })
}

const fn domain_run_status(status: RunStatus) -> DomainRunStatus {
    match status {
        RunStatus::PendingDispatch => DomainRunStatus::PendingDispatch,
        RunStatus::Queued => DomainRunStatus::Queued,
        RunStatus::Running => DomainRunStatus::Running,
        RunStatus::RetryWait => DomainRunStatus::RetryWait,
        RunStatus::Succeeded => DomainRunStatus::Succeeded,
        RunStatus::Failed => DomainRunStatus::Failed,
        RunStatus::Dead => DomainRunStatus::Dead,
        RunStatus::Skipped => DomainRunStatus::Skipped,
        RunStatus::Cancelled => DomainRunStatus::Cancelled,
        RunStatus::Unknown => DomainRunStatus::Unknown,
    }
}

fn run_attempt_resource(record: &RunAttemptRecord) -> Result<RunAttemptResource, ApiError> {
    let status = match record.status.as_str() {
        "pending_dispatch" => AttemptStatus::PendingDispatch,
        "queued" => AttemptStatus::Queued,
        "running" => AttemptStatus::Running,
        "succeeded" => AttemptStatus::Succeeded,
        "skipped" => AttemptStatus::Skipped,
        "failed" => AttemptStatus::Failed,
        "dead" => AttemptStatus::Dead,
        "unknown" => AttemptStatus::Unknown,
        value => {
            tracing::error!(status = value, "unsupported persisted Attempt status");
            return Err(crate::application::ApplicationError::Internal.into());
        }
    };
    Ok(RunAttemptResource {
        id: record.id,
        worker_id: record.worker_id.clone(),
        attempt: record.attempt,
        status,
        started_at: record.started_at.map(timestamp).transpose()?,
        completed_at: record.completed_at.map(timestamp).transpose()?,
        exit_code: record.exit_code,
        stdout_tail: record.stdout_tail.clone(),
        stderr_tail: record.stderr_tail.clone(),
        error: record.error.clone(),
    })
}

fn worker_resource(
    record: &WorkerRecord,
    now: time::OffsetDateTime,
) -> Result<WorkerResource, ApiError> {
    Ok(WorkerResource {
        worker_id: record.worker_id.clone(),
        queue_id: record.queue_id.get(),
        queue: record.queue.clone(),
        concurrency: record.concurrency,
        version: record.version.clone(),
        status: worker_status(record.last_seen_at, now),
        started_at: timestamp(record.started_at)?,
        last_seen_at: timestamp(record.last_seen_at)?,
        active_executions: record.active_executions,
    })
}

/// Classify presence using server time so clients cannot claim liveness.
fn worker_status(last_seen: time::OffsetDateTime, now: time::OffsetDateTime) -> WorkerStatus {
    let age_seconds = (now - last_seen).whole_seconds().max(0);
    if age_seconds <= 30 {
        WorkerStatus::Online
    } else if age_seconds <= 120 {
        WorkerStatus::Stale
    } else {
        WorkerStatus::Offline
    }
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

#[cfg(test)]
mod tests {
    use super::{run_resource, worker_status};
    use crate::{
        application::RunRecord,
        domain::{
            JobId, NamespaceName, QueueId, QueueName, ResourceName, Run, RunData, RunId, RunStatus,
            TargetId,
        },
    };
    use crono_api::{RunTriggerSource, WorkerStatus};
    use time::{Duration, OffsetDateTime};
    use uuid::Uuid;

    #[test]
    fn worker_status_uses_server_owned_heartbeat_windows() {
        let now = OffsetDateTime::UNIX_EPOCH + Duration::minutes(5);

        assert_eq!(worker_status(now, now), WorkerStatus::Online);
        assert_eq!(
            worker_status(now - Duration::seconds(30), now),
            WorkerStatus::Online
        );
        assert_eq!(
            worker_status(now - Duration::seconds(31), now),
            WorkerStatus::Stale
        );
        assert_eq!(
            worker_status(now - Duration::minutes(2), now),
            WorkerStatus::Stale
        );
        assert_eq!(
            worker_status(now - Duration::seconds(121), now),
            WorkerStatus::Offline
        );
    }

    #[test]
    fn run_resource_reports_server_trigger_and_nonnegative_duration() -> anyhow::Result<()> {
        let start = OffsetDateTime::UNIX_EPOCH + Duration::minutes(1);
        let id = RunId::new(Uuid::now_v7());
        let record = RunRecord {
            has_execution_snapshot: true,
            run: Run::new(RunData {
                id,
                request_id: Some(Uuid::now_v7()),
                rerun_of_run_id: None,
                schedule_id: None,
                job_id: JobId::new(Uuid::now_v7()),
                target_id: TargetId::new(Uuid::now_v7()),
                queue_id: QueueId::new(Uuid::now_v7()),
                status: RunStatus::Succeeded,
                scheduled_at: start,
                created_at: start,
                queued_at: Some(start),
                started_at: Some(start),
                completed_at: Some(start + Duration::milliseconds(8400)),
                attempt_count: 1,
                max_attempts: 2,
                lateness_seconds: 0,
                terminal_reason: None,
            }),
            job_namespace: NamespaceName::parse("production")?,
            job_name: ResourceName::parse("backup")?,
            target_namespace: NamespaceName::parse("production")?,
            target_name: ResourceName::parse("db")?,
            target_set_name: None,
            queue_name: QueueName::parse("default")?,
        };
        let resource = run_resource(&record).map_err(|_| anyhow::anyhow!("Run mapping failed"))?;
        assert_eq!(resource.trigger_source, RunTriggerSource::Api);
        assert_eq!(resource.trigger_actor, None);
        assert_eq!(resource.duration_ms, Some(8400));
        assert_eq!(resource.triggered_at, resource.created_at);
        assert_eq!(resource.job_name, "backup");
        Ok(())
    }
}
