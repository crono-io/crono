//! Durable DAG integration tests use real PostgreSQL and ordinary worker-control transitions.
//!
//! Fixtures serialize within this binary because reconciliation deliberately
//! processes global event queues. Each test removes its own Namespace and history;
//! concurrency tests independently race stores inside that isolated fixture.

mod support;

#[path = "workflows/input_normalization.rs"]
mod input_normalization;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use crono_api::{ClaimRequest, CompletionRequest};
use crono_server::{
    application::{
        Application, ApplicationError, AuthorizationError, Authorizer, Capability,
        ControlPlaneStore, JobDefinition, PermitAllAuthorizer, Principal, PrincipalKind,
        RequestContext, ResourceScope, StoreError, TargetDefinition, VisibilityScope,
        WorkflowInput, WorkflowLaunch, WorkflowNodeRunRecord, WorkflowRecord, WorkflowRunRecord,
    },
    domain::{
        DependencyCondition as Condition, ExecutorKind, JobId, NamespaceId, NamespaceName,
        QueueName, ResourceName, RunId, TargetId, TargetSelection, WorkflowNodeState as NodeState,
        WorkflowState,
    },
    infrastructure::{DatabasePoolConfig, PostgresStore},
};
use sqlx::PgPool;
use std::{collections::BTreeSet, sync::Arc};
use tokio::sync::{Mutex, MutexGuard};
use tower::ServiceExt;
use uuid::Uuid;

static FIXTURE_LOCK: Mutex<()> = Mutex::const_new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    url: String,
    store: PostgresStore,
    pool: PgPool,
    app: Application,
    context: RequestContext,
    namespace_id: NamespaceId,
    jobs: Vec<(String, JobId)>,
    target: TargetId,
}

impl Fixture {
    async fn new(names: &[&str]) -> Result<Option<Self>> {
        let Some(url) = support::database_url()? else {
            return Ok(None);
        };
        let guard = FIXTURE_LOCK.lock().await;
        let store = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
        let pool = PgPool::connect(&url).await?;
        let namespace = store
            .create_namespace(&NamespaceName::parse(&format!(
                "workflow-{}",
                Uuid::now_v7().simple()
            ))?)
            .await?;
        let queue = store
            .get_queue_by_name(&QueueName::parse("default")?)
            .await?;
        let mut jobs = Vec::new();
        for name in names {
            let job = store
                .create_job(
                    namespace.id(),
                    &ResourceName::parse(name)?,
                    &JobDefinition {
                        executor: ExecutorKind::Noop,
                        queue_id: queue.id(),
                        executable: None,
                        shell_command: None,
                        arguments: vec!["{{value}}".to_owned()],
                        inputs: serde_json::json!({"value":"job"}),
                        idempotent: false,
                        dry_run: false,
                        max_attempts: 1,
                        retry_initial_seconds: 1,
                        retry_max_seconds: 1,
                        retry_multiplier: 1.0,
                        retry_jitter: 0.0,
                    },
                )
                .await?;
            jobs.push(((*name).to_owned(), job.job.id()));
        }
        let target = store
            .create_target(
                namespace.id(),
                &ResourceName::parse("target")?,
                &TargetDefinition {
                    arguments: Vec::new(),
                    inputs: serde_json::json!({"value":"target"}),
                },
            )
            .await?;
        Ok(Some(Self {
            _guard: guard,
            url,
            app: Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer)),
            store,
            pool,
            namespace_id: namespace.id(),
            jobs,
            target: target.target.id(),
            context: RequestContext::new(
                Uuid::now_v7(),
                Principal::new("test/service".to_owned(), PrincipalKind::Service),
            ),
        }))
    }

    fn input(&self, edges: &[(&str, &str, Condition)]) -> WorkflowInput {
        WorkflowInput {
            name: "flow".to_owned(),
            description: None,
            nodes: self.jobs.clone(),
            edges: edges
                .iter()
                .map(|(from, to, condition)| ((*from).to_owned(), (*to).to_owned(), *condition))
                .collect(),
        }
    }

    async fn graph(&self, edges: &[(&str, &str, Condition)]) -> Result<WorkflowRecord> {
        Ok(self
            .app
            .create_workflow(&self.context, self.namespace_id.get(), self.input(edges))
            .await?)
    }

    async fn start(&self, graph: &WorkflowRecord) -> Result<WorkflowRunRecord> {
        Ok(self
            .app
            .start_workflow(
                &self.context,
                graph.id.get(),
                Uuid::now_v7(),
                TargetSelection::Target(self.target),
                serde_json::json!({"value":"invocation"}),
            )
            .await?
            .0)
    }

    async fn read(&self, run: &WorkflowRunRecord) -> Result<WorkflowRunRecord> {
        Ok(self
            .app
            .get_workflow_run(&self.context, run.id.get())
            .await?)
    }

    /// Simulate transport acknowledgement, then use the unchanged claim/completion APIs.
    async fn complete_run(&self, id: RunId, succeeded: bool) -> Result<()> {
        self.complete_run_with_skip(id, succeeded, false).await
    }

    /// Exercise the normal worker dry-run disposition without changing DAG logic.
    async fn complete_run_with_skip(
        &self,
        id: RunId,
        succeeded: bool,
        skipped: bool,
    ) -> Result<()> {
        let (attempt_id, queue_id): (Uuid, Uuid) = sqlx::query_as("SELECT a.id, r.queue_id FROM crono.run_attempts a JOIN crono.runs r ON r.id = a.run_id WHERE r.id = $1 ORDER BY a.attempt DESC LIMIT 1")
            .bind(id.get()).fetch_one(&self.pool).await?;
        sqlx::query("UPDATE crono.run_attempts SET status = 'queued' WHERE id = $1 AND status = 'pending_dispatch'").bind(attempt_id).execute(&self.pool).await?;
        sqlx::query(
            "UPDATE crono.runs SET status = 'queued' WHERE id = $1 AND status = 'pending_dispatch'",
        )
        .bind(id.get())
        .execute(&self.pool)
        .await?;
        let claimed = self
            .store
            .claim_attempt(&ClaimRequest {
                run_id: id.get(),
                attempt_id,
                queue_id,
                worker_id: "workflow-test-worker".to_owned(),
            })
            .await?;
        assert!(claimed.execution.is_some());
        assert!(
            self.store
                .complete_attempt(&CompletionRequest {
                    attempt_id,
                    worker_id: "workflow-test-worker".to_owned(),
                    succeeded,
                    skipped,
                    exit_code: Some(i32::from(!succeeded)),
                    stdout_tail: "normal output".to_owned(),
                    stderr_tail: String::new(),
                    error: None
                })
                .await?
        );
        Ok(())
    }

    async fn complete(
        &self,
        invocation: &WorkflowRunRecord,
        name: &str,
        succeeded: bool,
    ) -> Result<()> {
        let record = self.read(invocation).await?;
        for child in &node(&record, name)?.runs {
            self.complete_run(child.run_id, succeeded).await?;
        }
        self.store.reconcile(100).await?;
        Ok(())
    }

    /// Remove only this fixture's durable relationships in dependency order.
    async fn cleanup(self) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        for query in [
            "DELETE FROM crono.workflow_completion_events WHERE workflow_run_id IN (SELECT id FROM crono.workflow_runs WHERE namespace_id = $1)",
            "DELETE FROM crono.workflow_run_edges WHERE workflow_run_id IN (SELECT id FROM crono.workflow_runs WHERE namespace_id = $1)",
            "DELETE FROM crono.workflow_node_executions WHERE node_run_id IN (SELECT n.id FROM crono.workflow_node_runs n JOIN crono.workflow_runs w ON w.id = n.workflow_run_id WHERE w.namespace_id = $1)",
            "DELETE FROM crono.workflow_node_runs WHERE workflow_run_id IN (SELECT id FROM crono.workflow_runs WHERE namespace_id = $1)",
            "DELETE FROM crono.workflow_runs WHERE namespace_id = $1",
            "DELETE FROM crono.workflows WHERE namespace_id = $1",
            "DELETE FROM crono.outbox WHERE run_id IN (SELECT r.id FROM crono.runs r JOIN crono.jobs j ON j.id = r.job_id WHERE j.namespace_id = $1)",
            "DELETE FROM crono.run_events WHERE run_id IN (SELECT r.id FROM crono.runs r JOIN crono.jobs j ON j.id = r.job_id WHERE j.namespace_id = $1)",
            "DELETE FROM crono.run_attempts WHERE run_id IN (SELECT r.id FROM crono.runs r JOIN crono.jobs j ON j.id = r.job_id WHERE j.namespace_id = $1)",
            "DELETE FROM crono.runs WHERE job_id IN (SELECT id FROM crono.jobs WHERE namespace_id = $1)",
            "DELETE FROM crono.run_requests WHERE namespace_id = $1",
            "DELETE FROM crono.target_set_members WHERE target_set_id IN (SELECT id FROM crono.target_sets WHERE namespace_id = $1)",
            "DELETE FROM crono.target_sets WHERE namespace_id = $1",
            "DELETE FROM crono.jobs WHERE namespace_id = $1",
            "DELETE FROM crono.targets WHERE namespace_id = $1",
            "DELETE FROM crono.namespaces WHERE id = $1",
        ] {
            sqlx::query(query)
                .bind(self.namespace_id.get())
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        self.store.close().await;
        self.pool.close().await;
        Ok(())
    }
}

