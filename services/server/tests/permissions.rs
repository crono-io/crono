//! Provider-neutral grants enforced through the production router and PostgreSQL.
//!
//! Fixtures use explicit development grants to create workloads, then exercise
//! restricted callers against authoritative UUID ownership. Each test removes its
//! own Namespaces and history; no test grants authority through HTTP metadata.

mod support;

#[path = "permissions/cancellation.rs"]
mod cancellation;
#[path = "permissions/execution.rs"]
mod execution;
#[path = "permissions/mutations.rs"]
mod mutations;
#[path = "permissions/reads.rs"]
mod reads;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use crono_server::{
    api::build_router,
    application::{
        Application, ApplicationError, AuthenticatedCaller, AuthorizationError, Capability,
        ControlPlaneStore, GrantAuthorizer, GrantScope, GrantSet, JobDefinition, Principal,
        PrincipalKind, RequestContext, TargetDefinition, WorkflowInput, WorkflowRecord,
    },
    authentication::{AuthProvider, AuthenticationError, RequestCredentials},
    domain::{
        ExecutorKind, JobId, NamespaceId, NamespaceName, QueueName, ResourceName, TargetId,
        TargetSelection, TargetSetId,
    },
    infrastructure::{DatabasePoolConfig, NatsPublisher, PostgresStore},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN: &str = "test-only-permission-credential-1234567890";

/// Two fake verified provider layouts share the same application contract.
#[derive(Clone)]
enum Authority {
    Document(Option<Vec<u8>>),
    Assignments(Vec<(GrantScope, Vec<Capability>)>),
}

struct Provider(Authority);

#[async_trait]
impl AuthProvider for Provider {
    async fn authenticate(
        &self,
        credentials: &RequestCredentials,
    ) -> Result<AuthenticatedCaller, AuthenticationError> {
        let RequestCredentials::Bearer(token) = credentials;
        if token.expose_secret() != TOKEN {
            return Err(AuthenticationError::InvalidCredentials);
        }
        // Only this fixture credential establishes trust. Production adapters must
        // verify issuer, audience, validity, and delegation before normalization.
        let grants = match &self.0 {
            Authority::Document(document) => GrantSet::from_verified_json(document.as_deref()),
            Authority::Assignments(assignments) => GrantSet::new(assignments.clone()),
        }
        .map_err(|_| AuthenticationError::InvalidCredentials)?;
        Ok(AuthenticatedCaller::new(principal(), grants))
    }
}

fn principal() -> Principal {
    Principal::from_issuer(
        "https://fixture.example".to_owned(),
        "permission-user".to_owned(),
        PrincipalKind::Human,
    )
}

fn context(grants: GrantSet) -> RequestContext {
    RequestContext::new(
        Uuid::now_v7(),
        AuthenticatedCaller::new(principal(), grants),
    )
}

fn scoped(namespace_id: NamespaceId, permissions: &[Capability]) -> Result<GrantSet> {
    Ok(GrantSet::new([(
        GrantScope::Namespace { namespace_id },
        permissions.to_vec(),
    )])?)
}

struct Workload {
    namespace: NamespaceId,
    job: JobId,
    target: TargetId,
    target_set: TargetSetId,
    workflow: WorkflowRecord,
}

struct Fixture {
    store: PostgresStore,
    pool: PgPool,
    app: Application,
    a: Workload,
    b: Workload,
}

impl Fixture {
    async fn new() -> Result<Option<Self>> {
        let Some(url) = support::database_url()? else {
            return Ok(None);
        };
        let store = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
        let pool = PgPool::connect(&url).await?;
        let app = Application::new(
            Arc::new(store.clone()),
            Arc::new(GrantAuthorizer::new(Arc::new(store.clone()))),
        );
        let admin = context(GrantSet::development());
        let a = Self::workload(&store, &app, &admin, "a").await?;
        let b = Self::workload(&store, &app, &admin, "b").await?;
        Ok(Some(Self {
            store,
            pool,
            app,
            a,
            b,
        }))
    }

