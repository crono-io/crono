//! Verify worker diagnostics stay optional and require a global read decision.

use anyhow::{Context, Result};
use async_trait::async_trait;
use crono_api::{WorkerDiagnostics, WorkerHeartbeatRequest};
use crono_server::{
    application::{
        Application, ApplicationError, AuthorizationError, Authorizer, Capability,
        ControlPlaneStore, DevelopmentIdentity, PermitAllAuthorizer, RequestContext, ResourceScope,
        VisibilityScope,
    },
    domain::QueueName,
    infrastructure::{DatabasePoolConfig, PostgresStore},
};
use sqlx::PgPool;
use std::{env, sync::Arc};
use uuid::Uuid;

struct DenyWorkerRead;

#[async_trait]
impl Authorizer for DenyWorkerRead {
    async fn authorize(
        &self,
        _context: &RequestContext,
        _capability: Capability,
        _resource: &ResourceScope,
    ) -> std::result::Result<(), AuthorizationError> {
        Err(AuthorizationError::Forbidden)
    }

    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> std::result::Result<VisibilityScope, AuthorizationError> {
        Err(AuthorizationError::Forbidden)
    }
}

#[tokio::test]
#[ignore = "requires an initialized CRONO_TEST_DATABASE_URL"]
async fn worker_details_are_authorized_and_legacy_heartbeats_remain_readable() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL")?;
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default())
        .await
        .context("connect store")?;
    let pool = PgPool::connect(&database_url)
        .await
        .context("connect pool")?;
    let queue = store
        .get_queue_by_name(&QueueName::parse("default")?)
        .await
        .context("load default queue")?;
    let random = Uuid::now_v7().simple().to_string();
    let id = format!("worker-{}", random.get(20..).unwrap_or("test"));
    let mut heartbeat = WorkerHeartbeatRequest {
        worker_id: id.clone(),
        session_id: Uuid::now_v7(),
        queue_id: queue.id().get(),
        concurrency: 3,
        version: "test".to_string(),
        diagnostics: Some(WorkerDiagnostics {
            hostname: "test-host".to_string(),
            os: "linux".to_string(),
            architecture: "x86_64".to_string(),
            default_shell_path: "/bin/sh".to_string(),
            default_shell_present: true,
            dry_run: false,
            lang: Some("C.UTF-8".to_string()),
            lc_all: None,
            tz: None,
        }),
    };
    store
        .record_worker_heartbeat(&heartbeat)
        .await
        .context("record first heartbeat")?;
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    let allowed = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let details = allowed
        .get_worker(&context, &id)
        .await
        .context("get first worker")?;
    assert_eq!(
        details
            .diagnostics
            .as_ref()
            .map(|item| item.hostname.as_str()),
        Some("test-host")
    );
    let denied = Application::new(Arc::new(store.clone()), Arc::new(DenyWorkerRead));
    assert!(matches!(
        denied.get_worker(&context, &id).await,
        Err(ApplicationError::Authorization(
            AuthorizationError::Forbidden
        ))
    ));
    heartbeat.session_id = Uuid::now_v7();
    heartbeat.diagnostics = None;
    store
        .record_worker_heartbeat(&heartbeat)
        .await
        .context("record legacy heartbeat")?;
    assert!(
        allowed
            .get_worker(&context, &id)
            .await?
            .diagnostics
            .is_none()
    );
    sqlx::query("DELETE FROM crono.worker_presence WHERE worker_id = $1")
        .bind(&id)
        .execute(&pool)
        .await?;
    assert!(matches!(
        allowed.get_worker(&context, &id).await,
        Err(ApplicationError::NotFound)
    ));
    Ok(())
}