fn node<'a>(run: &'a WorkflowRunRecord, name: &str) -> Result<&'a WorkflowNodeRunRecord> {
    run.nodes
        .iter()
        .find(|node| node.name.as_str() == name)
        .ok_or_else(|| anyhow!("missing fixture node {name}"))
}
fn child(run: &WorkflowRunRecord, name: &str) -> Result<RunId> {
    node(run, name)?
        .runs
        .first()
        .map(|child| child.run_id)
        .ok_or_else(|| anyhow!("node {name} has no child Run"))
}

#[tokio::test]
async fn ordinary_execution_retry_does_not_release_dependencies_until_final_outcome() -> Result<()>
{
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE crono.jobs SET idempotent = true, max_attempts = 2 WHERE namespace_id = $1",
    )
    .bind(f.namespace_id.get())
    .execute(&f.pool)
    .await?;
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let run = f.start(&graph).await?;
    let root = child(&run, "a")?;
    f.complete_run(root, false).await?;
    f.store.reconcile(100).await?;
    assert_eq!(node(&f.read(&run).await?, "b")?.runs, []);
    let status: String = sqlx::query_scalar("SELECT status FROM crono.runs WHERE id = $1")
        .bind(root.get())
        .fetch_one(&f.pool)
        .await?;
    assert_eq!(status, "retry_wait");
    sqlx::query("UPDATE crono.runs SET next_retry_at = statement_timestamp() - interval '1 second' WHERE id = $1")
        .bind(root.get()).execute(&f.pool).await?;
    f.store.reconcile(100).await?;
    f.complete_run(root, true).await?;
    f.store.reconcile(100).await?;
    assert_eq!(node(&f.read(&run).await?, "b")?.runs.len(), 1);
    let attempts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM crono.run_attempts WHERE run_id = $1")
            .bind(root.get())
            .fetch_one(&f.pool)
            .await?;
    assert_eq!(attempts, 2);
    f.cleanup().await
}

#[tokio::test]
async fn ambiguous_lease_expiry_and_dry_run_skips_drive_the_same_durable_dependency_queue()
-> Result<()> {
    let Some(f) = Fixture::new(&["a", "verify", "rollback", "cleanup"]).await? else {
        return Ok(());
    };
    let graph = f
        .graph(&[
            ("a", "verify", Condition::Success),
            ("a", "rollback", Condition::Failure),
            ("a", "cleanup", Condition::Always),
        ])
        .await?;
    let run = f.start(&graph).await?;
    let root = child(&run, "a")?;
    // Simulate lost worker ownership; the existing reconciler determines Unknown.
    sqlx::query("UPDATE crono.run_attempts SET status = 'running', lease_expires_at = statement_timestamp() - interval '1 second' WHERE run_id = $1")
        .bind(root.get()).execute(&f.pool).await?;
    sqlx::query("UPDATE crono.runs SET status = 'running' WHERE id = $1")
        .bind(root.get())
        .execute(&f.pool)
        .await?;
    f.store.reconcile(100).await?;
    let progress = f.read(&run).await?;
    assert_eq!(node(&progress, "a")?.state, NodeState::Unknown);
    assert_eq!(node(&progress, "verify")?.state, NodeState::Skipped);
    assert_eq!(node(&progress, "rollback")?.state, NodeState::Running);
    assert_eq!(node(&progress, "cleanup")?.state, NodeState::Running);
    f.complete(&run, "rollback", true).await?;
    f.complete(&run, "cleanup", true).await?;
    assert_eq!(f.read(&run).await?.state, WorkflowState::Failed);

    sqlx::query("UPDATE crono.jobs SET dry_run = true WHERE namespace_id = $1 AND name = 'a'")
        .bind(f.namespace_id.get())
        .execute(&f.pool)
        .await?;
    let dry = f.start(&graph).await?;
    f.complete_run_with_skip(child(&dry, "a")?, true, true)
        .await?;
    f.store.reconcile(100).await?;
    let progress = f.read(&dry).await?;
    assert_eq!(node(&progress, "a")?.state, NodeState::Skipped);
    assert_eq!(node(&progress, "verify")?.state, NodeState::Skipped);
    assert_eq!(node(&progress, "rollback")?.state, NodeState::Skipped);
    assert_eq!(node(&progress, "cleanup")?.state, NodeState::Running);
    f.complete(&dry, "cleanup", true).await?;
    assert_eq!(f.read(&dry).await?.state, WorkflowState::Succeeded);
    f.cleanup().await
}

