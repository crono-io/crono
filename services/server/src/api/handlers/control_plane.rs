//! Namespace, Job, Target, Run, and overview HTTP endpoints.
//!
//! Handlers translate JSON and paths into the authorization-enforcing
//! application facade. They never access PostgreSQL or caller-provided claims
//! directly. Public records use RFC 3339 timestamps and qualified names so the
//! independently deployed browser client does not depend on server internals.

use crate::{
    api::{error::ApiError, state::AppState},
    application::{
        CreateJobInput, CreateScheduleInput, JobRecord, Page as ApplicationPage, RequestContext,
        RunRecord, ScheduleRecord, TargetRecord, WorkerRecord,
    },
    domain::{
        CatchupPolicy as DomainCatchup, ExecutorKind as DomainExecutor,
        MisfirePolicy as DomainMisfire, Namespace, RunStatus as DomainRunStatus, ScheduleTiming,
    },
};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
};
use crono_api::{
    CatchupPolicy, CreateJobRequest, CreateNamespaceRequest, CreateRunRequest,
    CreateScheduleRequest, CreateTargetRequest, ExecutorKind, JobResource, MisfirePolicy,
    NamespaceResource, OverviewResource, Page, RunResource, RunStatus, ScheduleResource,
    TargetResource, UpdateScheduleRequest, WorkerResource, WorkerStatus,
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
    path = "/api/namespaces/{namespace}",
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
    path = "/api/namespaces/{namespace}/jobs",
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
            CreateJobInput {
                name: request.name,
                queue: request.queue,
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
    path = "/api/namespaces/{namespace}/jobs",
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
    path = "/api/namespaces/{namespace}/jobs/{job}",
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
    path = "/api/namespaces/{namespace}/targets",
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
        .create_target(&context, &namespace, &request.name, request.arguments)
        .await?;
    Ok((StatusCode::CREATED, Json(target_resource(&target)?)))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace}/targets",
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
    path = "/api/namespaces/{namespace}/targets/{target}",
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
    path = "/api/namespaces/{namespace}/schedules",
    params(("namespace" = String, Path)),
    request_body = CreateScheduleRequest,
    responses((status = 201, body = ScheduleResource), (status = 400, body = crono_api::ErrorEnvelope), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn create_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
    Json(request): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResource>), ApiError> {
    let timing = match (request.cron_expression, request.execute_at) {
        (Some(expression), None) => ScheduleTiming::Cron {
            expression,
            timezone: request.timezone,
        },
        (None, Some(execute_at)) => ScheduleTiming::Once {
            execute_at: time::OffsetDateTime::parse(&execute_at, &Rfc3339).map_err(|_| {
                ApiError::from(crate::application::ApplicationError::InvalidInput(
                    "execute_at must be an RFC 3339 timestamp".to_string(),
                ))
            })?,
        },
        _ => {
            return Err(ApiError::from(
                crate::application::ApplicationError::InvalidInput(
                    "exactly one of cron_expression or execute_at is required".to_string(),
                ),
            ));
        }
    };
    let schedule = state
        .application()
        .create_schedule(
            &context,
            &namespace,
            CreateScheduleInput {
                name: request.name,
                job: request.job,
                target: request.target,
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
    path = "/api/namespaces/{namespace}/schedules",
    params(("namespace" = String, Path), PageQuery),
    responses((status = 200, body = Page<ScheduleResource>)),
    tag = "control-plane"
)]
pub async fn list_schedules(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path(namespace): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page<ScheduleResource>>, ApiError> {
    let page = state
        .application()
        .list_schedules(&context, &namespace, query.limit, query.after.as_deref())
        .await?;
    Ok(Json(map_page(page, |item| schedule_resource(&item))?))
}

#[utoipa::path(
    get,
    path = "/api/namespaces/{namespace}/schedules/{schedule}",
    params(("namespace" = String, Path), ("schedule" = String, Path)),
    responses((status = 200, body = ScheduleResource), (status = 404, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn get_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path((namespace, schedule)): Path<(String, String)>,
) -> Result<Json<ScheduleResource>, ApiError> {
    let record = state
        .application()
        .get_schedule(&context, &namespace, &schedule)
        .await?;
    Ok(Json(schedule_resource(&record)?))
}

#[utoipa::path(
    patch,
    path = "/api/namespaces/{namespace}/schedules/{schedule}",
    params(("namespace" = String, Path), ("schedule" = String, Path)),
    request_body = UpdateScheduleRequest,
    responses((status = 200, body = ScheduleResource), (status = 404, body = crono_api::ErrorEnvelope), (status = 409, body = crono_api::ErrorEnvelope)),
    tag = "control-plane"
)]
pub async fn update_schedule(
    State(state): State<AppState>,
    Extension(context): Extension<RequestContext>,
    Path((namespace, schedule)): Path<(String, String)>,
    Json(request): Json<UpdateScheduleRequest>,
) -> Result<Json<ScheduleResource>, ApiError> {
    let record = state
        .application()
        .set_schedule_enabled(
            &context,
            &namespace,
            &schedule,
            request.revision,
            request.enabled,
        )
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

fn job_resource(record: &JobRecord) -> Result<JobResource, ApiError> {
    let executor = match record.job.executor() {
        DomainExecutor::Noop => ExecutorKind::Noop,
        DomainExecutor::Process => ExecutorKind::Process,
    };
    Ok(JobResource {
        id: record.job.id().get(),
        namespace: record.namespace.to_string(),
        name: record.job.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.job.name()),
        executor,
        queue: record.job.queue().to_string(),
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
        namespace: record.namespace.to_string(),
        name: record.target.name().to_string(),
        qualified_name: format!("{}/{}", record.namespace, record.target.name()),
        arguments: record.target.arguments().to_vec(),
        created_at: timestamp(record.target.created_at())?,
        updated_at: timestamp(record.target.updated_at())?,
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
        namespace: record.namespace.to_string(),
        name: record.schedule.name.to_string(),
        job: format!("{}/{}", record.namespace, record.job_name),
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
        job: format!("{}/{}", record.job_namespace, record.job_name),
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
