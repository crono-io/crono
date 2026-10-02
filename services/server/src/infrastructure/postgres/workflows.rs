//! PostgreSQL Workflow catalog, snapshots, and durable dependency evaluation.
//!
//! Flow Overview: launch locks the definition, checks the authorized revision
//! and membership, prepares immutable normal executions, and activates roots.
//! A Run status trigger commits completion events. The existing reconciler
//! locks only invocations with queued events, updates affected nodes, propagates
//! skips/readiness, and commits new ordinary Runs through the shared Run helper.
//! Invocation row locks serialize cancellation and multiple orchestrators; event
//! deletion and downstream creation share a transaction, so crashes replay safely.

use super::{
    PostgresStore, create_normal_run, execution_snapshot_with_inputs, json_inputs_equal,
    load_selection_executions, selection_ids, store_error, target_selection,
};
use crate::{
    application::{
        Page, StoreError, VisibilityScope, WorkflowChildRun, WorkflowLaunch, WorkflowNodeRunRecord,
        WorkflowRecord, WorkflowRunRecord,
    },
    domain::{
        DependencyCondition, DependencyDecision, JobId, NamespaceId, NamespaceName, ResourceName,
        RunId, TargetId, WorkflowDefinition, WorkflowEdge, WorkflowId, WorkflowNode,
        WorkflowNodeId, WorkflowNodeRunId, WorkflowNodeState, WorkflowRunId, WorkflowState,
        dependency_decision,
    },
};
use crono_api::ExecutionTrigger;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredNode {
    id: Uuid,
    name: String,
    job_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEdge {
    from: String,
    to: String,
    condition: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredWorkflow {
    id: Uuid,
    namespace_id: Uuid,
    namespace: String,
    name: String,
    description: Option<String>,
    revision: u64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    nodes: Vec<StoredNode>,
    edges: Vec<StoredEdge>,
}

#[derive(sqlx::FromRow)]
struct WorkflowRow {
    id: Uuid,
    namespace_id: Uuid,
    namespace: String,
    name: String,
    description: Option<String>,
    revision: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}
#[derive(sqlx::FromRow)]
struct NodeRow {
    id: Uuid,
    workflow_node_id: Uuid,
    name: String,
    job_id: Uuid,
    state: String,
    started_at: Option<OffsetDateTime>,
    finished_at: Option<OffsetDateTime>,
}
#[derive(sqlx::FromRow)]
struct InvocationRow {
    id: Uuid,
    request_id: Uuid,
    definition_snapshot: serde_json::Value,
    target_id: Option<Uuid>,
    target_set_id: Option<Uuid>,
    inputs: serde_json::Value,
    state: String,
    cancellation_requested: bool,
    created_at: OffsetDateTime,
    started_at: Option<OffsetDateTime>,
    finished_at: Option<OffsetDateTime>,
}
#[derive(sqlx::FromRow)]
struct PreparedRow {
    target_id: Uuid,
    prospective_run_id: Uuid,
    queue_id: Uuid,
    max_attempts: i32,
    execution_snapshot: serde_json::Value,
}

impl StoredWorkflow {
    /// Convert trusted persisted graph data, rejecting corruption instead of guessing semantics.
    fn record(self) -> Result<WorkflowRecord, StoreError> {
        let node_ids = self
            .nodes
            .iter()
            .map(|node| (node.name.clone(), WorkflowNodeId::new(node.id)))
            .collect();
        let definition = WorkflowDefinition {
            name: resource_name(&self.name)?,
            description: self.description,
            nodes: self
                .nodes
                .into_iter()
                .map(|node| {
                    Ok(WorkflowNode {
                        name: resource_name(&node.name)?,
                        job_id: JobId::new(node.job_id),
                    })
                })
                .collect::<Result<_, StoreError>>()?,
            edges: self
                .edges
                .into_iter()
                .map(|edge| {
                    Ok(WorkflowEdge {
                        from: resource_name(&edge.from)?,
                        to: resource_name(&edge.to)?,
                        condition: condition(&edge.condition)?,
                    })
                })
                .collect::<Result<_, StoreError>>()?,
        };
        definition.validate().map_err(|_| StoreError::Internal)?;
        Ok(WorkflowRecord {
            id: WorkflowId::new(self.id),
            namespace_id: NamespaceId::new(self.namespace_id),
            namespace: NamespaceName::parse(&self.namespace).map_err(|_| StoreError::Internal)?,
            revision: self.revision,
            definition,
            node_ids,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Read a coherent catalog revision while preventing concurrent graph replacement.
async fn catalog(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<StoredWorkflow, StoreError> {
    let row = sqlx::query_as::<_, WorkflowRow>(
        "SELECT w.id, w.namespace_id, ns.name AS namespace, w.name, w.description, w.revision,
                w.created_at, w.updated_at FROM crono.workflows w JOIN crono.namespaces ns ON ns.id = w.namespace_id
          WHERE w.id = $1 FOR SHARE OF w")
        .bind(id).fetch_optional(&mut **transaction).await.map_err(store_error)?.ok_or(StoreError::NotFound)?;
    let nodes = sqlx::query_as::<_, (Uuid, String, Uuid)>(
        "SELECT id, name, job_id FROM crono.workflow_nodes WHERE workflow_id = $1 ORDER BY name",
    )
    .bind(id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(store_error)?
    .into_iter()
    .map(|(id, name, job_id)| StoredNode { id, name, job_id })
    .collect();
    let edges = sqlx::query_as::<_, (String, String, String)>(
        "SELECT a.name, b.name, e.condition FROM crono.workflow_edges e
         JOIN crono.workflow_nodes a ON a.id = e.from_node_id JOIN crono.workflow_nodes b ON b.id = e.to_node_id
         WHERE e.workflow_id = $1 ORDER BY a.name, b.name")
        .bind(id).fetch_all(&mut **transaction).await.map_err(store_error)?.into_iter()
        .map(|(from, to, condition)| StoredEdge { from, to, condition }).collect();
    Ok(StoredWorkflow {
        id: row.id,
        namespace_id: row.namespace_id,
        namespace: row.namespace,
        name: row.name,
        description: row.description,
        revision: u64::try_from(row.revision).map_err(|_| StoreError::Internal)?,
        created_at: row.created_at,
        updated_at: row.updated_at,
        nodes,
        edges,
    })
}

/// Replace all definition rows under one optimistic catalog lock; history has no FK to these nodes.
pub(super) async fn write(
    pool: &PgPool,
    namespace_id: NamespaceId,
    existing: Option<(WorkflowId, u64)>,
    definition: &WorkflowDefinition,
) -> Result<WorkflowRecord, StoreError> {
    definition.validate().map_err(|_| StoreError::InvalidData)?;
    let mut transaction = pool.begin().await.map_err(store_error)?;
    let id = if let Some((id, revision)) = existing {
        let updated = sqlx::query_scalar::<_, Uuid>(
            "UPDATE crono.workflows SET name = $3, description = $4, revision = revision + 1, updated_at = statement_timestamp()
             WHERE id = $1 AND namespace_id = $5 AND revision = $2 RETURNING id")
            .bind(id.get()).bind(i64::try_from(revision).map_err(|_| StoreError::StaleRevision)?)
            .bind(definition.name.as_str()).bind(&definition.description).bind(namespace_id.get())
            .fetch_optional(&mut *transaction).await.map_err(store_error)?.ok_or(StoreError::StaleRevision)?;
        sqlx::query("DELETE FROM crono.workflow_nodes WHERE workflow_id = $1")
            .bind(updated)
            .execute(&mut *transaction)
            .await
            .map_err(store_error)?;
        updated
    } else {
        sqlx::query_scalar::<_, Uuid>("INSERT INTO crono.workflows (namespace_id, name, description) VALUES ($1, $2, $3) RETURNING id")
            .bind(namespace_id.get()).bind(definition.name.as_str()).bind(&definition.description)
            .fetch_one(&mut *transaction).await.map_err(store_error)?
    };
    let mut ids = BTreeMap::new();
    for node in &definition.nodes {
        let job_namespace = sqlx::query_scalar::<_, Uuid>(
            "SELECT namespace_id FROM crono.jobs WHERE id = $1 FOR SHARE",
        )
        .bind(node.job_id.get())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(store_error)?
        .ok_or(StoreError::NotFound)?;
        if job_namespace != namespace_id.get() {
            return Err(StoreError::InvalidData);
        }
        let node_id = sqlx::query_scalar::<_, Uuid>("INSERT INTO crono.workflow_nodes (workflow_id, name, job_id) VALUES ($1, $2, $3) RETURNING id")
            .bind(id).bind(node.name.as_str()).bind(node.job_id.get()).fetch_one(&mut *transaction).await.map_err(store_error)?;
        ids.insert(node.name.as_str(), node_id);
    }
    for edge in &definition.edges {
        sqlx::query("INSERT INTO crono.workflow_edges (workflow_id, from_node_id, to_node_id, condition) VALUES ($1, $2, $3, $4)")
            .bind(id).bind(ids.get(edge.from.as_str()).ok_or(StoreError::InvalidData)?)
            .bind(ids.get(edge.to.as_str()).ok_or(StoreError::InvalidData)?).bind(condition_name(edge.condition))
            .execute(&mut *transaction).await.map_err(store_error)?;
    }
    let record = catalog(&mut transaction, id).await?.record()?;
    transaction.commit().await.map_err(store_error)?;
    Ok(record)
}

pub(super) async fn get(pool: &PgPool, id: WorkflowId) -> Result<WorkflowRecord, StoreError> {
    let mut transaction = pool.begin().await.map_err(store_error)?;
    let record = catalog(&mut transaction, id.get()).await?.record()?;
    transaction.commit().await.map_err(store_error)?;
    Ok(record)
}

pub(super) async fn list(
    pool: &PgPool,
    namespace_id: NamespaceId,
    visibility: &VisibilityScope,
    limit: u16,
    after: Option<&str>,
) -> Result<Page<WorkflowRecord>, StoreError> {
    if !visible(visibility, namespace_id.get()) {
        return Ok(super::empty_page());
    }
    let mut transaction = pool.begin().await.map_err(store_error)?;
    let mut ids = sqlx::query_as::<_, (Uuid, String)>("SELECT id, name FROM crono.workflows WHERE namespace_id = $1 AND ($2::text IS NULL OR name > $2) ORDER BY name LIMIT $3 FOR SHARE")
        .bind(namespace_id.get()).bind(after).bind(i64::from(limit) + 1).fetch_all(&mut *transaction).await.map_err(store_error)?;
    let more = ids.len() > usize::from(limit);
    if more {
        ids.pop();
    }
    let cursor = more
        .then(|| ids.last().map(|(_, name)| name.clone()))
        .flatten();
    let mut items = Vec::with_capacity(ids.len());
    for (id, _) in ids {
        items.push(catalog(&mut transaction, id).await?.record()?);
    }
    transaction.commit().await.map_err(store_error)?;
    Ok(Page {
        items,
        next_cursor: cursor,
    })
}

pub(super) async fn delete(pool: &PgPool, id: WorkflowId) -> Result<(), StoreError> {
    let result = sqlx::query("DELETE FROM crono.workflows WHERE id = $1")
        .bind(id.get())
        .execute(pool)
        .await
        .map_err(store_error)?;
    if result.rows_affected() == 0 {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

/// Pin every rendered child execution before activating roots; no worker or broker sees graph data.
pub(super) async fn start(
    pool: &PgPool,
    launch: &WorkflowLaunch,
) -> Result<(WorkflowRunRecord, bool), StoreError> {
    let mut transaction = pool.begin().await.map_err(store_error)?;
    // Serialize this request before any definition read, so concurrent replays cannot duplicate roots.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(launch.request_id.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
    let existing =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM crono.workflow_runs WHERE request_id = $1")
            .bind(launch.request_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(store_error)?;
    if let Some(id) = existing {
        let record = invocation(&mut transaction, id, &VisibilityScope::All).await?;
        if record.workflow.id != launch.workflow_id
            || record.target != launch.target
            || !json_inputs_equal(&mut *transaction, &record.inputs, &launch.inputs).await?
        {
            return Err(StoreError::IdempotencyConflict);
        }
        transaction.commit().await.map_err(store_error)?;
        return Ok((record, false));
    }
    let definition = catalog(&mut transaction, launch.workflow_id.get()).await?;
    if definition.revision != launch.revision {
        return Err(StoreError::StaleRevision);
    }
    let id = Uuid::now_v7();
    let (target_id, target_set_id) = selection_ids(launch.target);
    sqlx::query("INSERT INTO crono.workflow_runs (id, request_id, workflow_id, namespace_id, definition_snapshot, target_id, target_set_id, inputs, started_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,statement_timestamp())")
        .bind(id).bind(launch.request_id).bind(launch.workflow_id.get()).bind(definition.namespace_id)
        .bind(serde_json::to_value(&definition).map_err(|_| StoreError::Internal)?).bind(target_id).bind(target_set_id).bind(&launch.inputs)
        .execute(&mut *transaction).await.map_err(store_error)?;
    let job_ids: Vec<Uuid> = definition.nodes.iter().map(|node| node.job_id).collect();
    sqlx::query("SELECT id FROM crono.jobs WHERE id = ANY($1::uuid[]) ORDER BY id FOR SHARE")
        .bind(job_ids)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
    let target_ids: Vec<Uuid> = launch
        .authorized_targets
        .iter()
        .map(|id| id.get())
        .collect();
    sqlx::query("SELECT id FROM crono.targets WHERE id = ANY($1::uuid[]) ORDER BY id FOR SHARE")
        .bind(target_ids)
        .execute(&mut *transaction)
        .await
        .map_err(store_error)?;
    let node_ids = prepare_nodes(&mut transaction, id, &definition, launch).await?;
    for edge in &definition.edges {
        sqlx::query("INSERT INTO crono.workflow_run_edges (workflow_run_id, from_node_run_id, to_node_run_id, condition) VALUES ($1,$2,$3,$4)")
            .bind(id).bind(node_ids.get(edge.from.as_str()).ok_or(StoreError::Internal)?)
            .bind(node_ids.get(edge.to.as_str()).ok_or(StoreError::Internal)?).bind(&edge.condition)
            .execute(&mut *transaction).await.map_err(store_error)?;
    }
    for node in &definition.nodes {
        if !definition.edges.iter().any(|edge| edge.to == node.name) {
            activate(
                &mut transaction,
                *node_ids
                    .get(node.name.as_str())
                    .ok_or(StoreError::Internal)?,
            )
            .await?;
        }
    }
    let record = invocation(&mut transaction, id, &VisibilityScope::All).await?;
    transaction.commit().await.map_err(store_error)?;
    Ok((record, true))
}

/// Bound and pin prepared normal executions for every authorized node/Target pair.
async fn prepare_nodes(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    definition: &StoredWorkflow,
    launch: &WorkflowLaunch,
) -> Result<BTreeMap<String, Uuid>, StoreError> {
    let mut node_ids = BTreeMap::new();
    let mut prepared_bytes = 0_usize;
    let mut prepared_count = 0_usize;
    let authorized: BTreeSet<Uuid> = launch
        .authorized_targets
        .iter()
        .map(|id| id.get())
        .collect();
    for node in &definition.nodes {
        let node_id = Uuid::now_v7();
        node_ids.insert(node.name.clone(), node_id);
        sqlx::query("INSERT INTO crono.workflow_node_runs (id, workflow_run_id, workflow_node_id, name, job_id) VALUES ($1,$2,$3,$4,$5)")
            .bind(node_id).bind(id).bind(node.id).bind(&node.name).bind(node.job_id).execute(&mut **transaction).await.map_err(store_error)?;
        let (target_id, target_set_id) = selection_ids(launch.target);
        sqlx::query("INSERT INTO crono.run_requests (request_id, namespace_id, job_id, target_id, target_set_id, inputs) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(node_id).bind(definition.namespace_id).bind(node.job_id).bind(target_id).bind(target_set_id).bind(&launch.inputs)
            .execute(&mut **transaction).await.map_err(store_error)?;
        let (executions, set_inputs) =
            load_selection_executions(transaction, node.job_id, launch.target).await?;
        let actual: BTreeSet<Uuid> = executions
            .iter()
            .map(|execution| execution.target_id)
            .collect();
        if actual != authorized {
            return Err(StoreError::StaleRevision);
        }
        prepared_count += executions.len();
        if prepared_count > 4096 {
            return Err(StoreError::WorkflowLaunchTooLarge);
        }
        for execution in executions {
            if execution.namespace_id != definition.namespace_id {
                return Err(StoreError::InvalidData);
            }
            let run_id = Uuid::now_v7();
            let snapshot = execution_snapshot_with_inputs(
                run_id,
                &execution,
                set_inputs.as_ref(),
                &launch.inputs,
                ExecutionTrigger::Manual,
                None,
            )
            .map_err(|_| StoreError::InvalidData)?;
            prepared_bytes += serde_json::to_vec(&snapshot)
                .map_err(|_| StoreError::Internal)?
                .len();
            if prepared_bytes > 8 * 1024 * 1024 {
                return Err(StoreError::WorkflowLaunchTooLarge);
            }
            sqlx::query("INSERT INTO crono.workflow_node_executions (node_run_id, target_id, prospective_run_id, queue_id, max_attempts, execution_snapshot) VALUES ($1,$2,$3,$4,$5,$6)")
                .bind(node_id).bind(execution.target_id).bind(run_id).bind(execution.queue_id).bind(execution.max_attempts).bind(snapshot)
                .execute(&mut **transaction).await.map_err(store_error)?;
        }
    }
    Ok(node_ids)
}

/// Create a node's one normal invocation using the shared Run/Attempt/outbox path.
async fn activate(
    transaction: &mut Transaction<'_, Postgres>,
    node_id: Uuid,
) -> Result<(), StoreError> {
    let job_id = sqlx::query_scalar::<_, Uuid>(
        "UPDATE crono.workflow_node_runs n SET state = 'running', started_at = statement_timestamp()
          FROM crono.workflow_runs w WHERE n.id = $1 AND n.workflow_run_id = w.id
          AND n.state IN ('pending','ready') AND NOT w.cancellation_requested AND w.state = 'running'
          RETURNING n.job_id")
        .bind(node_id).fetch_optional(&mut **transaction).await.map_err(store_error)?;
    let Some(job_id) = job_id else {
        return Ok(());
    };
    let executions = sqlx::query_as::<_, PreparedRow>("SELECT target_id, prospective_run_id, queue_id, max_attempts, execution_snapshot FROM crono.workflow_node_executions WHERE node_run_id = $1 ORDER BY target_id")
        .bind(node_id).fetch_all(&mut **transaction).await.map_err(store_error)?;
    for execution in executions {
        create_normal_run(
            transaction,
            &super::PreparedRun {
                id: execution.prospective_run_id,
                request_id: node_id,
                job_id,
                target_id: execution.target_id,
                queue_id: execution.queue_id,
                max_attempts: execution.max_attempts,
                snapshot: execution.execution_snapshot,
            },
        )
        .await?;
        sqlx::query("UPDATE crono.workflow_node_executions SET run_id = prospective_run_id WHERE node_run_id = $1 AND target_id = $2 AND run_id IS NULL")
            .bind(node_id).bind(execution.target_id).execute(&mut **transaction).await.map_err(store_error)?;
    }
    Ok(())
}

/// Query only invocations with durable completion events, under cross-server row locks.
pub(super) async fn reconcile(pool: &PgPool, limit: u16) -> Result<u64, StoreError> {
    let mut count = 0;
    for _ in 0..limit {
        let mut transaction = pool.begin().await.map_err(store_error)?;
        let id = sqlx::query_scalar::<_, Uuid>("SELECT w.id FROM (SELECT DISTINCT workflow_run_id FROM crono.workflow_completion_events ORDER BY workflow_run_id LIMIT 100) pending JOIN crono.workflow_runs w ON w.id = pending.workflow_run_id ORDER BY w.id LIMIT 1 FOR UPDATE OF w SKIP LOCKED")
            .fetch_optional(&mut *transaction).await.map_err(store_error)?;
        let Some(id) = id else {
            break;
        };
        let events = sqlx::query_as::<_, (Uuid, Uuid)>("SELECT run_id, node_run_id FROM crono.workflow_completion_events WHERE workflow_run_id = $1 ORDER BY node_run_id, run_id")
            .bind(id).fetch_all(&mut *transaction).await.map_err(store_error)?;
        let affected: BTreeSet<Uuid> = events.iter().map(|(_, node_id)| *node_id).collect();
        let mut downstream = VecDeque::new();
        for node_id in affected {
            if complete_node(&mut transaction, node_id).await? {
                downstream.extend(outgoing(&mut transaction, node_id).await?);
            }
        }
        propagate(&mut transaction, &mut downstream).await?;
        finish(&mut transaction, id).await?;
        let event_ids: Vec<Uuid> = events.into_iter().map(|(run_id, _)| run_id).collect();
        sqlx::query("DELETE FROM crono.workflow_completion_events WHERE run_id = ANY($1::uuid[])")
            .bind(event_ids)
            .execute(&mut *transaction)
            .await
            .map_err(store_error)?;
        transaction.commit().await.map_err(store_error)?;
        count += 1;
    }
    Ok(count)
}

/// Aggregate only this node's children; retries are nonterminal and cannot unlock edges.
async fn complete_node(
    transaction: &mut Transaction<'_, Postgres>,
    node_id: Uuid,
) -> Result<bool, StoreError> {
    let statuses = sqlx::query_scalar::<_, String>("SELECT r.status FROM crono.workflow_node_executions e JOIN crono.runs r ON r.id = e.run_id WHERE e.node_run_id = $1")
        .bind(node_id).fetch_all(&mut **transaction).await.map_err(store_error)?;
    if statuses.is_empty()
        || statuses.iter().any(|status| {
            matches!(
                status.as_str(),
                "pending_dispatch" | "queued" | "running" | "retry_wait"
            )
        })
    {
        return Ok(false);
    }
    let state = if statuses.iter().any(|status| status == "unknown") {
        "unknown"
    } else if statuses
        .iter()
        .any(|status| matches!(status.as_str(), "failed" | "dead"))
    {
        "failed"
    } else if statuses.iter().any(|status| status == "cancelled") {
        "cancelled"
    } else if statuses.iter().any(|status| status == "skipped") {
        "skipped"
    } else {
        "succeeded"
    };
    let changed = sqlx::query("UPDATE crono.workflow_node_runs SET state = $2, finished_at = statement_timestamp() WHERE id = $1 AND state = 'running'")
        .bind(node_id).bind(state).execute(&mut **transaction).await.map_err(store_error)?;
    Ok(changed.rows_affected() == 1)
}

async fn outgoing(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Vec<Uuid>, StoreError> {
    sqlx::query_scalar("SELECT to_node_run_id FROM crono.workflow_run_edges WHERE from_node_run_id = $1 ORDER BY to_node_run_id")
        .bind(id).fetch_all(&mut **transaction).await.map_err(store_error)
}

/// Reevaluate only affected pending nodes and recursively propagate terminal skips.
async fn propagate(
    transaction: &mut Transaction<'_, Postgres>,
    downstream: &mut VecDeque<Uuid>,
) -> Result<(), StoreError> {
    while let Some(id) = downstream.pop_front() {
        let active = sqlx::query_scalar::<_, bool>("SELECT n.state = 'pending' AND NOT w.cancellation_requested FROM crono.workflow_node_runs n JOIN crono.workflow_runs w ON w.id = n.workflow_run_id WHERE n.id = $1")
            .bind(id).fetch_one(&mut **transaction).await.map_err(store_error)?;
        if !active {
            continue;
        }
        let incoming = sqlx::query_as::<_, (String, String)>("SELECT e.condition, n.state FROM crono.workflow_run_edges e JOIN crono.workflow_node_runs n ON n.id = e.from_node_run_id WHERE e.to_node_run_id = $1 ORDER BY e.from_node_run_id")
            .bind(id).fetch_all(&mut **transaction).await.map_err(store_error)?;
        let values = incoming
            .iter()
            .map(|(edge, state)| Ok((condition(edge)?, node_state(state)?)))
            .collect::<Result<Vec<_>, StoreError>>()?;
        match dependency_decision(values) {
            DependencyDecision::Waiting => {}
            DependencyDecision::Ready => activate(transaction, id).await?,
            DependencyDecision::Skipped => {
                sqlx::query("UPDATE crono.workflow_node_runs SET state = 'skipped', finished_at = statement_timestamp() WHERE id = $1 AND state = 'pending'")
                    .bind(id).execute(&mut **transaction).await.map_err(store_error)?;
                downstream.extend(outgoing(transaction, id).await?);
            }
        }
    }
    Ok(())
}

/// Derive the final result only after every node terminates, retaining failures after rollback paths.
async fn finish(transaction: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<(), StoreError> {
    sqlx::query("UPDATE crono.workflow_runs w SET state = CASE WHEN cancellation_requested THEN 'cancelled' WHEN EXISTS (SELECT 1 FROM crono.workflow_node_runs n WHERE n.workflow_run_id = w.id AND n.state IN ('failed','unknown','cancelled')) THEN 'failed' ELSE 'succeeded' END, finished_at = statement_timestamp() WHERE w.id = $1 AND w.state = 'running' AND NOT EXISTS (SELECT 1 FROM crono.workflow_node_runs n WHERE n.workflow_run_id = w.id AND n.state IN ('pending','ready','running'))")
        .bind(id).execute(&mut **transaction).await.map_err(store_error)?;
    Ok(())
}

pub(super) async fn cancel(
    pool: &PgPool,
    id: WorkflowRunId,
) -> Result<WorkflowRunRecord, StoreError> {
    let mut transaction = pool.begin().await.map_err(store_error)?;
    let state = sqlx::query_scalar::<_, String>(
        "SELECT state FROM crono.workflow_runs WHERE id = $1 FOR UPDATE",
    )
    .bind(id.get())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(store_error)?
    .ok_or(StoreError::NotFound)?;
    if state == "running" {
        sqlx::query("UPDATE crono.workflow_runs SET cancellation_requested = true WHERE id = $1")
            .bind(id.get())
            .execute(&mut *transaction)
            .await
            .map_err(store_error)?;
        sqlx::query("UPDATE crono.workflow_node_runs SET state = 'cancelled', finished_at = statement_timestamp() WHERE workflow_run_id = $1 AND state IN ('pending','ready')")
            .bind(id.get()).execute(&mut *transaction).await.map_err(store_error)?;
        finish(&mut transaction, id.get()).await?;
    }
    let record = invocation(&mut transaction, id.get(), &VisibilityScope::All).await?;
    transaction.commit().await.map_err(store_error)?;
    Ok(record)
}

/// Read a graph and node history coherently; orchestrators hold this invocation's exclusive lock.
async fn invocation(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    visibility: &VisibilityScope,
) -> Result<WorkflowRunRecord, StoreError> {
    let row = sqlx::query_as::<_, InvocationRow>("SELECT id, request_id, definition_snapshot, target_id, target_set_id, inputs, state, cancellation_requested, created_at, started_at, finished_at FROM crono.workflow_runs WHERE id = $1 FOR SHARE")
        .bind(id).fetch_optional(&mut **transaction).await.map_err(store_error)?.ok_or(StoreError::NotFound)?;
    let stored: StoredWorkflow =
        serde_json::from_value(row.definition_snapshot).map_err(|_| StoreError::Internal)?;
    if !visible(visibility, stored.namespace_id) {
        return Err(StoreError::NotFound);
    }
    let records = sqlx::query_as::<_, NodeRow>("SELECT id, workflow_node_id, name, job_id, state, started_at, finished_at FROM crono.workflow_node_runs WHERE workflow_run_id = $1 ORDER BY name")
        .bind(id).fetch_all(&mut **transaction).await.map_err(store_error)?;
    let mut nodes = Vec::with_capacity(records.len());
    for node in records {
        let children = sqlx::query_as::<_, (Uuid, Uuid)>("SELECT target_id, run_id FROM crono.workflow_node_executions WHERE node_run_id = $1 AND run_id IS NOT NULL ORDER BY target_id")
            .bind(node.id).fetch_all(&mut **transaction).await.map_err(store_error)?.into_iter()
            .map(|(target_id, run_id)| WorkflowChildRun { target_id: TargetId::new(target_id), run_id: RunId::new(run_id) }).collect();
        nodes.push(WorkflowNodeRunRecord {
            id: WorkflowNodeRunId::new(node.id),
            workflow_node_id: WorkflowNodeId::new(node.workflow_node_id),
            name: resource_name(&node.name)?,
            job_id: JobId::new(node.job_id),
            state: node_state(&node.state)?,
            runs: children,
            started_at: node.started_at,
            finished_at: node.finished_at,
        });
    }
    Ok(WorkflowRunRecord {
        id: WorkflowRunId::new(row.id),
        request_id: row.request_id,
        workflow: stored.record()?,
        target: target_selection(row.target_id, row.target_set_id)?,
        inputs: row.inputs,
        state: workflow_state(&row.state)?,
        cancellation_requested: row.cancellation_requested,
        nodes,
        created_at: row.created_at,
        started_at: row.started_at,
        finished_at: row.finished_at,
    })
}

pub(super) async fn get_run(
    pool: &PgPool,
    id: WorkflowRunId,
    visibility: &VisibilityScope,
) -> Result<WorkflowRunRecord, StoreError> {
    let mut transaction = pool.begin().await.map_err(store_error)?;
    let record = invocation(&mut transaction, id.get(), visibility).await?;
    transaction.commit().await.map_err(store_error)?;
    Ok(record)
}

pub(super) async fn for_request(
    pool: &PgPool,
    request_id: Uuid,
    inputs: &serde_json::Value,
) -> Result<Option<WorkflowRunRecord>, StoreError> {
    let id =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM crono.workflow_runs WHERE request_id = $1")
            .bind(request_id)
            .fetch_optional(pool)
            .await
            .map_err(store_error)?;
    if let Some(id) = id {
        let record = get_run(pool, WorkflowRunId::new(id), &VisibilityScope::All).await?;
        if !json_inputs_equal(pool, &record.inputs, inputs).await? {
            return Err(StoreError::IdempotencyConflict);
        }
        Ok(Some(record))
    } else {
        Ok(None)
    }
}

pub(super) async fn targets(pool: &PgPool, id: WorkflowRunId) -> Result<Vec<TargetId>, StoreError> {
    Ok(sqlx::query_scalar::<_, Uuid>("SELECT DISTINCT e.target_id FROM crono.workflow_node_executions e JOIN crono.workflow_node_runs n ON n.id = e.node_run_id WHERE n.workflow_run_id = $1 ORDER BY e.target_id")
        .bind(id.get()).fetch_all(pool).await.map_err(store_error)?.into_iter().map(TargetId::new).collect())
}

pub(super) async fn list_runs(
    pool: &PgPool,
    id: WorkflowId,
    visibility: &VisibilityScope,
    limit: u16,
    before: Option<Uuid>,
) -> Result<Page<WorkflowRunRecord>, StoreError> {
    let mut transaction = pool.begin().await.map_err(store_error)?;
    let restrict = !matches!(visibility, VisibilityScope::All);
    let namespaces = PostgresStore::namespace_ids(visibility);
    let mut ids = sqlx::query_scalar::<_, Uuid>("SELECT id FROM crono.workflow_runs WHERE workflow_id = $1 AND ($2::uuid IS NULL OR id < $2) AND (NOT $3 OR namespace_id = ANY($4::uuid[])) ORDER BY id DESC LIMIT $5 FOR SHARE")
        .bind(id.get()).bind(before).bind(restrict).bind(namespaces).bind(i64::from(limit) + 1).fetch_all(&mut *transaction).await.map_err(store_error)?;
    let more = ids.len() > usize::from(limit);
    if more {
        ids.pop();
    }
    let cursor = if more {
        ids.last().map(ToString::to_string)
    } else {
        None
    };
    let mut items = Vec::with_capacity(ids.len());
    for id in ids {
        items.push(invocation(&mut transaction, id, visibility).await?);
    }
    transaction.commit().await.map_err(store_error)?;
    Ok(Page {
        items,
        next_cursor: cursor,
    })
}

fn resource_name(value: &str) -> Result<ResourceName, StoreError> {
    ResourceName::parse(value).map_err(|_| StoreError::Internal)
}
fn visible(visibility: &VisibilityScope, id: Uuid) -> bool {
    match visibility {
        VisibilityScope::All => true,
        VisibilityScope::Namespaces(ids) => ids.contains(&NamespaceId::new(id)),
        VisibilityScope::None => false,
    }
}
pub(super) const fn condition_name(value: DependencyCondition) -> &'static str {
    match value {
        DependencyCondition::Success => "success",
        DependencyCondition::Failure => "failure",
        DependencyCondition::Always => "always",
    }
}
fn condition(value: &str) -> Result<DependencyCondition, StoreError> {
    match value {
        "success" => Ok(DependencyCondition::Success),
        "failure" => Ok(DependencyCondition::Failure),
        "always" => Ok(DependencyCondition::Always),
        _ => Err(StoreError::Internal),
    }
}
fn node_state(value: &str) -> Result<WorkflowNodeState, StoreError> {
    match value {
        "pending" => Ok(WorkflowNodeState::Pending),
        "ready" => Ok(WorkflowNodeState::Ready),
        "running" => Ok(WorkflowNodeState::Running),
        "succeeded" => Ok(WorkflowNodeState::Succeeded),
        "failed" => Ok(WorkflowNodeState::Failed),
        "skipped" => Ok(WorkflowNodeState::Skipped),
        "cancelled" => Ok(WorkflowNodeState::Cancelled),
        "unknown" => Ok(WorkflowNodeState::Unknown),
        _ => Err(StoreError::Internal),
    }
}
fn workflow_state(value: &str) -> Result<WorkflowState, StoreError> {
    match value {
        "pending" => Ok(WorkflowState::Pending),
        "running" => Ok(WorkflowState::Running),
        "succeeded" => Ok(WorkflowState::Succeeded),
        "failed" => Ok(WorkflowState::Failed),
        "cancelled" => Ok(WorkflowState::Cancelled),
        _ => Err(StoreError::Internal),
    }
}