#[tokio::test]
async fn postgres_rejects_cycle_and_cross_namespace_graph_writes_without_replacing_valid_definitions()
-> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let mut transaction = f.pool.begin().await?;
    sqlx::query("INSERT INTO crono.workflow_edges (workflow_id, from_node_id, to_node_id, condition) SELECT $1, b.id, a.id, 'always' FROM crono.workflow_nodes a CROSS JOIN crono.workflow_nodes b WHERE a.workflow_id = $1 AND b.workflow_id = $1 AND a.name = 'a' AND b.name = 'b'")
        .bind(graph.id.get()).execute(&mut *transaction).await?;
    let error = transaction
        .commit()
        .await
        .err()
        .ok_or_else(|| anyhow!("cycle unexpectedly committed"))?;
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23514")
    );
    assert_eq!(f.store.get_workflow(graph.id).await?, graph);
    let foreign = f
        .store
        .create_namespace(&NamespaceName::parse(&format!(
            "foreign-{}",
            Uuid::now_v7().simple()
        ))?)
        .await?;
    let foreign_job: Uuid = sqlx::query_scalar("INSERT INTO crono.jobs (namespace_id, name, queue_id) SELECT $1, 'foreign', id FROM crono.queues WHERE name = 'default' RETURNING id")
        .bind(foreign.id().get()).fetch_one(&f.pool).await?;
    let mut invalid = f.input(&[]);
    invalid.nodes = vec![("foreign".to_owned(), JobId::new(foreign_job))];
    assert!(matches!(
        f.app
            .update_workflow(&f.context, graph.id.get(), graph.revision, invalid)
            .await,
        Err(ApplicationError::InvalidInput { .. })
    ));
    let mut transaction = f.pool.begin().await?;
    sqlx::query(
        "UPDATE crono.workflow_nodes SET job_id = $2 WHERE workflow_id = $1 AND name = 'a'",
    )
    .bind(graph.id.get())
    .bind(foreign_job)
    .execute(&mut *transaction)
    .await?;
    assert!(transaction.commit().await.is_err());
    assert_eq!(f.store.get_workflow(graph.id).await?, graph);
    sqlx::query("DELETE FROM crono.jobs WHERE id = $1")
        .bind(foreign_job)
        .execute(&f.pool)
        .await?;
    f.store.delete_namespace(foreign.id()).await?;
    f.cleanup().await
}

#[tokio::test]
async fn linear_dependencies_create_normal_runs_only_after_predecessor_success() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b", "c"]).await? else {
        return Ok(());
    };
    let graph = f
        .graph(&[
            ("a", "b", Condition::Success),
            ("b", "c", Condition::Success),
        ])
        .await?;
    let run = f.start(&graph).await?;
    assert_eq!(node(&run, "a")?.state, NodeState::Running);
    assert_eq!(node(&run, "b")?.state, NodeState::Pending);
    assert_eq!(node(&run, "c")?.runs, []);
    f.complete(&run, "a", true).await?;
    let progress = f.read(&run).await?;
    assert_eq!(node(&progress, "b")?.state, NodeState::Running);
    assert_eq!(node(&progress, "c")?.state, NodeState::Pending);
    f.complete(&run, "b", true).await?;
    assert_eq!(node(&f.read(&run).await?, "c")?.state, NodeState::Running);
    f.complete(&run, "c", true).await?;
    let finished = f.read(&run).await?;
    assert_eq!(finished.state, WorkflowState::Succeeded);
    assert!(finished.finished_at.is_some());
    assert!(finished.nodes.iter().all(|node| node.started_at.is_some()
        && node.finished_at.is_some()
        && node.runs.len() == 1));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM crono.outbox o JOIN crono.runs r ON r.id = o.run_id JOIN crono.jobs j ON j.id = r.job_id WHERE j.namespace_id = $1 AND o.subject = 'crono.dispatch.' || r.queue_id::text")
        .bind(f.namespace_id.get()).fetch_one(&f.pool).await?;
    assert_eq!(count, 3);
    f.cleanup().await
}

#[tokio::test]
async fn parallel_branches_start_together_and_join_waits_for_both() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b", "c", "d"]).await? else {
        return Ok(());
    };
    let graph = f
        .graph(&[
            ("a", "b", Condition::Success),
            ("a", "c", Condition::Success),
            ("b", "d", Condition::Success),
            ("c", "d", Condition::Success),
        ])
        .await?;
    let run = f.start(&graph).await?;
    f.complete(&run, "a", true).await?;
    let branches = f.read(&run).await?;
    assert_eq!(node(&branches, "b")?.state, NodeState::Running);
    assert_eq!(node(&branches, "c")?.state, NodeState::Running);
    f.complete(&run, "c", true).await?;
    assert_eq!(node(&f.read(&run).await?, "d")?.runs, []);
    f.complete(&run, "b", true).await?;
    assert_eq!(node(&f.read(&run).await?, "d")?.state, NodeState::Running);
    f.complete(&run, "d", true).await?;
    assert_eq!(f.read(&run).await?.state, WorkflowState::Succeeded);
    f.cleanup().await
}

