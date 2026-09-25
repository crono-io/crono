//! Authorized orchestration of domain parsing and persistence operations.

use super::{
    ApplicationError, Authorizer, Capability, ControlPlaneStore, CreateJobInput, CreateQueueInput,
    CreateScheduleInput, JobDefinition, JobRecord, MonitorSnapshot, NewSchedule, Overview, Page,
    RequestContext, ResourceScope, RunAttemptRecord, RunRecord, ScheduleRecord, StoreError,
    TargetDefinition, TargetRecord, TargetSetRecord, UpdateQueueInput, WorkerRecord,
};
use crate::{
    domain::{
        ExecutorKind, Job, JobId, MisfirePolicy, Namespace, NamespaceId, NamespaceName, Queue,
        QueueId, QueueName, ResourceName, RunId, ScheduleId, ScheduleTiming, Target, TargetId,
        TargetSelection, TargetSetId,
    },
    scheduler::next_cron_occurrence,
};
use crono_execution::{
    merge_inputs, render_arguments, validate_argument_templates, validate_inputs,
};
use std::{collections::BTreeSet, sync::Arc};
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_LIMIT: u16 = 50;
const MAX_LIMIT: u16 = 100;
const MAX_TARGET_SET_MEMBERS: usize = 1_000;
const MAX_QUEUE_DESCRIPTION_CHARACTERS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRunOutcome {
    pub runs: Vec<RunRecord>,
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
        let name = NamespaceName::parse(value).map_err(invalid_name)?;
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
        id: Uuid,
    ) -> Result<Namespace, ApplicationError> {
        let id = NamespaceId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::NamespaceRead,
                &ResourceScope::Namespace(id),
            )
            .await?;
        Ok(self.store.get_namespace(id).await?)
    }

    /// Authorize, validate, and persist one global worker Queue.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_queue(
        &self,
        context: &RequestContext,
        input: CreateQueueInput,
    ) -> Result<Queue, ApplicationError> {
        self.authorizer
            .authorize(
                context,
                Capability::QueueCreate,
                &ResourceScope::ControlPlane,
            )
            .await?;
        let name = QueueName::parse(&input.name).map_err(invalid_name)?;
        validate_queue_description(input.description.as_deref())?;
        Ok(self
            .store
            .create_queue(&name, input.description.as_deref())
            .await?)
    }

    /// List Queues after a global read authorization decision.
    ///
    /// # Errors
    ///
    /// Returns invalid pagination, authorization, or dependency failures.
    pub async fn list_queues(
        &self,
        context: &RequestContext,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<Queue>, ApplicationError> {
        self.authorizer
            .authorize(context, Capability::QueueRead, &ResourceScope::ControlPlane)
            .await?;
        Ok(self.store.list_queues(page_limit(limit)?, after).await?)
    }

    /// Read one Queue by immutable identity.
    ///
    /// # Errors
    ///
    /// Returns authorization, not-found, or dependency failures.
    pub async fn get_queue(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<Queue, ApplicationError> {
        let id = QueueId::new(id);
        self.authorizer
            .authorize(context, Capability::QueueRead, &ResourceScope::Queue(id))
            .await?;
        Ok(self.store.get_queue(id).await?)
    }

    /// Replace editable Queue metadata while preserving its routing UUID.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, not-found, or dependency failures.
    pub async fn update_queue(
        &self,
        context: &RequestContext,
        id: Uuid,
        input: UpdateQueueInput,
    ) -> Result<Queue, ApplicationError> {
        let id = QueueId::new(id);
        let name = QueueName::parse(&input.name).map_err(invalid_name)?;
        validate_queue_description(input.description.as_deref())?;
        self.authorizer
            .authorize(context, Capability::QueueUpdate, &ResourceScope::Queue(id))
            .await?;
        let current = self.store.get_queue(id).await?;
        if current.system() && name.as_str() != current.name().as_str() {
            return Err(ApplicationError::invalid(
                "name",
                "the system default Queue cannot be renamed",
            ));
        }
        if current.system() && !input.enabled {
            return Err(ApplicationError::invalid(
                "enabled",
                "the system default Queue cannot be disabled",
            ));
        }
        Ok(self
            .store
            .update_queue(id, &name, input.description.as_deref(), input.enabled)
            .await?)
    }

    /// Delete an unused Queue without orphaning Job or worker relationships.
    ///
    /// PostgreSQL reference constraints are authoritative, so a Queue still in
    /// use returns a conflict rather than detaching related resources.
    ///
    /// # Errors
    ///
    /// Returns authorization, conflict, not-found, or dependency failures.
    pub async fn delete_queue(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<(), ApplicationError> {
        let id = QueueId::new(id);
        self.authorizer
            .authorize(context, Capability::QueueDelete, &ResourceScope::Queue(id))
            .await?;
        if self.store.get_queue(id).await?.system() {
            return Err(ApplicationError::invalid_request(
                "the system default Queue cannot be deleted",
            ));
        }
        Ok(self.store.delete_queue(id).await?)
    }

    /// Create a directly editable Job definition after Namespace authorization.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_job(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        input: CreateJobInput,
    ) -> Result<JobRecord, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        let name = ResourceName::parse(&input.name).map_err(invalid_name)?;
        validate_job(&input)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobCreate,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let queue_id = QueueId::new(input.queue_id);
        self.authorizer
            .authorize(
                context,
                Capability::QueueRead,
                &ResourceScope::Queue(queue_id),
            )
            .await?;
        let queue = self.store.get_queue(queue_id).await.map_err(|error| {
            if error == StoreError::NotFound {
                ApplicationError::invalid("queue_id", "Select an existing Queue.")
            } else {
                error.into()
            }
        })?;
        if !queue.enabled() {
            return Err(ApplicationError::invalid(
                "queue_id",
                "Select an enabled Queue.",
            ));
        }
        let definition = JobDefinition {
            executor: input.executor,
            queue_id,
            executable: input.executable,
            arguments: input.arguments,
            inputs: input.inputs,
            idempotent: input.idempotent,
            dry_run: input.dry_run,
            max_attempts: input.max_attempts,
            retry_initial_seconds: input.retry_initial_seconds,
            retry_max_seconds: input.retry_max_seconds,
            retry_multiplier: input.retry_multiplier,
            retry_jitter: input.retry_jitter,
        };
        Ok(self
            .store
            .create_job(namespace_id, &name, &definition)
            .await?)
    }

    /// Replace a Job definition while preserving its identity and Namespace.
    ///
    /// Existing Runs retain their immutable snapshots; only future Runs observe
    /// the updated command, inputs, Queue, and retry policy.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, conflict, or storage failures.
    pub async fn update_job(
        &self,
        context: &RequestContext,
        id: Uuid,
        input: CreateJobInput,
    ) -> Result<JobRecord, ApplicationError> {
        let id = JobId::new(id);
        let name = ResourceName::parse(&input.name).map_err(invalid_name)?;
        validate_job(&input)?;
        self.authorizer
            .authorize(context, Capability::JobUpdate, &ResourceScope::Job(id))
            .await?;
        let existing = self.store.get_job(id).await?;
        let queue_id = QueueId::new(input.queue_id);
        self.authorizer
            .authorize(
                context,
                Capability::QueueRead,
                &ResourceScope::Queue(queue_id),
            )
            .await?;
        let queue = self.store.get_queue(queue_id).await.map_err(|error| {
            if error == StoreError::NotFound {
                ApplicationError::invalid("queue_id", "Select an existing Queue.")
            } else {
                error.into()
            }
        })?;
        if !queue.enabled() && queue_id != existing.job.queue_id() {
            return Err(ApplicationError::invalid(
                "queue_id",
                "Select an enabled Queue.",
            ));
        }
        let definition = JobDefinition {
            executor: input.executor,
            queue_id,
            executable: input.executable,
            arguments: input.arguments,
            inputs: input.inputs,
            idempotent: input.idempotent,
            dry_run: input.dry_run,
            max_attempts: input.max_attempts,
            retry_initial_seconds: input.retry_initial_seconds,
            retry_max_seconds: input.retry_max_seconds,
            retry_multiplier: input.retry_multiplier,
            retry_jitter: input.retry_jitter,
        };
        Ok(self.store.update_job(id, &name, &definition).await?)
    }

    /// List visible Jobs from one authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, pagination, authorization, or dependency failures.
    pub async fn list_jobs(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        self.authorizer
            .authorize(
                context,
                Capability::JobRead,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::JobRead)
            .await?;
        Ok(self
            .store
            .list_jobs(namespace_id, &visibility, page_limit(limit)?, after)
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
        id: Uuid,
    ) -> Result<JobRecord, ApplicationError> {
        let id = JobId::new(id);
        self.authorizer
            .authorize(context, Capability::JobRead, &ResourceScope::Job(id))
            .await?;
        Ok(self.store.get_job(id).await?)
    }

    /// Create a Target whose arguments will be snapshotted into future Runs.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_target(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        name: &str,
        arguments: Vec<String>,
        inputs: serde_json::Value,
    ) -> Result<TargetRecord, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        let name = ResourceName::parse(name).map_err(invalid_name)?;
        validate_arguments(&arguments)?;
        validate_input_object(&inputs)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetCreate,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let definition = TargetDefinition { arguments, inputs };
        Ok(self
            .store
            .create_target(namespace_id, &name, &definition)
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
        namespace_id: Uuid,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        self.authorizer
            .authorize(
                context,
                Capability::TargetRead,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::TargetRead)
            .await?;
        Ok(self
            .store
            .list_targets(namespace_id, &visibility, page_limit(limit)?, after)
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
        id: Uuid,
    ) -> Result<TargetRecord, ApplicationError> {
        let id = TargetId::new(id);
        self.authorizer
            .authorize(context, Capability::TargetRead, &ResourceScope::Target(id))
            .await?;
        Ok(self.store.get_target(id).await?)
    }

    /// Replace Target arguments and inputs for future execution snapshots.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, conflict, or storage failures.
    pub async fn update_target(
        &self,
        context: &RequestContext,
        id: Uuid,
        name: &str,
        arguments: Vec<String>,
        inputs: serde_json::Value,
    ) -> Result<TargetRecord, ApplicationError> {
        let id = TargetId::new(id);
        let name = ResourceName::parse(name).map_err(invalid_name)?;
        validate_arguments(&arguments)?;
        validate_input_object(&inputs)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetUpdate,
                &ResourceScope::Target(id),
            )
            .await?;
        let definition = TargetDefinition { arguments, inputs };
        Ok(self.store.update_target(id, &name, &definition).await?)
    }

    /// Create a non-empty Target Set from explicit same-Namespace Target IDs.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, conflict, or storage failures.
    pub async fn create_target_set(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        name: &str,
        target_ids: Vec<Uuid>,
        inputs: serde_json::Value,
    ) -> Result<TargetSetRecord, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        let name = ResourceName::parse(name).map_err(invalid_name)?;
        validate_input_object(&inputs)?;
        if target_ids.is_empty() || target_ids.len() > MAX_TARGET_SET_MEMBERS {
            return Err(ApplicationError::invalid(
                "target_ids",
                format!("target_ids must contain between 1 and {MAX_TARGET_SET_MEMBERS} values"),
            ));
        }
        let unique: BTreeSet<Uuid> = target_ids.iter().copied().collect();
        if unique.len() != target_ids.len() {
            return Err(ApplicationError::invalid(
                "target_ids",
                "target_ids must not contain duplicates",
            ));
        }
        self.authorizer
            .authorize(
                context,
                Capability::TargetSetCreate,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let ids: Vec<TargetId> = target_ids.into_iter().map(TargetId::new).collect();
        for id in &ids {
            self.authorizer
                .authorize(context, Capability::TargetRead, &ResourceScope::Target(*id))
                .await?;
            let target = match self.store.get_target(*id).await {
                Ok(target) => target,
                Err(StoreError::NotFound) => {
                    return Err(ApplicationError::invalid(
                        "target_ids",
                        "one or more selected Targets no longer exist",
                    ));
                }
                Err(error) => return Err(error.into()),
            };
            if target.target.namespace_id() != namespace_id {
                return Err(ApplicationError::invalid(
                    "target_ids",
                    "all selected Targets must belong to the Target Set Namespace",
                ));
            }
        }
        Ok(self
            .store
            .create_target_set(namespace_id, &name, &ids, &inputs)
            .await?)
    }

    /// List visible Target Sets from one authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns invalid pagination, authorization, or storage failures.
    pub async fn list_target_sets(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<TargetSetRecord>, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        self.authorizer
            .authorize(
                context,
                Capability::TargetSetRead,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::TargetSetRead)
            .await?;
        Ok(self
            .store
            .list_target_sets(namespace_id, &visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read one Target Set by immutable identity.
    ///
    /// # Errors
    ///
    /// Returns authorization, not-found, or storage failures.
    pub async fn get_target_set(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<TargetSetRecord, ApplicationError> {
        let id = TargetSetId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::TargetSetRead,
                &ResourceScope::TargetSet(id),
            )
            .await?;
        Ok(self.store.get_target_set(id).await?)
    }

    /// Replace Target Set membership and shared inputs atomically.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, conflict, or storage failures.
    pub async fn update_target_set(
        &self,
        context: &RequestContext,
        id: Uuid,
        name: &str,
        target_ids: Vec<Uuid>,
        inputs: serde_json::Value,
    ) -> Result<TargetSetRecord, ApplicationError> {
        let id = TargetSetId::new(id);
        let existing = self.store.get_target_set(id).await?;
        let namespace_id = existing.target_set.namespace_id();
        let name = ResourceName::parse(name).map_err(invalid_name)?;
        validate_input_object(&inputs)?;
        validate_target_ids(&target_ids)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetSetUpdate,
                &ResourceScope::TargetSet(id),
            )
            .await?;
        let ids: Vec<TargetId> = target_ids.into_iter().map(TargetId::new).collect();
        self.validate_target_members(context, namespace_id, &ids)
            .await?;
        Ok(self
            .store
            .update_target_set(id, &name, &ids, &inputs)
            .await?)
    }

    /// Create a durable Schedule without consulting NATS.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, conflict, or storage failures.
    pub async fn create_schedule(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        input: CreateScheduleInput,
    ) -> Result<ScheduleRecord, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        let name = ResourceName::parse(&input.name).map_err(invalid_name)?;
        validate_input_object(&input.inputs)?;
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleCreate,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        self.authorizer
            .authorize(
                context,
                Capability::JobRead,
                &ResourceScope::Job(input.job_id),
            )
            .await?;
        let job = self.store.get_job(input.job_id).await?;
        let (selection_namespace, target_set_inputs, targets) =
            self.execution_targets(context, input.target).await?;
        if job.job.namespace_id() != namespace_id || selection_namespace != namespace_id {
            return Err(ApplicationError::invalid_request(
                "Schedule, Job, and execution target must belong to the same Namespace",
            ));
        }
        for selected_target in &targets {
            validate_rendered_execution(
                &job.job,
                target_set_inputs.as_ref(),
                selected_target,
                &input.inputs,
            )?;
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
                let next = next_cron_occurrence(&expression, &timezone, now).map_err(|error| {
                    ApplicationError::invalid("cron_expression", error.to_string())
                })?;
                (Some(expression), None, timezone, next)
            }
            ScheduleTiming::Once { execute_at } => {
                (None, Some(execute_at), "UTC".to_string(), execute_at)
            }
        };
        self.store
            .create_schedule(&NewSchedule {
                namespace_id,
                name,
                job_id: input.job_id,
                target: input.target,
                inputs: input.inputs,
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
        namespace_id: Uuid,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<ScheduleRecord>, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleRead,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::ScheduleRead)
            .await?;
        Ok(self
            .store
            .list_schedules(namespace_id, &visibility, page_limit(limit)?, after)
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
        id: Uuid,
    ) -> Result<ScheduleRecord, ApplicationError> {
        let id = ScheduleId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleRead,
                &ResourceScope::Schedule(id),
            )
            .await?;
        Ok(self.store.get_schedule(id).await?)
    }

    /// Enable or disable a Schedule using optimistic revision matching.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, stale-revision, or storage failures.
    pub async fn set_schedule_enabled(
        &self,
        context: &RequestContext,
        id: Uuid,
        revision: u64,
        enabled: bool,
    ) -> Result<ScheduleRecord, ApplicationError> {
        let id = ScheduleId::new(id);
        let record = self.get_schedule(context, id.get()).await?;
        self.authorizer
            .authorize(
                context,
                Capability::ScheduleUpdate,
                &ResourceScope::Schedule(id),
            )
            .await?;
        let next = if enabled {
            match &record.schedule.timing {
                ScheduleTiming::Cron {
                    expression,
                    timezone,
                } => Some(
                    next_cron_occurrence(expression, timezone, OffsetDateTime::now_utc()).map_err(
                        |error| ApplicationError::invalid("cron_expression", error.to_string()),
                    )?,
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
        job_id: Uuid,
        target: TargetSelection,
        inputs: serde_json::Value,
    ) -> Result<CreateRunOutcome, ApplicationError> {
        let job_id = JobId::new(job_id);
        validate_input_object(&inputs)?;
        self.authorizer
            .authorize(context, Capability::JobExecute, &ResourceScope::Job(job_id))
            .await?;
        let job = self.store.get_job(job_id).await?;
        let (selection_namespace, target_set_inputs, targets) =
            self.execution_targets(context, target).await?;
        if job.job.namespace_id() != selection_namespace {
            return Err(ApplicationError::invalid_request(
                "Job and execution target must belong to the same Namespace",
            ));
        }
        for selected_target in &targets {
            validate_rendered_execution(
                &job.job,
                target_set_inputs.as_ref(),
                selected_target,
                &inputs,
            )?;
        }
        self.authorizer
            .authorize(
                context,
                Capability::RunCreate,
                &ResourceScope::Namespace(job.job.namespace_id()),
            )
            .await?;
        let (runs, created) = self
            .store
            .create_runs(request_id, job_id, target, &inputs)
            .await?;
        Ok(CreateRunOutcome { runs, created })
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

    /// Read bounded Attempt output after authorizing the Run and applying its
    /// Namespace visibility inside the persistence query.
    ///
    /// # Errors
    ///
    /// Returns authorization, not-found, or dependency failures.
    pub async fn list_run_attempts(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<Vec<RunAttemptRecord>, ApplicationError> {
        self.authorizer
            .authorize(context, Capability::RunRead, &ResourceScope::Run(id))
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::RunRead)
            .await?;
        Ok(self
            .store
            .list_run_attempts(RunId::new(id), &visibility)
            .await?)
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

    /// Authorize operator monitoring before reading aggregate database state.
    ///
    /// A failed sample is returned as absent so the monitor can still report
    /// independently probed dependency health without leaking database errors.
    ///
    /// # Errors
    ///
    /// Returns an authorization failure before sampling when monitor access is denied.
    pub async fn monitor_snapshot(
        &self,
        context: &RequestContext,
    ) -> Result<Option<MonitorSnapshot>, ApplicationError> {
        self.authorizer
            .authorize(
                context,
                Capability::MonitorRead,
                &ResourceScope::ControlPlane,
            )
            .await?;
        match self.store.monitor_snapshot().await {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(error) => {
                tracing::warn!(%error, "operator monitor database sample unavailable");
                Ok(None)
            }
        }
    }

    async fn validate_target_members(
        &self,
        context: &RequestContext,
        namespace_id: NamespaceId,
        ids: &[TargetId],
    ) -> Result<(), ApplicationError> {
        for id in ids {
            self.authorizer
                .authorize(context, Capability::TargetRead, &ResourceScope::Target(*id))
                .await?;
            let target = self.store.get_target(*id).await.map_err(|error| {
                if error == StoreError::NotFound {
                    ApplicationError::invalid(
                        "target_ids",
                        "one or more selected Targets no longer exist",
                    )
                } else {
                    error.into()
                }
            })?;
            if target.target.namespace_id() != namespace_id {
                return Err(ApplicationError::invalid(
                    "target_ids",
                    "all selected Targets must belong to the Target Set Namespace",
                ));
            }
        }
        Ok(())
    }

    async fn execution_targets(
        &self,
        context: &RequestContext,
        selection: TargetSelection,
    ) -> Result<(NamespaceId, Option<serde_json::Value>, Vec<Target>), ApplicationError> {
        match selection {
            TargetSelection::Target(id) => {
                self.authorizer
                    .authorize(context, Capability::TargetUse, &ResourceScope::Target(id))
                    .await?;
                let record = self.store.get_target(id).await?;
                Ok((record.target.namespace_id(), None, vec![record.target]))
            }
            TargetSelection::TargetSet(id) => {
                self.authorizer
                    .authorize(
                        context,
                        Capability::TargetSetUse,
                        &ResourceScope::TargetSet(id),
                    )
                    .await?;
                let record = self.store.get_target_set(id).await?;
                for target in &record.targets {
                    self.authorizer
                        .authorize(
                            context,
                            Capability::TargetUse,
                            &ResourceScope::Target(target.id()),
                        )
                        .await?;
                }
                Ok((
                    record.target_set.namespace_id(),
                    Some(record.target_set.inputs().clone()),
                    record.targets,
                ))
            }
        }
    }
}

fn page_limit(limit: Option<u16>) -> Result<u16, ApplicationError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        Err(ApplicationError::invalid(
            "limit",
            format!("limit must be between 1 and {MAX_LIMIT}"),
        ))
    } else {
        Ok(limit)
    }
}

fn invalid_name(error: crate::domain::NameError) -> ApplicationError {
    ApplicationError::invalid("name", error.to_string())
}

fn validate_queue_description(description: Option<&str>) -> Result<(), ApplicationError> {
    if description.is_some_and(|value| {
        value.contains('\0') || value.chars().count() > MAX_QUEUE_DESCRIPTION_CHARACTERS
    }) {
        return Err(ApplicationError::invalid(
            "description",
            format!(
                "description must be NUL-free and no longer than {MAX_QUEUE_DESCRIPTION_CHARACTERS} characters"
            ),
        ));
    }
    Ok(())
}

fn validate_arguments(arguments: &[String]) -> Result<(), ApplicationError> {
    if arguments.len() > 128
        || arguments
            .iter()
            .any(|argument| argument.len() > 4096 || argument.contains('\0'))
    {
        return Err(ApplicationError::invalid(
            "arguments",
            "arguments must contain at most 128 bounded, NUL-free values",
        ));
    }
    validate_argument_templates(arguments)
        .map_err(|error| ApplicationError::invalid("arguments", error.to_string()))
}

fn validate_input_object(inputs: &serde_json::Value) -> Result<(), ApplicationError> {
    validate_inputs(inputs).map_err(|error| ApplicationError::invalid("inputs", error.to_string()))
}

fn validate_target_ids(target_ids: &[Uuid]) -> Result<(), ApplicationError> {
    if target_ids.is_empty() || target_ids.len() > MAX_TARGET_SET_MEMBERS {
        return Err(ApplicationError::invalid(
            "target_ids",
            format!("target_ids must contain between 1 and {MAX_TARGET_SET_MEMBERS} values"),
        ));
    }
    let unique: BTreeSet<Uuid> = target_ids.iter().copied().collect();
    if unique.len() != target_ids.len() {
        return Err(ApplicationError::invalid(
            "target_ids",
            "target_ids must not contain duplicates",
        ));
    }
    Ok(())
}

fn validate_rendered_execution(
    job: &Job,
    target_set_inputs: Option<&serde_json::Value>,
    target: &Target,
    invocation_inputs: &serde_json::Value,
) -> Result<(), ApplicationError> {
    let empty = serde_json::json!({});
    let merged = merge_inputs(&[
        job.inputs(),
        target_set_inputs.unwrap_or(&empty),
        target.inputs(),
        invocation_inputs,
    ])
    .map_err(|error| ApplicationError::invalid("inputs", error.to_string()))?;
    let mut arguments = job.arguments().to_vec();
    arguments.extend_from_slice(target.arguments());
    render_arguments(&arguments, &merged)
        .map(|_| ())
        .map_err(|error| ApplicationError::invalid("arguments", error.to_string()))
}

fn validate_job(input: &CreateJobInput) -> Result<(), ApplicationError> {
    validate_arguments(&input.arguments)?;
    validate_input_object(&input.inputs)?;
    if !(1..=100).contains(&input.max_attempts) {
        return Err(ApplicationError::invalid(
            "max_attempts",
            "max_attempts must be between 1 and 100",
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
        return Err(ApplicationError::invalid_request(
            "execution retry policy is outside supported bounds",
        ));
    }
    match (input.executor, input.executable.as_deref()) {
        (ExecutorKind::Noop, None) => Ok(()),
        (ExecutorKind::Process, Some(path)) if path.starts_with('/') && !path.contains('\0') => {
            Ok(())
        }
        _ => Err(ApplicationError::invalid(
            "executable",
            "process jobs require an absolute executable; noop jobs require none",
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
        return Err(ApplicationError::invalid(
            "misfire_grace_seconds",
            "grace_period requires grace seconds and other policies forbid it",
        ));
    }
    if !(1..=1000).contains(&max_catchup_runs)
        || !(60..=31_536_000).contains(&max_catchup_age_seconds)
    {
        return Err(ApplicationError::invalid_request(
            "catch-up limits are outside supported bounds",
        ));
    }
    Ok(())
}
