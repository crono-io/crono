//! Exercise the evaluator without HTTP, IAM libraries, or workload payloads.

use super::*;
use crate::{
    application::{AuthenticatedCaller, GrantScope, GrantSet, Principal, PrincipalKind},
    domain::{JobId, QueueId, WorkflowId},
};
use anyhow::Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

struct Resolver {
    namespace: Option<NamespaceId>,
    unavailable: bool,
    calls: AtomicUsize,
}

#[async_trait]
impl ResourceNamespaceResolver for Resolver {
    async fn namespace_for(
        &self,
        _: &ResourceScope,
    ) -> Result<Option<NamespaceId>, AuthorizationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.unavailable {
            Err(AuthorizationError::Unavailable)
        } else {
            Ok(self.namespace)
        }
    }
}

fn context(grants: GrantSet) -> RequestContext {
    RequestContext::new(
        Uuid::now_v7(),
        AuthenticatedCaller::new(
            Principal::from_issuer(
                "https://issuer.example".to_string(),
                "subject".to_string(),
                PrincipalKind::Human,
            ),
            grants,
        ),
    )
}

#[tokio::test]
async fn explicit_namespace_grants_use_authoritative_membership_and_hide_history() -> Result<()> {
    let allowed = NamespaceId::new(Uuid::from_u128(1));
    let foreign = NamespaceId::new(Uuid::from_u128(2));
    let resolver = Arc::new(Resolver {
        namespace: Some(foreign),
        unavailable: false,
        calls: AtomicUsize::new(0),
    });
    let policy = GrantAuthorizer::new(resolver.clone());
    let context = context(GrantSet::new([(
        GrantScope::Namespace {
            namespace_id: allowed,
        },
        vec![
            Capability::JobRead,
            Capability::JobExecute,
            Capability::RunRead,
            Capability::WorkflowRead,
        ],
    )])?);
    assert!(
        policy
            .authorize(
                &context,
                Capability::JobRead,
                &ResourceScope::Namespace(allowed)
            )
            .await
            .is_ok()
    );
    assert_eq!(
        policy
            .authorize(
                &context,
                Capability::JobExecute,
                &ResourceScope::Job(JobId::new(Uuid::now_v7()))
            )
            .await,
        Err(AuthorizationError::Forbidden)
    );
    assert_eq!(
        policy
            .authorize(
                &context,
                Capability::RunRead,
                &ResourceScope::Run(Uuid::now_v7())
            )
            .await,
        Err(AuthorizationError::NotFound)
    );
    assert_eq!(
        policy
            .authorize(
                &context,
                Capability::WorkflowRead,
                &ResourceScope::Workflow(WorkflowId::new(Uuid::now_v7()))
            )
            .await,
        Err(AuthorizationError::NotFound)
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 3);
    Ok(())
}

#[tokio::test]
async fn missing_metadata_and_resolver_failure_never_enable_access() -> Result<()> {
    let id = NamespaceId::new(Uuid::now_v7());
    let context = context(GrantSet::new([(
        GrantScope::Namespace { namespace_id: id },
        vec![Capability::JobRead],
    )])?);
    for (unavailable, expected) in [
        (false, AuthorizationError::NotFound),
        (true, AuthorizationError::Unavailable),
    ] {
        let policy = GrantAuthorizer::new(Arc::new(Resolver {
            namespace: None,
            unavailable,
            calls: AtomicUsize::new(0),
        }));
        assert_eq!(
            policy
                .authorize(
                    &context,
                    Capability::JobRead,
                    &ResourceScope::Job(JobId::new(Uuid::now_v7()))
                )
                .await,
            Err(expected)
        );
    }
    Ok(())
}

#[tokio::test]
async fn development_grants_preserve_access_without_bypassing_resource_types() -> Result<()> {
    let resolver = Arc::new(Resolver {
        namespace: None,
        unavailable: true,
        calls: AtomicUsize::new(0),
    });
    let policy = GrantAuthorizer::new(resolver.clone());
    let context = context(GrantSet::development());
    policy
        .authorize(
            &context,
            Capability::JobExecute,
            &ResourceScope::Job(JobId::new(Uuid::now_v7())),
        )
        .await?;
    policy
        .authorize(
            &context,
            Capability::QueueRead,
            &ResourceScope::Queue(QueueId::new(Uuid::now_v7())),
        )
        .await?;
    assert_eq!(
        policy
            .visibility(&context, Capability::OverviewRead)
            .await?,
        VisibilityScope::All
    );
    assert_eq!(
        policy
            .authorize(
                &context,
                Capability::JobExecute,
                &ResourceScope::ControlPlane
            )
            .await,
        Err(AuthorizationError::Forbidden)
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn absent_authority_denies_before_lookup_and_mutations_have_no_read_visibility() -> Result<()>
{
    let resolver = Arc::new(Resolver {
        namespace: None,
        unavailable: true,
        calls: AtomicUsize::new(0),
    });
    let policy = GrantAuthorizer::new(resolver.clone());
    let context = context(GrantSet::default());
    assert_eq!(
        policy
            .authorize(
                &context,
                Capability::JobExecute,
                &ResourceScope::Job(JobId::new(Uuid::now_v7()))
            )
            .await,
        Err(AuthorizationError::Forbidden)
    );
    assert_eq!(
        policy
            .visibility(&context, Capability::OverviewRead)
            .await?,
        VisibilityScope::None
    );
    assert_eq!(
        policy.visibility(&context, Capability::JobCreate).await,
        Err(AuthorizationError::Forbidden)
    );
    assert_eq!(
        policy.visibility(&context, Capability::QueueRead).await,
        Err(AuthorizationError::Forbidden)
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    Ok(())
}