#[tokio::test]
async fn success_failure_and_always_branches_terminate_without_aborting_recovery() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "verify", "rollback", "cleanup"]).await? else {
        return Ok(());
    };
    let graph = f
        .graph(&[
            ("a", "verify", Condition::Success),
            ("a", "rollback", Condition::Failure),
            ("a", "cleanup", Condition::Always),
        ])
        .await?;
    for succeeded in [true, false] {
        let run = f.start(&graph).await?;
        f.complete(&run, "a", succeeded).await?;
        let state = f.read(&run).await?;
        let selected = if succeeded { "verify" } else { "rollback" };
        let skipped = if succeeded { "rollback" } else { "verify" };
        assert_eq!(node(&state, selected)?.state, NodeState::Running);
        assert_eq!(node(&state, skipped)?.state, NodeState::Skipped);
        assert_eq!(node(&state, skipped)?.runs, []);
        assert_eq!(node(&state, "cleanup")?.state, NodeState::Running);
        assert_eq!(state.state, WorkflowState::Running);
        f.complete(&run, selected, true).await?;
        f.complete(&run, "cleanup", true).await?;
        assert_eq!(
            f.read(&run).await?.state,
            if succeeded {
                WorkflowState::Succeeded
            } else {
                WorkflowState::Failed
            }
        );
    }
    f.cleanup().await
}

#[tokio::test]
async fn impossible_conditions_propagate_skips_and_always_can_follow_a_skip() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b", "c", "cleanup"]).await? else {
        return Ok(());
    };
    let graph = f
        .graph(&[
            ("a", "b", Condition::Success),
            ("b", "c", Condition::Success),
            ("c", "cleanup", Condition::Always),
        ])
        .await?;
    let run = f.start(&graph).await?;
    f.complete(&run, "a", false).await?;
    let progress = f.read(&run).await?;
    assert_eq!(node(&progress, "b")?.state, NodeState::Skipped);
    assert_eq!(node(&progress, "c")?.state, NodeState::Skipped);
    assert_eq!(node(&progress, "cleanup")?.state, NodeState::Running);
    f.complete(&run, "cleanup", true).await?;
    assert_eq!(f.read(&run).await?.state, WorkflowState::Failed);
    f.cleanup().await
}

#[tokio::test]
async fn concurrent_launch_replay_and_reconciliation_create_one_child_per_node() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let request_id = Uuid::now_v7();
    let target = TargetSelection::Target(f.target);
    let (one, two) = tokio::try_join!(
        f.app.start_workflow(
            &f.context,
            graph.id.get(),
            request_id,
            target,
            serde_json::json!({})
        ),
        f.app.start_workflow(
            &f.context,
            graph.id.get(),
            request_id,
            target,
            serde_json::json!({})
        )
    )?;
    assert_eq!(one.0.id, two.0.id);
    assert_ne!(one.1, two.1);
    f.complete_run(child(&one.0, "a")?, true).await?;
    let other = PostgresStore::connect(&f.url, &DatabasePoolConfig::default()).await?;
    tokio::try_join!(f.store.reconcile(100), other.reconcile(100))?;
    let result = f.read(&one.0).await?;
    assert_eq!(node(&result, "b")?.runs.len(), 1);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM crono.runs WHERE request_id = $1")
        .bind(node(&result, "b")?.id.get())
        .fetch_one(&f.pool)
        .await?;
    assert_eq!(count, 1);
    // Redelivered durable completion intent also cannot create a second child.
    sqlx::query("INSERT INTO crono.workflow_completion_events (run_id, workflow_run_id, node_run_id) VALUES ($1,$2,$3)")
        .bind(child(&one.0, "a")?.get()).bind(one.0.id.get()).bind(node(&one.0, "a")?.id.get())
        .execute(&f.pool).await?;
    other.reconcile(100).await?;
    assert_eq!(f.read(&one.0).await?, result);
    assert!(matches!(
        f.app
            .start_workflow(
                &f.context,
                graph.id.get(),
                request_id,
                target,
                serde_json::json!({"different":true})
            )
            .await,
        Err(ApplicationError::IdempotencyConflict)
    ));
    other.close().await;
    f.cleanup().await
}

#[tokio::test]
async fn persisted_completion_event_continues_after_server_state_is_recreated() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let run = f.start(&graph).await?;
    f.complete_run(child(&run, "a")?, true).await?;
    f.store.close().await;
    let restarted = PostgresStore::connect(&f.url, &DatabasePoolConfig::default()).await?;
    restarted.reconcile(100).await?;
    let app = Application::new(Arc::new(restarted.clone()), Arc::new(PermitAllAuthorizer));
    let recovered = app.get_workflow_run(&f.context, run.id.get()).await?;
    assert_eq!(node(&recovered, "a")?.state, NodeState::Succeeded);
    assert_eq!(node(&recovered, "b")?.state, NodeState::Running);
    restarted.reconcile(100).await?;
    assert_eq!(
        app.get_workflow_run(&f.context, run.id.get()).await?,
        recovered
    );
    restarted.close().await;
    f.cleanup().await
}

#[tokio::test]
async fn manual_requests_cannot_preempt_reserved_pending_workflow_invocation_ids() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let run = f.start(&graph).await?;
    let pending = node(&run, "b")?;
    assert!(matches!(
        f.app
            .create_run(
                &f.context,
                pending.id.get(),
                pending.job_id.get(),
                run.target,
                run.inputs.clone()
            )
            .await,
        Err(ApplicationError::IdempotencyConflict)
    ));
    assert_eq!(node(&f.read(&run).await?, "b")?.runs, []);
    f.complete(&run, "a", true).await?;
    assert_eq!(node(&f.read(&run).await?, "b")?.runs.len(), 1);
    f.cleanup().await
}

#[tokio::test]
async fn definition_job_target_and_input_edits_cannot_change_launched_execution_history()
-> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let run = f.start(&graph).await?;
    let mut changed = f.input(&[("a", "b", Condition::Failure)]);
    changed.name = "edited".to_owned();
    f.app
        .update_workflow(&f.context, graph.id.get(), graph.revision, changed)
        .await?;
    sqlx::query(
        "UPDATE crono.jobs SET inputs = '{\"value\":\"edited-job\"}' WHERE namespace_id = $1",
    )
    .bind(f.namespace_id.get())
    .execute(&f.pool)
    .await?;
    sqlx::query("UPDATE crono.targets SET inputs = '{\"value\":\"edited-target\"}' WHERE id = $1")
        .bind(f.target.get())
        .execute(&f.pool)
        .await?;
    f.complete(&run, "a", true).await?;
    let progress = f.read(&run).await?;
    assert_eq!(progress.workflow, run.workflow);
    assert_eq!(node(&progress, "b")?.state, NodeState::Running);
    let snapshot: serde_json::Value =
        sqlx::query_scalar("SELECT execution_snapshot FROM crono.runs WHERE id = $1")
            .bind(child(&progress, "b")?.get())
            .fetch_one(&f.pool)
            .await?;
    assert_eq!(
        snapshot.get("arguments"),
        Some(&serde_json::json!(["invocation"]))
    );
    assert_eq!(
        snapshot.pointer("/inputs/value"),
        Some(&serde_json::json!("invocation"))
    );
    let (replay, created) = f
        .app
        .start_workflow(
            &f.context,
            graph.id.get(),
            run.request_id,
            run.target,
            run.inputs.clone(),
        )
        .await?;
    assert!(!created);
    assert_eq!(replay.workflow, run.workflow);
    assert!(matches!(
        f.app.delete_workflow(&f.context, graph.id.get()).await,
        Err(ApplicationError::InUse)
    ));
    f.cleanup().await
}

