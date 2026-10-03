//! Verify replaceable authentication against existing resource handlers and policy.
//!
//! HTTP requests use the production router with a throwaway PostgreSQL store.
//! Recording policy receives only provider-issued identity and typed scopes;
//! failed verification cannot reach it. No handlers or use cases are specialized
//! for the fake external provider.

mod support;

use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Router,
    body::{self, Body},
    http::{Request, StatusCode},
};
use crono_server::{
    api::build_router,
    application::{
        Application, AuthenticatedCaller, AuthorizationError, Authorizer, Capability,
        ControlPlaneStore, PermitAllAuthorizer, Principal, PrincipalKind, RequestContext,
        ResourceScope, VisibilityScope,
    },
    authentication::{
        AuthProvider, AuthenticationError, BearerToken, DevelopmentAuthProvider, RequestCredentials,
    },
    domain::{NamespaceId, NamespaceName},
    infrastructure::{DatabasePoolConfig, NatsPublisher, PostgresStore},
};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;

const TEST_TOKEN: &str = "test-only-http-integration-token-123456789";
type Decisions = Arc<Mutex<Vec<(Principal, Capability, ResourceScope)>>>;

/// Test policy grants only when explicitly configured; caller headers never select it.
struct RecordingAuthorizer {
    decisions: Decisions,
    allow: bool,
}

#[async_trait]
impl Authorizer for RecordingAuthorizer {
    async fn authorize(
        &self,
        context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        self.decisions
            .lock()
            .map_err(|_| AuthorizationError::Unavailable)?
            .push((context.principal().clone(), capability, resource.clone()));
        if self.allow {
            Ok(())
        } else {
            Err(AuthorizationError::Forbidden)
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

/// Represents an external verifier: one opaque fixture credential maps to a verified identity.
struct FakeAuthProvider {
    principal: Principal,
}

#[async_trait]
impl AuthProvider for FakeAuthProvider {
    async fn authenticate(
        &self,
        credentials: &RequestCredentials,
    ) -> Result<AuthenticatedCaller, AuthenticationError> {
        let RequestCredentials::Bearer(token) = credentials;
        if token.expose_secret() == TEST_TOKEN {
            Ok(self.principal.clone().into())
        } else {
            Err(AuthenticationError::InvalidCredentials)
        }
    }
}

fn router(
    store: &PostgresStore,
    provider: Arc<dyn AuthProvider>,
    authorizer: Arc<dyn Authorizer>,
) -> Router {
    let store: Arc<dyn ControlPlaneStore> = Arc::new(store.clone());
    let application = Application::new(Arc::clone(&store), authorizer);
    build_router(
        application,
        store,
        NatsPublisher::new("nats://127.0.0.1:1"),
        provider,
    )
}

fn development_provider() -> Result<Arc<dyn AuthProvider>> {
    Ok(Arc::new(DevelopmentAuthProvider::new(BearerToken::new(
        TEST_TOKEN.to_string(),
    )?)?))
}

async fn response_json(response: axum::response::Response) -> Result<serde_json::Value> {
    Ok(serde_json::from_slice(
        &body::to_bytes(response.into_body(), 64 * 1024).await?,
    )?)
}

#[tokio::test]
async fn verified_development_identity_reaches_the_existing_authorizer_before_create() -> Result<()>
{
    let Some(url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let app = router(
        &store,
        development_provider()?,
        Arc::new(RecordingAuthorizer {
            decisions: Arc::clone(&decisions),
            allow: true,
        }),
    );
    let name = format!("auth-test-{}", Uuid::now_v7().simple());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/namespaces")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .header("content-type", "application/json")
                .header("x-principal", "attacker")
                .header("x-role", "admin")
                .header("x-capability", "*")
                .body(Body::from(serde_json::json!({"name":name}).to_string()))?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let value = response_json(response).await?;
    let id = Uuid::parse_str(
        value
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing namespace id"))?,
    )?;
    let observed = decisions
        .lock()
        .map_err(|_| anyhow::anyhow!("decisions poisoned"))?
        .clone();
    assert_eq!(
        observed,
        vec![(
            Principal::new("development/local".to_string(), PrincipalKind::Development),
            Capability::NamespaceCreate,
            ResourceScope::ControlPlane
        )]
    );
    assert_eq!(
        store
            .get_namespace(NamespaceId::new(id))
            .await?
            .name()
            .as_str(),
        name
    );
    store.delete_namespace(NamespaceId::new(id)).await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn fake_external_provider_uses_the_unchanged_namespace_handler_and_resource_policy()
-> Result<()> {
    let Some(url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
    let namespace = store
        .create_namespace(&NamespaceName::parse(&format!(
            "external-{}",
            Uuid::now_v7().simple()
        ))?)
        .await?;
    let principal = Principal::from_issuer(
        "https://test.example".to_string(),
        "test/external-user".to_string(),
        PrincipalKind::Service,
    );
    let provider: Arc<dyn AuthProvider> = Arc::new(FakeAuthProvider {
        principal: principal.clone(),
    });
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let app = router(
        &store,
        provider,
        Arc::new(RecordingAuthorizer {
            decisions: Arc::clone(&decisions),
            allow: true,
        }),
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/namespaces/{}", namespace.id().get()))
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response)
            .await?
            .get("name")
            .and_then(serde_json::Value::as_str),
        Some(namespace.name().as_str())
    );
    assert_eq!(
        *decisions
            .lock()
            .map_err(|_| anyhow::anyhow!("decisions poisoned"))?,
        vec![(
            principal,
            Capability::NamespaceRead,
            ResourceScope::Namespace(namespace.id())
        )]
    );
    store.delete_namespace(namespace.id()).await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn rejected_credentials_skip_authorization_and_authenticated_denial_remains_403() -> Result<()>
{
    let Some(url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
    let decisions = Arc::new(Mutex::new(Vec::new()));
    let app = router(
        &store,
        development_provider()?,
        Arc::new(RecordingAuthorizer {
            decisions: Arc::clone(&decisions),
            allow: false,
        }),
    );
    for token in [None, Some("wrong"), Some(TEST_TOKEN)] {
        let mut request = Request::builder().uri(format!("/api/namespaces/{}", Uuid::now_v7()));
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = app.clone().oneshot(request.body(Body::empty())?).await?;
        assert_eq!(
            response.status(),
            if token == Some(TEST_TOKEN) {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
    }
    assert_eq!(
        decisions
            .lock()
            .map_err(|_| anyhow::anyhow!("decisions poisoned"))?
            .len(),
        1
    );
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn development_provider_plus_permit_all_preserves_access_and_rejects_body_authority()
-> Result<()> {
    let Some(url) = support::database_url()? else {
        return Ok(());
    };
    let store = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
    let app = router(
        &store,
        development_provider()?,
        Arc::new(PermitAllAuthorizer),
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/namespaces")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    for field in ["principal", "role", "capability"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/namespaces")
                    .header("authorization", format!("Bearer {TEST_TOKEN}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"name": "injected", (field): "admin"}).to_string(),
                    ))?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    store.close().await;
    Ok(())
}
