//! Namespace, Queue, Job, Target, Run, and overview HTTP endpoints.
//!
//! Handlers translate JSON and paths into the authorization-enforcing
//! application facade. They never access PostgreSQL or caller-provided claims
//! directly. Public records use RFC 3339 timestamps and qualified names so the
//! independently deployed browser client does not depend on server internals.

use crate::{
    api::{error::ApiError, state::AppState},
    application::{
        CreateJobInput, CreateQueueInput, CreateScheduleInput, JobRecord, Page as ApplicationPage,
        RequestContext, RunRecord, ScheduleRecord, TargetRecord, TargetSetRecord, UpdateQueueInput,
        WorkerRecord,
    },
    domain::{
        CatchupPolicy as DomainCatchup, ExecutorKind as DomainExecutor,
        MisfirePolicy as DomainMisfire, Namespace, Queue, RunStatus as DomainRunStatus,
        ScheduleTiming,
    },
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use crono_api::{
    CatchupPolicy, CreateJobRequest, CreateNamespaceRequest, CreateQueueRequest, CreateRunRequest,
    CreateScheduleRequest, CreateTargetRequest, CreateTargetSetRequest, ExecutorKind, JobResource,
    MisfirePolicy, NamespaceResource, OverviewResource, Page, QueueResource, RunResource,
    RunStatus, ScheduleResource, TargetReference, TargetResource, TargetSetResource,
    UpdateQueueRequest, UpdateScheduleRequest, WorkerResource, WorkerStatus,
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
    path = "/api/namespaces",
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
    path = "/api/namespaces",
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
    path = "/api/namespaces/{namespace_id}",
    params(("namespace_id" = Uuid, Path, description = "Immutable Namespace ID")),
    responses((status = 200, body = NamespaceResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_namespace(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
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
    responses((status = 201, body = QueueResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Json(request): Json<CreateQueueRequest>,
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
    responses((status = 200, body = Page<QueueResource>)),
    tag = "control-plane"
)]
pub async fn list_queues(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<PageQuery>,
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
    responses((status = 200, body = QueueResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(queue_id): Path<Uuid>,
) -> Result<Json<QueueResource>, ApiError> {
    let queue = state.application().get_queue(&context, queue_id).await?;
    Ok(Json(queue_resource(&queue)?))
}

#[utoipa::path(
    put,
    path = "/api/queues/{queue_id}",
    params(("queue_id" = Uuid, Path, description = "Immutable Queue ID")),
    request_body = UpdateQueueRequest,
    responses((status = 200, body = QueueResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn update_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(queue_id): Path<Uuid>,
    Json(request): Json<UpdateQueueRequest>,
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
    responses((status = 204), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn delete_queue(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(queue_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state.application().delete_queue(&context, queue_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/jobs",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateJobRequest,
    responses((status = 201, body = JobResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Json(request): Json<CreateJobRequest>,
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
                },
                executable: request.executable,
                arguments: request.arguments,
                idempotent: request.idempotent,
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
    responses((status = 200, body = Page<JobResource>)),
    tag = "control-plane"
)]
pub async fn list_jobs(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Query(query): Query<PageQuery>,
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
    responses((status = 200, body = JobResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_job(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<JobResource>, ApiError> {
    let job = state.application().get_job(&context, job_id).await?;
    Ok(Json(job_resource(&job)?))
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/targets",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateTargetRequest,
    responses((status = 201, body = TargetResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Json(request): Json<CreateTargetRequest>,
) -> Result<(StatusCode, Json<TargetResource>), ApiError> {
    let target = state
        .application()
        .create_target(&context, namespace_id, &request.name, request.arguments)
        .await?;
    Ok((StatusCode::CREATED, Json(target_resource(&target)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}/targets",
    params(("namespace_id" = Uuid, Path), PageQuery),
    responses((status = 200, body = Page<TargetResource>)),
    tag = "control-plane"
)]
pub async fn list_targets(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Query(query): Query<PageQuery>,
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
    responses((status = 200, body = TargetResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_target(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(target_id): Path<Uuid>,
) -> Result<Json<TargetResource>, ApiError> {
    let target = state.application().get_target(&context, target_id).await?;
    Ok(Json(target_resource(&target)?))
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/target-sets",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateTargetSetRequest,
    responses((status = 201, body = TargetSetResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_target_set(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Json(request): Json<CreateTargetSetRequest>,
) -> Result<(StatusCode, Json<TargetSetResource>), ApiError> {
    let target_set = state
        .application()
        .create_target_set(&context, namespace_id, &request.name, request.target_ids)
        .await?;
    Ok((StatusCode::CREATED, Json(target_set_resource(&target_set)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace_id}/target-sets",
    params(("namespace_id" = Uuid, Path), PageQuery),
    responses((status = 200, body = Page<TargetSetResource>)),
    tag = "control-plane"
)]
pub async fn list_target_sets(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Query(query): Query<PageQuery>,
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
    responses((status = 200, body = TargetSetResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_target_set(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(target_set_id): Path<Uuid>,
) -> Result<Json<TargetSetResource>, ApiError> {
    let target_set = state
        .application()
        .get_target_set(&context, target_set_id)
        .await?;
    Ok(Json(target_set_resource(&target_set)?))
}

#[utoipa::path(
    post,
    path = "/api/namespaces/{namespace_id}/schedules",
    params(("namespace_id" = Uuid, Path)),
    request_body = CreateScheduleRequest,
    responses((status = 201, body = ScheduleResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Json(request): Json<CreateScheduleRequest>,
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
                target_id: crate::domain::TargetId::new(request.target_id),
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
    responses((status = 200, body = Page<ScheduleResource>)),
    tag = "control-plane"
)]
pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace_id): Path<Uuid>,
    Query(query): Query<PageQuery>,
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
    responses((status = 200, body = ScheduleResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(schedule_id): Path<Uuid>,
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
    responses((status = 200, body = ScheduleResource), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn update_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(schedule_id): Path<Uuid>,
    Json(request): Json<UpdateScheduleRequest>,
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
        .create_run(
            &context,
            request.request_id,
            request.job_id,
            request.target_id,
        )
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
    path = "/api/runs",
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
    path = "/api/runs/{run_id}",
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
    path = "/api/workers",
    params(PageQuery),
    responses((status = 200, body = Page<WorkerResource>)),
    tag = "control-plane"
)]
pub async fn list_workers(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<PageQuery>,
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
    path = "/api/overview",
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
        arguments: record.job.arguments().to_vec(),
        idempotent: record.job.idempotent(),
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
        created_at: timestamp(record.target_set.created_at())?,
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
        target_id: record.schedule.target_id.get(),
        target: format!("{}/{}", record.namespace, record.target_name),
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
        target_id: record.run.target_id().get(),
        target: format!("{}/{}", record.target_namespace, record.target_name),
        status,
        scheduled_at: timestamp(record.run.scheduled_at())?,
        created_at: timestamp(record.run.created_at())?,
        queued_at: record.run.queued_at().map(timestamp).transpose()?,
        started_at: record.run.started_at().map(timestamp).transpose()?,
        completed_at: record.run.completed_at().map(timestamp).transpose()?,
        attempt_count: record.run.attempt_count(),
        max_attempts: record.run.max_attempts(),
        lateness_seconds: record.run.lateness_seconds(),
        terminal_reason: record.run.terminal_reason().map(str::to_string),
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
    use super::worker_status;
    use crono_api::WorkerStatus;
    use time::{Duration, OffsetDateTime};

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
}