#[tokio::test]
async fn target_set_fanout_is_pinned_and_downstream_waits_for_every_member() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let second = f
        .store
        .create_target(
            f.namespace_id,
            &ResourceName::parse("second")?,
            &TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let set = f
        .store
        .create_target_set(
            f.namespace_id,
            &ResourceName::parse("fleet")?,
            &[f.target, second.target.id()],
            &serde_json::json!({"value":"set","set":true}),
        )
        .await?;
    let graph = f.graph(&[("a", "b", Condition::Success)]).await?;
    let run = f
        .app
        .start_workflow(
            &f.context,
            graph.id.get(),
            Uuid::now_v7(),
            TargetSelection::TargetSet(set.target_set.id()),
            serde_json::json!({"value":"invocation"}),
        )
        .await?
        .0;
    assert_eq!(node(&run, "a")?.runs.len(), 2);
    f.store
        .update_target_set(
            set.target_set.id(),
            &ResourceName::parse("fleet")?,
            &[second.target.id()],
            &serde_json::json!({"value":"changed"}),
        )
        .await?;
    let first = node(&run, "a")?
        .runs
        .first()
        .ok_or_else(|| anyhow!("missing fan-out member"))?;
    f.complete_run(first.run_id, true).await?;
    f.store.reconcile(100).await?;
    assert_eq!(node(&f.read(&run).await?, "b")?.runs, []);
    let last = node(&run, "a")?
        .runs
        .last()
        .ok_or_else(|| anyhow!("missing fan-out member"))?;
    f.complete_run(last.run_id, true).await?;
    f.store.reconcile(100).await?;
    let progress = f.read(&run).await?;
    assert_eq!(node(&progress, "b")?.runs.len(), 2);
    for child in &node(&progress, "b")?.runs {
        let snapshot: serde_json::Value =
            sqlx::query_scalar("SELECT execution_snapshot FROM crono.runs WHERE id = $1")
                .bind(child.run_id.get())
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(
            snapshot.pointer("/inputs/value"),
            Some(&serde_json::json!("invocation"))
        );
        assert_eq!(
            snapshot.pointer("/inputs/set"),
            Some(&serde_json::json!(true))
        );
    }
    f.cleanup().await
}

/// A set permission cannot substitute for permission on a specific member.
struct DenyMember(TargetId);
#[async_trait]
impl Authorizer for DenyMember {
    async fn authorize(
        &self,
        _context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        if capability == Capability::TargetUse && resource == &ResourceScope::Target(self.0) {
            Err(AuthorizationError::Forbidden)
        } else {
            Ok(())
        }
    }
    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        Ok(VisibilityScope::All)
    }
}

#[tokio::test]
async fn target_set_permissions_and_concurrent_membership_changes_cannot_introduce_unapproved_targets()
-> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let second = f
        .store
        .create_target(
            f.namespace_id,
            &ResourceName::parse("second")?,
            &TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let set = f
        .store
        .create_target_set(
            f.namespace_id,
            &ResourceName::parse("fleet")?,
            &[f.target],
            &serde_json::json!({}),
        )
        .await?;
    let graph = f.graph(&[]).await?;
    let target = TargetSelection::TargetSet(set.target_set.id());
    let launch = WorkflowLaunch {
        workflow_id: graph.id,
        revision: graph.revision,
        request_id: Uuid::now_v7(),
        target,
        inputs: serde_json::json!({}),
        authorized_targets: vec![f.target],
    };
    f.store
        .update_target_set(
            set.target_set.id(),
            &ResourceName::parse("fleet")?,
            &[f.target, second.target.id()],
            &serde_json::json!({}),
        )
        .await?;
    assert!(matches!(
        f.store.start_workflow(&launch).await,
        Err(StoreError::StaleRevision)
    ));
    let denied_set = Application::new(
        Arc::new(f.store.clone()),
        Arc::new(Deny {
            capability: Capability::TargetSetUse,
            visibility: VisibilityScope::All,
        }),
    );
    assert!(matches!(
        denied_set
            .start_workflow(
                &f.context,
                graph.id.get(),
                Uuid::now_v7(),
                target,
                serde_json::json!({})
            )
            .await,
        Err(ApplicationError::Authorization(
            AuthorizationError::Forbidden
        ))
    ));
    let denied_member = Application::new(
        Arc::new(f.store.clone()),
        Arc::new(DenyMember(second.target.id())),
    );
    assert!(matches!(
        denied_member
            .start_workflow(
                &f.context,
                graph.id.get(),
                Uuid::now_v7(),
                target,
                serde_json::json!({})
            )
            .await,
        Err(ApplicationError::Authorization(
            AuthorizationError::Forbidden
        ))
    ));
    assert_eq!(
        f.app
            .list_workflow_runs(&f.context, graph.id.get(), None, None)
            .await?
            .items,
        []
    );
    f.cleanup().await
}

#[tokio::test]
async fn cancellation_serializes_with_completion_and_never_starts_pending_nodes() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[("a", "b", Condition::Always)]).await?;
    let run = f.start(&graph).await?;
    let stopped = f.app.cancel_workflow_run(&f.context, run.id.get()).await?;
    assert!(stopped.cancellation_requested);
    assert_eq!(stopped.state, WorkflowState::Running);
    assert_eq!(node(&stopped, "a")?.state, NodeState::Running);
    assert_eq!(node(&stopped, "b")?.state, NodeState::Cancelled);
    f.complete(&run, "a", true).await?;
    let finished = f.read(&run).await?;
    assert_eq!(finished.state, WorkflowState::Cancelled);
    assert_eq!(node(&finished, "b")?.runs, []);
    assert_eq!(
        f.app.cancel_workflow_run(&f.context, run.id.get()).await?,
        finished
    );
    f.cleanup().await
}

