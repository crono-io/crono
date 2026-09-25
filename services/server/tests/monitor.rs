//! Read-only operator monitoring checks against an isolated initialized test database.
//!
//! The database test is opt-in because a live scheduler can change aggregate
//! counts between the independent SQL reads. It never inserts or removes data.

use anyhow::Result;
use async_trait::async_trait;
use crono_server::{
    application::{
        Application, AuthorizationError, Authorizer, Capability, ControlPlaneStore,
        DevelopmentIdentity, RequestContext, ResourceScope, VisibilityScope,
    },
    infrastructure::PostgresStore,
};
use sqlx::PgPool;
use std::{env, sync::Arc};

struct DenyMonitor;

#[async_trait]
impl Authorizer for DenyMonitor {
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
#[ignore = "requires an isolated initialized CRONO_TEST_DATABASE_URL"]
async fn monitor_samples_database_counts_and_rejects_unauthorized_readers() -> Result<()> {
    let database_url = env::var("CRONO_TEST_DATABASE_URL")?;
    let store = PostgresStore::connect(&database_url).await?;
    let snapshot = store.monitor_snapshot().await?;
    let pool = PgPool::connect(&database_url).await?;
    let expected: (i64, i64, Option<::time::OffsetDateTime>, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM crono.schedules WHERE enabled = true AND next_run_at IS NOT NULL),
                (SELECT count(*) FROM crono.schedules WHERE enabled = true AND next_run_at <= statement_timestamp()),
                (SELECT min(next_run_at) FROM crono.schedules WHERE enabled = true AND next_run_at IS NOT NULL),
                (SELECT count(*) FROM crono.outbox WHERE published_at IS NULL AND cancelled_at IS NULL),
                pg_database_size(current_database())::bigint",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(snapshot.enabled_schedules, expected.0);
    assert_eq!(snapshot.due_schedules, expected.1);
    assert_eq!(snapshot.earliest_next_run_at, expected.2);
    assert_eq!(snapshot.metrics.outbox_pending, expected.3);
    assert!(snapshot.database_size_bytes > 0);
    assert!(expected.4 > 0);
    assert!(snapshot.pool_connections <= snapshot.pool_max_connections);
    assert!(snapshot.pool_idle_connections <= snapshot.pool_connections);

    let application = Application::new(Arc::new(store), Arc::new(DenyMonitor));
    let denied = application
        .monitor_snapshot(&DevelopmentIdentity.context())
        .await;
    assert!(matches!(
        denied,
        Err(crono_server::application::ApplicationError::Authorization(
            AuthorizationError::Forbidden
        ))
    ));
    Ok(())
}
