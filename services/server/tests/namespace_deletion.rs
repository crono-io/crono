//! Verify Namespace deletion authorization, bootstrap protection, and reference safety.
//!
//! Isolated Namespaces live only in the throwaway test database. A failed deletion
//! must preserve contained definitions and immutable execution history; denying
//! the scoped capability takes precedence over existence and protection checks.

mod support;

use anyhow::Result;
use async_trait::async_trait;
use crono_server::{
    application::{
        Application, ApplicationError, AuthorizationError, Authorizer, Capability,
        ControlPlaneStore, DevelopmentIdentity, JobDefinition, PermitAllAuthorizer, RequestContext,
        ResourceScope, StoreError, TargetDefinition, VisibilityScope,
    },
    domain::{ExecutorKind, NamespaceId, NamespaceName, QueueName, ResourceName},
    infrastructure::{DatabasePoolConfig, PostgresStore},
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn deleting_empty_namespace_releases_its_name_and_reports_missing_ids() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let name = NamespaceName::parse(&format!("delete-ns-{}", Uuid::now_v7().simple()))?;
    let namespace = store.create_namespace(&name).await?;
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    assert_eq!(
        app.list_jobs(&context, namespace.id().get(), None, None)
            .await?
            .items,
        []
    );
    assert_eq!(
        app.list_targets(&context, namespace.id().get(), None, None)
            .await?
            .items,
        []
    );
    assert_eq!(
        app.list_target_sets(&context, namespace.id().get(), None, None)
            .await?
            .items,
        []
    );
    assert_eq!(
        app.list_schedules(&context, namespace.id().get(), None, None)
            .await?
            .items,
        []
    );
    app.delete_namespace(&context, namespace.id().get()).await?;
    assert!(matches!(
        store.get_namespace(namespace.id()).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        app.delete_namespace(&context, namespace.id().get()).await,
        Err(ApplicationError::NotFound)
    ));
    for result in [
        app.list_jobs(&context, namespace.id().get(), None, None)
            .await
            .map(|_| ()),
        app.list_targets(&context, namespace.id().get(), None, None)
            .await
            .map(|_| ()),
        app.list_target_sets(&context, namespace.id().get(), None, None)
            .await
            .map(|_| ()),
        app.list_schedules(&context, namespace.id().get(), None, None)
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(result, Err(ApplicationError::NotFound)));
    }
    let replacement = store.create_namespace(&name).await?;
    assert_ne!(replacement.id(), namespace.id());
    app.delete_namespace(&context, replacement.id().get())
        .await?;
    Ok(())
}

/// Deny precisely the requested Namespace capability and scope without side effects.
struct DenyDeletion(NamespaceId);

#[async_trait]
impl Authorizer for DenyDeletion {
    async fn authorize(
        &self,
        _context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        assert_eq!(capability, Capability::NamespaceDelete);
        assert_eq!(resource, &ResourceScope::Namespace(self.0));
        Err(AuthorizationError::Forbidden)
    }

    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        Err(AuthorizationError::Forbidden)
    }
}

#[tokio::test]
async fn namespace_deletion_authorizes_before_default_and_existence_checks() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let pool = PgPool::connect(&database_url).await?;
    let default_id: Uuid =
        sqlx::query_scalar("SELECT id FROM crono.namespaces WHERE name = 'default'")
            .fetch_one(&pool)
            .await?;
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    for id in [default_id, Uuid::now_v7()] {
        let app = Application::new(
            Arc::new(store.clone()),
            Arc::new(DenyDeletion(NamespaceId::new(id))),
        );
        assert!(matches!(
            app.delete_namespace(&context, id).await,
            Err(ApplicationError::Authorization(
                AuthorizationError::Forbidden
            ))
        ));
    }
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    assert!(matches!(
        app.delete_namespace(&context, default_id).await,
        Err(ApplicationError::InvalidInput { field: None, .. })
    ));
    assert!(
        store
            .get_namespace(NamespaceId::new(default_id))
            .await
            .is_ok()
    );
    Ok(())
}

#[tokio::test]
async fn namespace_deletion_succeeds_only_after_its_last_target_is_removed() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let namespace = store
        .create_namespace(&NamespaceName::parse(&format!(
            "target-ns-{}",
            Uuid::now_v7().simple()
        ))?)
        .await?;
    let target = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("destination")?,
            &TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    assert!(matches!(
        app.delete_namespace(&context, namespace.id().get()).await,
        Err(ApplicationError::InUse)
    ));
    assert!(store.get_namespace(namespace.id()).await.is_ok());
    assert!(store.get_target(target.target.id()).await.is_ok());
    app.delete_target(&context, target.target.id().get())
        .await?;
    app.delete_namespace(&context, namespace.id().get()).await?;
    Ok(())
}

#[tokio::test]
async fn namespace_deletion_preserves_definitions_and_run_history() -> Result<()> {
    let Some(database_url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&database_url, &DatabasePoolConfig::default()).await?;
    let namespace = store
        .create_namespace(&NamespaceName::parse(&format!(
            "history-ns-{}",
            Uuid::now_v7().simple()
        ))?)
        .await?;
    let queue = store
        .get_queue_by_name(&QueueName::parse("default")?)
        .await?;
    let job = store
        .create_job(
            namespace.id(),
            &ResourceName::parse("job")?,
            &JobDefinition {
                executor: ExecutorKind::Noop,
                queue_id: queue.id(),
                executable: None,
                shell_command: None,
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
                idempotent: false,
                dry_run: false,
                max_attempts: 1,
                retry_initial_seconds: 1,
                retry_max_seconds: 60,
                retry_multiplier: 2.0,
                retry_jitter: 0.2,
            },
        )
        .await?;
    let app = Application::new(Arc::new(store.clone()), Arc::new(PermitAllAuthorizer));
    let context = DevelopmentIdentity.context(Uuid::now_v7());
    // A Job alone must block deletion before any Target exists.
    assert!(matches!(
        app.delete_namespace(&context, namespace.id().get()).await,
        Err(ApplicationError::InUse)
    ));
    let target = store
        .create_target(
            namespace.id(),
            &ResourceName::parse("destination")?,
            &TargetDefinition {
                arguments: Vec::new(),
                inputs: serde_json::json!({}),
            },
        )
        .await?;
    let set = store
        .create_target_set(
            namespace.id(),
            &ResourceName::parse("group")?,
            &[target.target.id()],
            &serde_json::json!({}),
        )
        .await?;
    let (run, _) = store
        .create_run(Uuid::now_v7(), job.job.id(), target.target.id())
        .await?;
    assert!(matches!(
        app.delete_namespace(&context, namespace.id().get()).await,
        Err(ApplicationError::InUse)
    ));
    assert!(store.get_namespace(namespace.id()).await.is_ok());
    assert!(store.get_job(job.job.id()).await.is_ok());
    assert!(store.get_target(target.target.id()).await.is_ok());
    assert_eq!(
        store.get_target_set(set.target_set.id()).await?.targets,
        vec![target.target]
    );
    assert!(
        store
            .get_run(run.run.id(), &VisibilityScope::All)
            .await
            .is_ok()
    );
    Ok(())
}