/// Deny exactly one typed capability; record no client identity/role metadata.
struct Deny {
    capability: Capability,
    visibility: VisibilityScope,
}

/// Use production HTTP composition and safe JSON envelopes; no live NATS is needed.
fn fixture_router(f: &Fixture) -> Result<(Router, String)> {
    use crono_server::{
        api::build_router,
        authentication::{BearerToken, DevelopmentAuthProvider},
        infrastructure::NatsPublisher,
    };
    let token = format!("workflow-test-{}", Uuid::now_v7().simple());
    let router = build_router(
        f.app.clone(),
        Arc::new(f.store.clone()),
        NatsPublisher::new("nats://127.0.0.1:1"),
        Arc::new(DevelopmentAuthProvider::new(BearerToken::new(
            token.clone(),
        )?)?),
    );
    Ok((router, token))
}

/// Send one authenticated request and parse the normal error/resource envelope.
async fn http(
    router: &Router,
    token: Option<&str>,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<(StatusCode, serde_json::Value)> {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let payload = body.map_or_else(Body::empty, |body| Body::from(body.to_string()));
    let response = router.clone().oneshot(request.body(payload)?).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 512 * 1024).await?;
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, value))
}

/// Verify authenticated definition CRUD, pagination, and optimistic revision handling.
async fn http_create_update_definition(
    f: &Fixture,
    router: &Router,
    token: &str,
    definition: &serde_json::Value,
) -> Result<(Uuid, String)> {
    let collection = format!("/api/namespaces/{}/workflows", f.namespace_id.get());
    assert_eq!(
        http(router, None, "POST", &collection, Some(definition.clone()))
            .await?
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, created) = http(
        router,
        Some(token),
        "POST",
        &collection,
        Some(definition.clone()),
    )
    .await?;
    assert_eq!(status, StatusCode::CREATED);
    let id = Uuid::parse_str(
        created
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("missing Workflow ID"))?,
    )?;
    let resource = format!("/api/workflows/{id}");
    assert_eq!(
        http(router, Some(token), "GET", &resource, None).await?.1,
        created
    );
    let list = http(router, Some(token), "GET", &collection, None).await?.1;
    assert_eq!(list.get("items"), Some(&serde_json::json!([created])));
    let mut updated = definition.clone();
    set(&mut updated, "revision", serde_json::json!(99))?;
    assert_eq!(
        http(router, Some(token), "PUT", &resource, Some(updated.clone()))
            .await?
            .0,
        StatusCode::CONFLICT
    );
    set(&mut updated, "revision", serde_json::json!(1))?;
    set(
        &mut updated,
        "description",
        serde_json::json!("updated through HTTP"),
    )?;
    let (status, _) = http(router, Some(token), "PUT", &resource, Some(updated)).await?;
    assert_eq!(status, StatusCode::OK);
    Ok((id, collection))
}

/// Update fixture JSON through checked object access, preserving the production lint contract.
fn set(value: &mut serde_json::Value, name: &str, replacement: serde_json::Value) -> Result<()> {
    value
        .as_object_mut()
        .ok_or_else(|| anyhow!("fixture was not an object"))?
        .insert(name.to_owned(), replacement);
    Ok(())
}

/// Verify cycle rejection and safe removal of a definition with no invocation history.
async fn http_invalid_and_unused_definitions(
    router: &Router,
    token: &str,
    collection: &str,
    definition: serde_json::Value,
) -> Result<()> {
    let mut invalid = definition.clone();
    set(&mut invalid, "name", serde_json::json!("cyclic"))?;
    set(
        &mut invalid,
        "edges",
        serde_json::json!([{"from":"a","to":"b","condition":"success"},{"from":"b","to":"a","condition":"always"}]),
    )?;
    assert_eq!(
        http(router, Some(token), "POST", collection, Some(invalid))
            .await?
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut unused = definition;
    set(&mut unused, "name", serde_json::json!("unused"))?;
    let (_, created) = http(router, Some(token), "POST", collection, Some(unused)).await?;
    let unused_path = format!(
        "/api/workflows/{}",
        created
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("missing unused ID"))?
    );
    assert_eq!(
        http(router, Some(token), "DELETE", &unused_path, None)
            .await?
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        http(router, Some(token), "GET", &unused_path, None)
            .await?
            .0,
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test]
async fn http_workflow_crud_execution_history_and_cancellation_follow_existing_contracts()
-> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let (router, token) = fixture_router(&f)?;
    let definition = serde_json::json!({"name":"via-http", "nodes": f.jobs.iter().map(|(name, id)| serde_json::json!({"name":name,"job_id":id.get()})).collect::<Vec<_>>(),
        "edges":[{"from":"a","to":"b","condition":"success"}]});
    let (id, collection) = http_create_update_definition(&f, &router, &token, &definition).await?;
    let resource = format!("/api/workflows/{id}");
    let request = serde_json::json!({"request_id":Uuid::now_v7(),"target":{"kind":"target","id":f.target.get()},"inputs":{"value":"http"}});
    let runs = format!("{resource}/runs");
    let (status, invocation) =
        http(&router, Some(&token), "POST", &runs, Some(request.clone())).await?;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        invocation.pointer("/workflow/revision"),
        Some(&serde_json::json!(2))
    );
    assert_eq!(
        http(&router, Some(&token), "POST", &runs, Some(request))
            .await?
            .0,
        StatusCode::OK
    );
    let run_id = Uuid::parse_str(
        invocation
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("missing WorkflowRun ID"))?,
    )?;
    let run_path = format!("/api/workflow-runs/{run_id}");
    assert_eq!(
        http(&router, Some(&token), "GET", &run_path, None).await?.1,
        invocation
    );
    let listed = http(&router, Some(&token), "GET", &runs, None).await?.1;
    assert_eq!(listed.get("items"), Some(&serde_json::json!([invocation])));
    assert_eq!(
        http(
            &router,
            Some(&token),
            "POST",
            &format!("{run_path}/cancel"),
            None
        )
        .await?
        .0,
        StatusCode::OK
    );
    let record = f.app.get_workflow_run(&f.context, run_id).await?;
    f.complete(&record, "a", true).await?;
    let finished = http(&router, Some(&token), "GET", &run_path, None).await?.1;
    assert_eq!(finished.get("state"), Some(&serde_json::json!("cancelled")));
    assert_eq!(
        http(&router, Some(&token), "DELETE", &resource, None)
            .await?
            .0,
        StatusCode::CONFLICT
    );
    http_invalid_and_unused_definitions(&router, &token, &collection, definition).await?;
    f.cleanup().await
}
#[async_trait]
impl Authorizer for Deny {
    async fn authorize(
        &self,
        _context: &RequestContext,
        capability: Capability,
        _resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        if capability == self.capability {
            Err(AuthorizationError::Forbidden)
        } else {
            Ok(())
        }
    }
    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        Ok(self.visibility.clone())
    }
}

