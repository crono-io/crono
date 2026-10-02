//! Workflow use cases authorize an immutable execution intent before persistence.
//!
//! Launch checks every Job and selected Target, including Target Set membership.
//! PostgreSQL pins the authorized graph revision and exact Target IDs, snapshots
//! ordinary executions, and starts roots atomically. Later orchestration consumes
//! that committed authority; it never impersonates the caller or grants new work.
//! Reads apply Namespace visibility before exposing graphs or child Run IDs.

use super::{
    Application, ApplicationError, Capability, Page, RequestContext, ResourceScope, VisibilityScope,
};
use crate::domain::{
    DependencyCondition, WorkflowDefinition, WorkflowEdge, WorkflowId, WorkflowNode,
    WorkflowNodeId, WorkflowNodeRunId, WorkflowNodeState, WorkflowRunId, WorkflowState,
};
use crate::domain::{
    JobId, NamespaceId, NamespaceName, ResourceName, RunId, Target, TargetId, TargetSelection,
};
use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

/// Untrusted definition values parsed and validated at the application boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowInput {
    pub name: String,
    pub description: Option<String>,
    pub nodes: Vec<(String, JobId)>,
    pub edges: Vec<(String, String, DependencyCondition)>,
}

/// Catalog graph with stable Namespace identity and optimistic revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRecord {
    pub id: WorkflowId,
    pub namespace_id: NamespaceId,
    pub namespace: NamespaceName,
    pub revision: u64,
    pub definition: WorkflowDefinition,
    pub node_ids: BTreeMap<String, WorkflowNodeId>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// One per-target ordinary Run, allocated only after the node becomes eligible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowChildRun {
    pub target_id: TargetId,
    pub run_id: RunId,
}

/// One logical node invocation aggregates its normal Target Set fan-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowNodeRunRecord {
    pub id: WorkflowNodeRunId,
    pub workflow_node_id: WorkflowNodeId,
    pub name: ResourceName,
    pub job_id: JobId,
    pub state: WorkflowNodeState,
    pub runs: Vec<WorkflowChildRun>,
    pub started_at: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
}

/// Durable invocation plus immutable graph, selection, and node history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunRecord {
    pub id: WorkflowRunId,
    pub request_id: Uuid,
    pub workflow: WorkflowRecord,
    pub target: TargetSelection,
    pub inputs: serde_json::Value,
    pub state: WorkflowState,
    pub cancellation_requested: bool,
    pub nodes: Vec<WorkflowNodeRunRecord>,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
}

/// Launch carries the exact server-authorized revision and Target membership.
pub struct WorkflowLaunch {
    pub workflow_id: WorkflowId,
    pub revision: u64,
    pub request_id: Uuid,
    pub target: TargetSelection,
    pub inputs: serde_json::Value,
    pub authorized_targets: Vec<TargetId>,
}

impl WorkflowInput {
    /// Trim name padding, then reject collisions and invalid graphs before writing.
    /// Node names and edge endpoints share normalization; literal descriptions do not.
    fn definition(self) -> Result<WorkflowDefinition, ApplicationError> {
        let definition = WorkflowDefinition {
            name: ResourceName::parse(self.name.trim()).map_err(super::service::invalid_name)?,
            description: self.description,
            nodes: self
                .nodes
                .into_iter()
                .map(|(name, job_id)| {
                    Ok(WorkflowNode {
                        name: ResourceName::parse(name.trim())
                            .map_err(super::service::invalid_name)?,
                        job_id,
                    })
                })
                .collect::<Result<_, ApplicationError>>()?,
            edges: self
                .edges
                .into_iter()
                .map(|(from, to, condition)| {
                    Ok(WorkflowEdge {
                        from: ResourceName::parse(from.trim())
                            .map_err(super::service::invalid_name)?,
                        to: ResourceName::parse(to.trim()).map_err(super::service::invalid_name)?,
                        condition,
                    })
                })
                .collect::<Result<_, ApplicationError>>()?,
        };
        definition
            .validate()
            .map_err(ApplicationError::invalid_request)?;
        Ok(definition)
    }
}

