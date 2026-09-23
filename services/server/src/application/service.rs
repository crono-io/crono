//! Authorized orchestration of domain parsing and persistence operations.

use super::{
    ApplicationError, Authorizer, Capability, ControlPlaneStore, CreateJobInput,
    CreateScheduleInput, JobDefinition, JobRecord, NewSchedule, Overview, Page, RequestContext,
    ResourceScope, RunRecord, ScheduleRecord, TargetRecord, WorkerRecord,
};
use crate::{
    domain::{
        ExecutorKind, MisfirePolicy, Namespace, NamespaceName, QueueName, ResourceName, RunId,
        ScheduleTiming,
    },
    scheduler::next_cron_occurrence,
};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_LIMIT: u16 = 50;
const MAX_LIMIT: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRunOutcome {
    pub run: RunRecord,
    pub created: bool,
}

#[derive(Clone)]
pub struct Application {
    store: Arc<dyn ControlPlaneStore>,
    authorizer: Arc<dyn Authorizer>,
}

impl Application {
    #[must_use]
    pub fn new(store: Arc<dyn ControlPlaneStore>, authorizer: Arc<dyn Authorizer>) -> Self {
        Self { store, authorizer }
    }

    /// Authorize, validate, and persist one Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_namespace(
        &self,
        context: &RequestContext,
        value: &str,
    ) -> Result<Namespace, ApplicationError> {
        self.authorizer
            .authorize(
                context,
                Capability::NamespaceCreate,
                &ResourceScope::ControlPlane,
            )
            .await?;
        let name = NamespaceName::parse(value).map_err(invalid)?;
        Ok(self.store.create_namespace(&name).await?)
    }

    /// List only Namespaces visible to the established principal.
    ///
    /// # Errors
    ///
    /// Returns invalid pagination, authorization, or dependency failures.
    pub async fn list_namespaces(
        &self,
        context: &RequestContext,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<Namespace>, ApplicationError> {
        let visibility = self
            .authorizer
            .visibility(context, Capability::NamespaceRead)
            .await?;
        Ok(self
            .store
            .list_namespaces(&visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read one Namespace after its resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or dependency failures.
    pub async fn get_namespace(
        &self,
        context: &RequestContext,
        value: &str,
    ) -> Result<Namespace, ApplicationError> {
        let name = NamespaceName::parse(value).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::NamespaceRead,
                &ResourceScope::Namespace(name.to_string()),
            )
            .await?;
        Ok(self.store.get_namespace(&name).await?)
    }

    /// Create a directly editable Job definition after Namespace authorization.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_job(
        &self,
        context: &RequestContext,
        namespace: &str,
        input: CreateJobInput,
    ) -> Result<JobRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(&input.name).map_err(invalid)?;
        let queue = QueueName::parse(&input.queue).map_err(invalid)?;
        validate_job(&input)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobCreate,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        let definition = JobDefinition {
            executor: input.executor,
            queue,
            executable: input.executable,
            arguments: input.arguments,
            idempotent: input.idempotent,
            max_attempts: input.max_attempts,
            retry_initial_seconds: input.retry_initial_seconds,
            retry_max_seconds: input.retry_max_seconds,
            retry_multiplier: input.retry_multiplier,
            retry_jitter: input.retry_jitter,
        };
        Ok(self
            .store
            .create_job(&namespace, &name, &definition)
            .await?)
    }

    /// List visible Jobs from one authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, pagination, authorization, or dependency failures.
    pub async fn list_jobs(
        &self,
        context: &RequestContext,
        namespace: &str,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobRead,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::JobRead)
            .await?;
        Ok(self
            .store
            .list_jobs(&namespace, &visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read a qualified Job after a resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or dependency failures.
    pub async fn get_job(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
    ) -> Result<JobRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobRead,
                &ResourceScope::Job {
                    namespace: namespace.to_string(),
                    job: name.to_string(),
                },
            )
            .await?;
        Ok(self.store.get_job(&namespace, &name).await?)
    }

    /// Create a Target whose arguments will be snapshotted into future Runs.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_target(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
        arguments: Vec<String>,
    ) -> Result<TargetRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        validate_arguments(&arguments)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetCreate,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        Ok(self
            .store
            .create_target(&namespace, &name, &arguments)
            .await?)
    }

    /// List visible Targets from one authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, pagination, authorization, or dependency failures.
    pub async fn list_targets(
        &self,
        context: &RequestContext,
        namespace: &str,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetRead,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::TargetRead)
            .await?;
        Ok(self
            .store
            .list_targets(&namespace, &visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read a qualified Target after a resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or dependency failures.
    pub async fn get_target(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
    ) -> Result<TargetRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetRead,
                &ResourceScope::Target {
                    namespace: namespace.to_string(),
                    target: name.to_string(),
                },
            )
            .await?;
        Ok(self.store.get_target(&namespace, &name).await?)
    }

    /// Create a durable Schedule without consulting NATS.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, conflict, or storage failures.
    pub async fn create_schedule(
        &self,
        context: &RequestContext,
        namespace: &str,
        input: CreateScheduleInput,
    ) -> Result<ScheduleRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(&input.name).map_err(invalid)?;
        let (job_namespace, job_name) = qualified(&input.job)?;
        let (target_namespace, target_name) = qualified(&input.target)?;
        if namespace != job_namespace || namespace != target_namespace {
            return Err(ApplicationError::InvalidInput(
                "Schedule, Job, and Target must belong to the same Namespace".to_string(),
            ));
        }
        validate_schedule_policy(
            input.misfire_policy,
            input.misfire_grace_seconds,
            input.max_catchup_runs,
            input.max_catchup_age_seconds,
        )?;
        let now = OffsetDateTime::now_utc();
        let (cron_expression, execute_at, timezone, next_run_at) = match input.timing {
            ScheduleTiming::Cron {
                expression,
                timezone,
            } => {
                let next = next_cron_occurrence(&expression, &timezone, now)
                    .map_err(|error| ApplicationError::InvalidInput(error.to_string()))?;
                (Some(expression), None, timezone, next)
            }
            ScheduleTiming::Once { execute_at } => {
                (None, Some(execute_at), "UTC".to_string(), execute_at)
            }
        };
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleCreate,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        self.store
            .create_schedule(&NewSchedule {
                namespace,
                name,
                job_name,
                target_name,
                cron_expression,
                execute_at,
                timezone,
                next_run_at,
                misfire_policy: input.misfire_policy,
                misfire_grace_seconds: input.misfire_grace_seconds,
                catchup_policy: input.catchup_policy,
                max_catchup_runs: input.max_catchup_runs,
                max_catchup_age_seconds: input.max_catchup_age_seconds,
            })
            .await
            .map_err(Into::into)
    }

    /// List Schedules visible within one Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, pagination, or storage failures.
    pub async fn list_schedules(
        &self,
        context: &RequestContext,
        namespace: &str,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<ScheduleRecord>, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleRead,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::ScheduleRead)
            .await?;
        Ok(self
            .store
            .list_schedules(&namespace, &visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read one Schedule after its resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or storage failures.
    pub async fn get_schedule(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
    ) -> Result<ScheduleRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleRead,
                &ResourceScope::Schedule {
                    namespace: namespace.to_string(),
                    schedule: name.to_string(),
                },
            )
            .await?;
        Ok(self.store.get_schedule(&namespace, &name).await?)
    }

    /// Enable or disable a Schedule using optimistic revision matching.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, stale-revision, or storage failures.
    pub async fn set_schedule_enabled(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
        revision: u64,
        enabled: bool,
    ) -> Result<ScheduleRecord, ApplicationError> {
        let record = self.get_schedule(context, namespace, name).await?;
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleUpdate,
                &ResourceScope::Schedule {
                    namespace: namespace.to_string(),
                    schedule: name.to_string(),
                },
            )
            .await?;
        let next = if enabled {
            match &record.schedule.timing {
                ScheduleTiming::Cron {
                    expression,
                    timezone,
                } => Some(
                    next_cron_occurrence(expression, timezone, OffsetDateTime::now_utc())
                        .map_err(|error| ApplicationError::InvalidInput(error.to_string()))?,
                ),
                ScheduleTiming::Once { execute_at } => Some(*execute_at),
            }
        } else {
            None
        };
        Ok(self
            .store
            .set_schedule_enabled(record.schedule.id, revision, enabled, next)
            .await?)
    }

    /// Authorize every Run input and persist an idempotent dispatch request.
    ///
    /// # Errors
    ///
    /// Returns input, authorization, not-found, idempotency, or dependency failures.
    pub async fn create_run(
        &self,
        context: &RequestContext,
        request_id: Uuid,
        job: &str,
        target: &str,
    ) -> Result<CreateRunOutcome, ApplicationError> {
        let (job_namespace, job_name) = qualified(job)?;
        let (target_namespace, target_name) = qualified(target)?;
        if job_namespace != target_namespace {
            return Err(ApplicationError::InvalidInput(
                "Job and Target must belong to the same Namespace".to_string(),
            ));
        }
        self.authorizer
            .authorize(
                context,
                Capability::RunCreate,
                &ResourceScope::Namespace(job_namespace.to_string()),
            )
            .await?;
        self.authorizer
            .authorize(
                context,
                Capability::JobExecute,
                &ResourceScope::Job {
                    namespace: job_namespace.to_string(),
                    job: job_name.to_string(),
                },
            )
            .await?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetUse,
                &ResourceScope::Target {
                    namespace: target_namespace.to_string(),
                    target: target_name.to_string(),
                },
            )
            .await?;
        let (run, created) = self
            .store
            .create_run(
                request_id,
                &job_namespace,
                &job_name,
                &target_namespace,
                &target_name,
            )
            .await?;
        Ok(CreateRunOutcome { run, created })
    }

    /// List Runs after applying principal visibility within the SQL query.
    ///
    /// # Errors
    ///
    /// Returns pagination, authorization, or dependency failures.
    pub async fn list_runs(
        &self,
        context: &RequestContext,
        limit: Option<u16>,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, ApplicationError> {
        let visibility = self
            .authorizer
            .visibility(context, Capability::RunRead)
            .await?;
        Ok(self
            .store
            .list_runs(&visibility, page_limit(limit)?, before)
            .await?)
    }

    /// Read one Run after its resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns authorization, not-found, or dependency failures.
    pub async fn get_run(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<RunRecord, ApplicationError> {
        self.authorizer
            .authorize(context, Capability::RunRead, &ResourceScope::Run(id))
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::RunRead)
            .await?;
        Ok(self.store.get_run(RunId::new(id), &visibility).await?)
    }

    /// List recently observed workers after a control-plane authorization decision.
    ///
    /// Presence is operational metadata rather than Namespace-owned data, so
    /// callers require the global worker-read capability.
    ///
    /// # Errors
    ///
    /// Returns invalid pagination, authorization, or dependency failures.
    pub async fn list_workers(
        &self,
        context: &RequestContext,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<WorkerRecord>, ApplicationError> {
        self.authorizer
            .authorize(
                context,
                Capability::WorkerRead,
                &ResourceScope::ControlPlane,
            )
            .await?;
        Ok(self.store.list_workers(page_limit(limit)?, after).await?)
    }

    /// Count only resources visible to the established principal.
    ///
    /// # Errors
    ///
    /// Returns authorization or dependency failures.
    pub async fn overview(&self, context: &RequestContext) -> Result<Overview, ApplicationError> {
        let visibility = self
            .authorizer
            .visibility(context, Capability::NamespaceRead)
            .await?;
        Ok(self.store.overview(&visibility).await?)
    }
}

fn page_limit(limit: Option<u16>) -> Result<u16, ApplicationError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        Err(ApplicationError::InvalidInput(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )))
    } else {
        Ok(limit)
    }
}