#[tokio::test]
async fn workflow_execution_cannot_bypass_job_target_or_run_creation_authorization() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[]).await?;
    for capability in [
        Capability::WorkflowExecute,
        Capability::JobExecute,
        Capability::TargetUse,
        Capability::RunCreate,
    ] {
        let app = Application::new(
            Arc::new(f.store.clone()),
            Arc::new(Deny {
                capability,
                visibility: VisibilityScope::All,
            }),
        );
        assert!(matches!(
            app.start_workflow(
                &f.context,
                graph.id.get(),
                Uuid::now_v7(),
                TargetSelection::Target(f.target),
                serde_json::json!({})
            )
            .await,
            Err(ApplicationError::Authorization(
                AuthorizationError::Forbidden
            ))
        ));
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM crono.workflow_runs WHERE namespace_id = $1")
            .bind(f.namespace_id.get())
            .fetch_one(&f.pool)
            .await?;
    assert_eq!(count, 0);
    let run = f.start(&graph).await?;
    let hidden = Application::new(
        Arc::new(f.store.clone()),
        Arc::new(Deny {
            capability: Capability::MonitorRead,
            visibility: VisibilityScope::None,
        }),
    );
    assert!(matches!(
        hidden.get_workflow(&f.context, graph.id.get()).await,
        Err(ApplicationError::NotFound)
    ));
    assert!(matches!(
        hidden.get_workflow_run(&f.context, run.id.get()).await,
        Err(ApplicationError::NotFound)
    ));
    let visible = Application::new(
        Arc::new(f.store.clone()),
        Arc::new(Deny {
            capability: Capability::MonitorRead,
            visibility: VisibilityScope::Namespaces(BTreeSet::from([f.namespace_id])),
        }),
    );
    assert_eq!(
        visible.get_workflow_run(&f.context, run.id.get()).await?.id,
        run.id
    );
    f.cleanup().await
}

#[tokio::test]
async fn invalid_graphs_and_stale_launch_membership_fail_before_root_creation() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b", "c"]).await? else {
        return Ok(());
    };
    assert!(matches!(
        f.app
            .create_workflow(
                &f.context,
                f.namespace_id.get(),
                f.input(&[
                    ("a", "b", Condition::Success),
                    ("b", "c", Condition::Success),
                    ("c", "a", Condition::Success)
                ])
            )
            .await,
        Err(ApplicationError::InvalidInput { .. })
    ));
    let graph = f.graph(&[]).await?;
    assert!(matches!(
        f.app
            .update_workflow(&f.context, graph.id.get(), graph.revision + 1, f.input(&[]))
            .await,
        Err(ApplicationError::Conflict)
    ));
    let launch = WorkflowLaunch {
        workflow_id: graph.id,
        revision: graph.revision,
        request_id: Uuid::now_v7(),
        target: TargetSelection::Target(f.target),
        inputs: serde_json::json!({}),
        authorized_targets: Vec::new(),
    };
    assert!(matches!(
        f.store.start_workflow(&launch).await,
        Err(StoreError::StaleRevision)
    ));
    assert!(
        f.store
            .workflow_run_for_request(launch.request_id, &launch.inputs)
            .await?
            .is_none()
    );
    assert_eq!(
        f.app
            .list_workflow_runs(&f.context, graph.id.get(), None, None)
            .await?
            .items,
        []
    );
    f.app.delete_workflow(&f.context, graph.id.get()).await?;
    f.cleanup().await
}

/// Pause authorization to force an invocation to appear after the initial replay lookup.
struct PauseJobAuthorization {
    paused: JobId,
    denied: JobId,
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}

#[async_trait]
impl Authorizer for PauseJobAuthorization {
    async fn authorize(
        &self,
        _context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        if capability == Capability::JobExecute {
            if *resource == ResourceScope::Job(self.denied) {
                return Err(AuthorizationError::Forbidden);
            }
            if *resource == ResourceScope::Job(self.paused) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
        }
        Ok(())
    }
    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        Ok(VisibilityScope::All)
    }
}

#[tokio::test]
async fn racing_replay_authorizes_the_returned_historical_jobs() -> Result<()> {
    let Some(f) = Fixture::new(&["a", "b"]).await? else {
        return Ok(());
    };
    let mut initial = f.input(&[]);
    initial.nodes.retain(|(name, _)| name == "a");
    let graph = f
        .app
        .create_workflow(&f.context, f.namespace_id.get(), initial)
        .await?;
    let a = f
        .jobs
        .iter()
        .find(|(name, _)| name == "a")
        .ok_or_else(|| anyhow!("missing a"))?
        .1;
    let b = f
        .jobs
        .iter()
        .find(|(name, _)| name == "b")
        .ok_or_else(|| anyhow!("missing b"))?
        .1;
    let authorizer = Arc::new(PauseJobAuthorization {
        paused: a,
        denied: b,
        entered: tokio::sync::Notify::new(),
        resume: tokio::sync::Notify::new(),
    });
    let app = Application::new(Arc::new(f.store.clone()), authorizer.clone());
    let request_id = Uuid::now_v7();
    let target = TargetSelection::Target(f.target);
    let inputs = serde_json::json!({});
    let mut racing = Box::pin(app.start_workflow(
        &f.context,
        graph.id.get(),
        request_id,
        target,
        inputs.clone(),
    ));
    tokio::select! {
        () = authorizer.entered.notified() => {},
        result = &mut racing => return Err(anyhow!("launch completed before pause: {result:?}")),
    }
    let mut replacement = f.input(&[]);
    replacement.nodes.retain(|(name, _)| name == "b");
    f.app
        .update_workflow(&f.context, graph.id.get(), graph.revision, replacement)
        .await?;
    let winner = f
        .app
        .start_workflow(&f.context, graph.id.get(), request_id, target, inputs)
        .await?
        .0;
    assert_eq!(
        winner
            .nodes
            .first()
            .ok_or_else(|| anyhow!("missing winning node"))?
            .job_id,
        b
    );
    authorizer.resume.notify_one();
    let result = racing.await;
    f.cleanup().await?;
    assert!(
        matches!(
            result,
            Err(ApplicationError::Authorization(
                AuthorizationError::Forbidden
            ))
        ),
        "unauthorized historical invocation was disclosed: {result:?}"
    );
    Ok(())
}