    async fn workload(
        store: &PostgresStore,
        app: &Application,
        admin: &RequestContext,
        prefix: &str,
    ) -> Result<Workload> {
        let namespace = store
            .create_namespace(&NamespaceName::parse(&format!(
                "permissions-{prefix}-{}",
                Uuid::now_v7().simple()
            ))?)
            .await?
            .id();
        let queue = store
            .get_queue_by_name(&QueueName::parse("default")?)
            .await?;
        let job = store
            .create_job(
                namespace,
                &ResourceName::parse("job")?,
                &JobDefinition {
                    executor: ExecutorKind::Noop,
                    queue_id: queue.id(),
                    executable: None,
                    shell_command: None,
                    arguments: Vec::new(),
                    inputs: json!({}),
                    idempotent: false,
                    dry_run: false,
                    max_attempts: 1,
                    retry_initial_seconds: 1,
                    retry_max_seconds: 1,
                    retry_multiplier: 1.0,
                    retry_jitter: 0.0,
                },
            )
            .await?
            .job
            .id();
        let target = store
            .create_target(
                namespace,
                &ResourceName::parse("target")?,
                &TargetDefinition {
                    arguments: Vec::new(),
                    inputs: json!({}),
                },
            )
            .await?
            .target
            .id();
        let target_set = store
            .create_target_set(
                namespace,
                &ResourceName::parse("set")?,
                &[target],
                &json!({}),
            )
            .await?
            .target_set
            .id();
        let workflow = app
            .create_workflow(
                admin,
                namespace.get(),
                WorkflowInput {
                    name: "flow".to_owned(),
                    description: None,
                    nodes: vec![("node".to_owned(), job)],
                    edges: Vec::new(),
                },
            )
            .await?;
        Ok(Workload {
            namespace,
            job,
            target,
            target_set,
            workflow,
        })
    }

    fn router(&self, authority: Authority) -> Router {
        build_router(
            self.app.clone(),
            Arc::new(self.store.clone()),
            NatsPublisher::new("nats://127.0.0.1:1"),
            Arc::new(Provider(authority)),
        )
    }

    fn grants_router(&self, grants: &GrantSet) -> Result<Router> {
        Ok(self.router(Authority::Document(Some(grants.to_json()?))))
    }

    async fn run(&self, workload: &Workload) -> Result<crono_server::domain::RunId> {
        self.app
            .create_run(
                &context(GrantSet::development()),
                Uuid::now_v7(),
                workload.job.get(),
                TargetSelection::Target(workload.target),
                json!({}),
            )
            .await?
            .runs
            .first()
            .map(|record| record.run.id())
            .ok_or_else(|| anyhow!("missing Run"))
    }

    async fn invocation(&self, workload: &Workload) -> Result<crono_server::domain::WorkflowRunId> {
        Ok(self
            .app
            .start_workflow(
                &context(GrantSet::development()),
                workload.workflow.id.get(),
                Uuid::now_v7(),
                TargetSelection::Target(workload.target),
                json!({}),
            )
            .await?
            .0
            .id)
    }

    /// Counts only fixture-owned execution intent, including invisible dispatch rows.
    async fn intent_counts(&self) -> Result<(i64, i64, i64)> {
        Ok(sqlx::query_as(
            "SELECT (SELECT count(*) FROM crono.run_requests WHERE namespace_id = ANY($1)), \
            (SELECT count(*) FROM crono.workflow_runs WHERE namespace_id = ANY($1)), \
            (SELECT count(*) FROM crono.outbox o JOIN crono.runs r ON r.id = o.run_id \
            JOIN crono.jobs j ON j.id = r.job_id WHERE j.namespace_id = ANY($1))",
        )
        .bind(vec![self.a.namespace.get(), self.b.namespace.get()])
        .fetch_one(&self.pool)
        .await?)
    }

    async fn cleanup(self) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        for workload in [&self.a, &self.b] {
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
                "DELETE FROM crono.schedule_events WHERE schedule_id IN (SELECT id FROM crono.schedules WHERE namespace_id = $1)",
                "DELETE FROM crono.schedules WHERE namespace_id = $1",
                "DELETE FROM crono.target_set_members WHERE target_set_id IN (SELECT id FROM crono.target_sets WHERE namespace_id = $1)",
                "DELETE FROM crono.target_sets WHERE namespace_id = $1",
                "DELETE FROM crono.jobs WHERE namespace_id = $1",
                "DELETE FROM crono.targets WHERE namespace_id = $1",
                "DELETE FROM crono.namespaces WHERE id = $1",
            ] {
                sqlx::query(query)
                    .bind(workload.namespace.get())
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        transaction.commit().await?;
        self.store.close().await;
        self.pool.close().await;
        Ok(())
    }
}

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    payload: Option<Value>,
) -> Result<(StatusCode, Value)> {
    let request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .header("x-role", "admin")
        .header("x-permissions", "*")
        .header("x-namespace-id", Uuid::nil().to_string())
        .body(payload.map_or_else(Body::empty, |value| Body::from(value.to_string())))?;
    let response = router.clone().oneshot(request).await?;
    let status = response.status();
    let value = serde_json::from_slice(&to_bytes(response.into_body(), 256 * 1024).await?)?;
    Ok((status, value))
}

fn forbidden(result: Result<impl std::fmt::Debug, ApplicationError>) {
    let error = result.err();
    assert!(
        matches!(
            error,
            Some(ApplicationError::Authorization(
                AuthorizationError::Forbidden
            ))
        ),
        "expected an authorization denial, received {error:?}"
    );
}

fn items(value: &Value) -> Result<&Vec<Value>> {
    value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("missing page items"))
}