fn invalid(error: crate::domain::NameError) -> ApplicationError {
    ApplicationError::InvalidInput(error.to_string())
}

fn qualified(value: &str) -> Result<(NamespaceName, ResourceName), ApplicationError> {
    let Some((namespace, resource)) = value.split_once('/') else {
        return Err(ApplicationError::InvalidInput(
            "qualified resource names must use namespace/resource".to_string(),
        ));
    };
    if resource.contains('/') {
        return Err(ApplicationError::InvalidInput(
            "qualified resource names must contain one slash".to_string(),
        ));
    }
    Ok((
        NamespaceName::parse(namespace).map_err(invalid)?,
        ResourceName::parse(resource).map_err(invalid)?,
    ))
}

fn validate_arguments(arguments: &[String]) -> Result<(), ApplicationError> {
    if arguments.len() > 128
        || arguments
            .iter()
            .any(|argument| argument.len() > 4096 || argument.contains('\0'))
    {
        return Err(ApplicationError::InvalidInput(
            "arguments must contain at most 128 bounded, NUL-free values".to_string(),
        ));
    }
    Ok(())
}

fn validate_job(input: &CreateJobInput) -> Result<(), ApplicationError> {
    validate_arguments(&input.arguments)?;
    if !(1..=100).contains(&input.max_attempts) {
        return Err(ApplicationError::InvalidInput(
            "max_attempts must be between 1 and 100".to_string(),
        ));
    }
    if !(1..=86_400).contains(&input.retry_initial_seconds)
        || input.retry_max_seconds < input.retry_initial_seconds
        || input.retry_max_seconds > 86_400
        || !input.retry_multiplier.is_finite()
        || input.retry_multiplier < 1.0
        || !input.retry_jitter.is_finite()
        || !(0.0..=1.0).contains(&input.retry_jitter)
    {
        return Err(ApplicationError::InvalidInput(
            "execution retry policy is outside supported bounds".to_string(),
        ));
    }
    match (input.executor, input.executable.as_deref()) {
        (ExecutorKind::Noop, None) => Ok(()),
        (ExecutorKind::Process, Some(path)) if path.starts_with('/') && !path.contains('\0') => {
            Ok(())
        }
        _ => Err(ApplicationError::InvalidInput(
            "process jobs require an absolute executable; noop jobs require none".to_string(),
        )),
    }
}

fn validate_schedule_policy(
    policy: MisfirePolicy,
    grace_seconds: Option<u32>,
    max_catchup_runs: u16,
    max_catchup_age_seconds: u32,
) -> Result<(), ApplicationError> {
    if matches!(policy, MisfirePolicy::GracePeriod) != grace_seconds.is_some() {
        return Err(ApplicationError::InvalidInput(
            "grace_period requires grace seconds and other policies forbid it".to_string(),
        ));
    }
    if !(1..=1000).contains(&max_catchup_runs)
        || !(60..=31_536_000).contains(&max_catchup_age_seconds)
    {
        return Err(ApplicationError::InvalidInput(
            "catch-up limits are outside supported bounds".to_string(),
        ));
    }
    Ok(())
}