#[tokio::test]
async fn numeric_inputs_replay_after_postgres_normalization() -> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let graph = f.graph(&[]).await?;
    let request_id = Uuid::now_v7();
    let target = TargetSelection::Target(f.target);
    let inputs: serde_json::Value =
        serde_json::from_str(r#"{"value":1e16,"exact":9007199254740993}"#)?;
    let first = f
        .app
        .start_workflow(
            &f.context,
            graph.id.get(),
            request_id,
            target,
            inputs.clone(),
        )
        .await?;
    assert!(first.1);
    let replay = f
        .app
        .start_workflow(
            &f.context,
            graph.id.get(),
            request_id,
            target,
            inputs.clone(),
        )
        .await;
    let direct = f
        .store
        .start_workflow(&WorkflowLaunch {
            workflow_id: graph.id,
            revision: graph.revision,
            request_id,
            target,
            inputs: inputs.clone(),
            authorized_targets: vec![f.target],
        })
        .await;
    let mut different = inputs;
    set(
        &mut different,
        "exact",
        serde_json::json!(9_007_199_254_740_992_u64),
    )?;
    let conflict = f
        .app
        .start_workflow(&f.context, graph.id.get(), request_id, target, different)
        .await;
    f.cleanup().await?;
    assert!(
        matches!(replay, Ok((_, false))),
        "same invocation Inputs rejected: {replay:?}"
    );
    assert!(
        matches!(direct, Ok((_, false))),
        "store replay rejected: {direct:?}"
    );
    assert!(matches!(
        conflict,
        Err(ApplicationError::IdempotencyConflict)
    ));
    Ok(())
}

#[tokio::test]
async fn manual_numeric_replays_use_the_same_exact_json_comparison() -> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let job_id = f
        .jobs
        .first()
        .ok_or_else(|| anyhow!("missing fixture Job"))?
        .1;
    let request_id = Uuid::now_v7();
    let target = TargetSelection::Target(f.target);
    let inputs: serde_json::Value =
        serde_json::from_str(r#"{"value":1e16,"exact":9007199254740993}"#)?;
    let first = f
        .app
        .create_run(&f.context, request_id, job_id.get(), target, inputs.clone())
        .await?;
    assert!(first.created);
    let replay = f
        .app
        .create_run(&f.context, request_id, job_id.get(), target, inputs.clone())
        .await;
    let mut different = inputs;
    set(
        &mut different,
        "exact",
        serde_json::json!(9_007_199_254_740_992_u64),
    )?;
    let conflict = f
        .app
        .create_run(&f.context, request_id, job_id.get(), target, different)
        .await;
    f.cleanup().await?;
    let replay = replay?;
    assert!(!replay.created);
    assert_eq!(replay.runs, first.runs);
    assert!(matches!(
        conflict,
        Err(ApplicationError::IdempotencyConflict)
    ));
    Ok(())
}

/// Build a legal fleet whose prepared graph crosses one documented launch budget.
async fn oversized_launch_reports_its_bound(
    nodes: usize,
    members: usize,
    inputs: serde_json::Value,
) -> Result<()> {
    let Some(f) = Fixture::new(&["a"]).await? else {
        return Ok(());
    };
    let mut definition = f.input(&[]);
    let job_id = definition
        .nodes
        .first()
        .ok_or_else(|| anyhow!("missing fixture Job"))?
        .1;
    definition.nodes = (0..nodes)
        .map(|index| (format!("node-{index}"), job_id))
        .collect();
    let graph = f
        .app
        .create_workflow(&f.context, f.namespace_id.get(), definition)
        .await?;
    let mut targets = vec![f.target];
    for index in 1..members {
        let target = f
            .store
            .create_target(
                f.namespace_id,
                &ResourceName::parse(&format!("member-{index}"))?,
                &TargetDefinition {
                    arguments: Vec::new(),
                    inputs: serde_json::json!({}),
                },
            )
            .await?;
        targets.push(target.target.id());
    }
    let set = f
        .store
        .create_target_set(
            f.namespace_id,
            &ResourceName::parse("fleet")?,
            &targets,
            &serde_json::json!({}),
        )
        .await?;
    let (router, token) = fixture_router(&f)?;
    let (status, response) = http(
        &router,
        Some(&token),
        "POST",
        &format!("/api/workflows/{}/runs", graph.id.get()),
        Some(serde_json::json!({"request_id": Uuid::now_v7(), "target": {"kind":"target_set", "id": set.target_set.id().get()}, "inputs": inputs})),
    ).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM crono.workflow_runs WHERE namespace_id = $1")
            .bind(f.namespace_id.get())
            .fetch_one(&f.pool)
            .await?;
    let requests: i64 =
        sqlx::query_scalar("SELECT count(*) FROM crono.run_requests WHERE namespace_id = $1")
            .bind(f.namespace_id.get())
            .fetch_one(&f.pool)
            .await?;
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM crono.runs WHERE job_id = $1")
        .bind(job_id.get())
        .fetch_one(&f.pool)
        .await?;
    f.cleanup().await?;
    assert_eq!(
        (count, requests, runs),
        (0, 0, 0),
        "oversized launch must be atomic"
    );
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let message = response
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("missing budget error message"))?;
    assert!(
        message.contains("4,096") && message.contains("8 MiB"),
        "misleading launch bound: {message}"
    );
    Ok(())
}

#[tokio::test]
async fn workflow_execution_count_limit_reports_an_actionable_error_and_rolls_back() -> Result<()> {
    oversized_launch_reports_its_bound(64, 65, serde_json::json!({})).await
}

#[tokio::test]
async fn workflow_snapshot_byte_limit_reports_an_actionable_error_and_rolls_back() -> Result<()> {
    oversized_launch_reports_its_bound(1, 145, serde_json::json!({"padding": "x".repeat(62_000)}))
        .await
}