impl Application {
    /// Authorize Namespace creation and Job reads; persist only a valid same-Namespace DAG.
    ///
    /// # Errors
    /// Returns validation, authorization, missing reference, or persistence failures.
    pub async fn create_workflow(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        input: WorkflowInput,
    ) -> Result<WorkflowRecord, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowCreate,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let definition = input.definition()?;
        self.validate_workflow_jobs(context, namespace_id, &definition, Capability::JobRead)
            .await?;
        Ok(self
            .store
            .write_workflow(namespace_id, None, &definition)
            .await?)
    }

    /// Authorize the immutable Workflow scope and atomically replace a matching revision.
    ///
    /// # Errors
    /// A stale revision conflicts; existing `WorkflowRun` snapshots remain untouched.
    pub async fn update_workflow(
        &self,
        context: &RequestContext,
        id: Uuid,
        revision: u64,
        input: WorkflowInput,
    ) -> Result<WorkflowRecord, ApplicationError> {
        let id = WorkflowId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowUpdate,
                &ResourceScope::Workflow(id),
            )
            .await?;
        let existing = self.store.get_workflow(id).await?;
        let definition = input.definition()?;
        self.validate_workflow_jobs(
            context,
            existing.namespace_id,
            &definition,
            Capability::JobRead,
        )
        .await?;
        Ok(self
            .store
            .write_workflow(existing.namespace_id, Some((id, revision)), &definition)
            .await?)
    }

    /// Read only after a resource decision and server-derived Namespace visibility.
    ///
    /// # Errors
    /// Hidden Workflows return not found, preserving resource-hiding semantics.
    pub async fn get_workflow(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<WorkflowRecord, ApplicationError> {
        let id = WorkflowId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowRead,
                &ResourceScope::Workflow(id),
            )
            .await?;
        let record = self.store.get_workflow(id).await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::WorkflowRead)
            .await?;
        visible_namespace(&visibility, record.namespace_id)?;
        Ok(record)
    }

    /// List visible definitions after scoped authorization and parent existence checks.
    ///
    /// # Errors
    /// Returns authorization, pagination, missing parent, or dependency failures.
    pub async fn list_workflows(
        &self,
        context: &RequestContext,
        namespace_id: Uuid,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<WorkflowRecord>, ApplicationError> {
        let namespace_id = NamespaceId::new(namespace_id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowRead,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::WorkflowRead)
            .await?;
        visible_namespace(&visibility, namespace_id)?;
        self.store.get_namespace(namespace_id).await?;
        Ok(self
            .store
            .list_workflows(
                namespace_id,
                &visibility,
                super::service::page_limit(limit)?,
                super::service::page_cursor(after)?,
            )
            .await?)
    }

    /// Delete an unused definition; PostgreSQL protects all invocation history.
    ///
    /// # Errors
    /// Returns denial, not found, in-use, or persistence failure without cascading Runs.
    pub async fn delete_workflow(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<(), ApplicationError> {
        let id = WorkflowId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowDelete,
                &ResourceScope::Workflow(id),
            )
            .await?;
        Ok(self.store.delete_workflow(id).await?)
    }

    /// Commit one authorized, idempotent invocation and create ordinary root Runs.
    ///
    /// Every referenced Job requires `JobExecute`; selections require `TargetSetUse`
    /// and each `TargetUse`; `RunCreate` is checked for the owning Namespace. Replays
    /// authorize historical references rather than a subsequently edited graph.
    ///
    /// # Errors
    /// Returns denial, invalid execution, revision/idempotency conflict, or dependency failure.
    pub async fn start_workflow(
        &self,
        context: &RequestContext,
        id: Uuid,
        request_id: Uuid,
        target: TargetSelection,
        inputs: serde_json::Value,
    ) -> Result<(WorkflowRunRecord, bool), ApplicationError> {
        let id = WorkflowId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowExecute,
                &ResourceScope::Workflow(id),
            )
            .await?;
        super::service::validate_input_object(&inputs)?;
        if let Some(existing) = self
            .store
            .workflow_run_for_request(request_id, &inputs)
            .await?
        {
            if existing.workflow.id != id || existing.target != target {
                return Err(ApplicationError::IdempotencyConflict);
            }
            self.authorize_workflow_replay(context, &existing).await?;
            return Ok((existing, false));
        }
        let workflow = self.store.get_workflow(id).await?;
        let (namespace_id, set_inputs, targets) = self.execution_targets(context, target).await?;
        if namespace_id != workflow.namespace_id {
            return Err(ApplicationError::invalid_request(
                "Workflow and execution target must belong to the same Namespace",
            ));
        }
        self.validate_workflow_jobs(
            context,
            namespace_id,
            &workflow.definition,
            Capability::JobExecute,
        )
        .await?;
        for node in &workflow.definition.nodes {
            let job = self.store.get_job(node.job_id).await?;
            for target in &targets {
                super::service::validate_rendered_execution(
                    &job.job,
                    set_inputs.as_ref(),
                    target,
                    &inputs,
                )?;
            }
        }
        self.authorizer
            .authorize(
                context,
                Capability::RunCreate,
                &ResourceScope::Namespace(namespace_id),
            )
            .await?;
        let (record, created) = self
            .store
            .start_workflow(&WorkflowLaunch {
                workflow_id: id,
                revision: workflow.revision,
                request_id,
                target,
                inputs,
                authorized_targets: targets.iter().map(Target::id).collect(),
            })
            .await?;
        // A concurrent launch may commit after the initial lookup. Authorize the
        // actual immutable intent returned by either replay path before exposing it.
        if !created {
            self.authorize_workflow_replay(context, &record).await?;
        }
        Ok((record, created))
    }

    /// Read invocation history through `WorkflowRunRead` and Namespace visibility.
    ///
    /// # Errors
    /// Hidden invocations return not found and disclose no child Run identifiers.
    pub async fn get_workflow_run(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<WorkflowRunRecord, ApplicationError> {
        let id = WorkflowRunId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowRunRead,
                &ResourceScope::WorkflowRun(id),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::WorkflowRunRead)
            .await?;
        Ok(self.store.get_workflow_run(id, &visibility).await?)
    }

    /// Page visible invocations of one Workflow, newest UUID first.
    ///
    /// # Errors
    /// Returns denial, hidden/missing parent, invalid pagination, or dependency failure.
    pub async fn list_workflow_runs(
        &self,
        context: &RequestContext,
        id: Uuid,
        limit: Option<u16>,
        before: Option<Uuid>,
    ) -> Result<Page<WorkflowRunRecord>, ApplicationError> {
        let id = WorkflowId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowRunRead,
                &ResourceScope::Workflow(id),
            )
            .await?;
        let workflow = self.store.get_workflow(id).await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::WorkflowRunRead)
            .await?;
        visible_namespace(&visibility, workflow.namespace_id)?;
        Ok(self
            .store
            .list_workflow_runs(id, &visibility, super::service::page_limit(limit)?, before)
            .await?)
    }

    /// Prevent pending nodes from starting while active ordinary Runs drain normally.
    ///
    /// # Errors
    /// Requires `WorkflowRunCancel` on this invocation; no process-kill authority is implied.
    pub async fn cancel_workflow_run(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<WorkflowRunRecord, ApplicationError> {
        let id = WorkflowRunId::new(id);
        self.authorizer
            .authorize(
                context,
                Capability::WorkflowRunCancel,
                &ResourceScope::WorkflowRun(id),
            )
            .await?;
        Ok(self.store.cancel_workflow_run(id).await?)
    }

    /// Authorize every historical Job and pinned Target before returning an idempotent replay.
    async fn authorize_workflow_replay(
        &self,
        context: &RequestContext,
        run: &WorkflowRunRecord,
    ) -> Result<(), ApplicationError> {
        for node in &run.nodes {
            self.authorizer
                .authorize(
                    context,
                    Capability::JobExecute,
                    &ResourceScope::Job(node.job_id),
                )
                .await?;
        }
        if let TargetSelection::TargetSet(id) = run.target {
            self.authorizer
                .authorize(
                    context,
                    Capability::TargetSetUse,
                    &ResourceScope::TargetSet(id),
                )
                .await?;
        }
        let targets = self.store.workflow_run_targets(run.id).await?;
        for id in targets {
            self.authorizer
                .authorize(context, Capability::TargetUse, &ResourceScope::Target(id))
                .await?;
        }
        self.authorizer
            .authorize(
                context,
                Capability::RunCreate,
                &ResourceScope::Namespace(run.workflow.namespace_id),
            )
            .await?;
        Ok(())
    }

    /// Check a verified caller's Job capability and authoritative Namespace membership.
    async fn validate_workflow_jobs(
        &self,
        context: &RequestContext,
        namespace_id: NamespaceId,
        definition: &WorkflowDefinition,
        capability: Capability,
    ) -> Result<(), ApplicationError> {
        for node in &definition.nodes {
            self.authorizer
                .authorize(context, capability, &ResourceScope::Job(node.job_id))
                .await?;
            let job = self.store.get_job(node.job_id).await?;
            if job.job.namespace_id() != namespace_id {
                return Err(ApplicationError::invalid_request(
                    "every Workflow Job must belong to its Namespace",
                ));
            }
        }
        Ok(())
    }
}

/// Authorize data exposure only inside server-derived visible Namespaces.
fn visible_namespace(
    visibility: &VisibilityScope,
    id: NamespaceId,
) -> Result<(), ApplicationError> {
    match visibility {
        VisibilityScope::All => Ok(()),
        VisibilityScope::Namespaces(ids) if ids.contains(&id) => Ok(()),
        _ => Err(ApplicationError::NotFound),
    }
}
