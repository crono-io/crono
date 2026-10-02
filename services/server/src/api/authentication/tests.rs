//! Exercise authentication through the same transport stack used by the server.

use crate::{
    api::{app, request_id::REQUEST_ID_HEADER},
    application::{Principal, PrincipalKind, RequestContext},
    authentication::{
        AuthProvider, AuthenticationError, BearerToken, DevelopmentAuthProvider, RequestCredentials,
    },
};
use anyhow::Result;
use async_trait::async_trait;
use axum::{
    Extension, Json, Router,
    body::{self, Body},
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    routing::get,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

const TEST_TOKEN: &str = "test-only-transport-credential-123456789";

async fn identity(Extension(context): Extension<RequestContext>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"id": context.principal().id(), "request_id": context.request_id()}))
}

fn test_app(provider: Arc<dyn AuthProvider>) -> Router {
    app(
        Router::new()
            .route("/probe", get(identity))
            .route("/live", get(|| async { "alive" })),
        provider,
    )
}

fn development_app() -> Result<Router> {
    Ok(test_app(Arc::new(DevelopmentAuthProvider::new(
        BearerToken::new(TEST_TOKEN.to_string())?,
    )?)))
}

async fn rejection(response: axum::response::Response, status: StatusCode) -> Result<()> {
    assert_eq!(response.status(), status);
    assert!(response.headers().contains_key(REQUEST_ID_HEADER));
    if status == StatusCode::UNAUTHORIZED {
        assert_eq!(
            response.headers().get("www-authenticate"),
            Some(&HeaderValue::from_static("Bearer realm=\"crono\""))
        );
    }
    let bytes = body::to_bytes(response.into_body(), 4096).await?;
    let text = std::str::from_utf8(&bytes)?;
    assert!(!text.contains(TEST_TOKEN));
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let code = if status == StatusCode::UNAUTHORIZED {
        "unauthenticated"
    } else {
        "dependency_unavailable"
    };
    assert_eq!(
        value
            .pointer("/error/code")
            .and_then(serde_json::Value::as_str),
        Some(code)
    );
    Ok(())
}

#[tokio::test]
async fn missing_unsupported_and_malformed_credentials_return_401_before_handlers() -> Result<()> {
    let app = development_app()?;
    for value in [
        None,
        Some(""),
        Some("Bearer"),
        Some("Bearer "),
        Some("Basic ignored"),
        Some("Bearer wrong"),
        Some("Bearer\tvalue"),
        Some(" Bearer value"),
        Some("Bearer value extra"),
        Some("Bearer a=b"),
        Some("Bearer value, Bearer value"),
    ] {
        let mut request = Request::builder().uri("/probe");
        if let Some(value) = value {
            request = request.header("authorization", value);
        }
        rejection(
            app.clone().oneshot(request.body(Body::empty())?).await?,
            StatusCode::UNAUTHORIZED,
        )
        .await?;
    }
    let request = Request::builder()
        .uri("/probe")
        .header("authorization", format!("Bearer {TEST_TOKEN}"))
        .header("authorization", format!("Bearer {TEST_TOKEN}"))
        .body(Body::empty())?;
    rejection(app.oneshot(request).await?, StatusCode::UNAUTHORIZED).await?;
    Ok(())
}

#[tokio::test]
async fn bearer_scheme_is_case_insensitive_while_token_identity_is_provider_owned() -> Result<()> {
    for scheme in ["Bearer ", "bearer ", "bEaReR   "] {
        let mut request = Request::builder()
            .uri("/probe")
            .header("authorization", format!("{scheme}{TEST_TOKEN}"))
            .header("x-principal", "attacker")
            .header("x-role", "admin")
            .header("x-capability", "*")
            .body(Body::empty())?;
        request.extensions_mut().insert(RequestContext::new(
            Uuid::nil(),
            Principal::new("attacker".to_string(), PrincipalKind::System),
        ));
        let response = development_app()?.oneshot(request).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let value: serde_json::Value =
            serde_json::from_slice(&body::to_bytes(response.into_body(), 4096).await?)?;
        assert_eq!(
            value.get("id").and_then(serde_json::Value::as_str),
            Some("development/local")
        );
        assert_eq!(
            value.get("request_id").and_then(serde_json::Value::as_str),
            request_id.as_deref()
        );
    }
    Ok(())
}

struct UnavailableProvider(Arc<AtomicUsize>);

#[async_trait]
impl AuthProvider for UnavailableProvider {
    async fn authenticate(
        &self,
        _credentials: &RequestCredentials,
    ) -> Result<Principal, AuthenticationError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(AuthenticationError::Unavailable)
    }
}

#[tokio::test]
async fn provider_outage_fails_closed_and_public_probes_do_not_call_the_provider() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = test_app(Arc::new(UnavailableProvider(Arc::clone(&calls))));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/probe")
                .header("authorization", format!("Bearer {TEST_TOKEN}"))
                .body(Body::empty())?,
        )
        .await?;
    rejection(response, StatusCode::SERVICE_UNAVAILABLE).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let response = app
        .clone()
        .oneshot(Request::builder().uri("/live").body(Body::empty())?)
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .oneshot(Request::builder().uri("/live/other").body(Body::empty())?)
        .await?;
    rejection(response, StatusCode::UNAUTHORIZED).await?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn query_body_and_identity_headers_cannot_supply_credentials() -> Result<()> {
    let request = Request::builder()
        .method("POST")
        .uri(format!("/probe?access_token={TEST_TOKEN}"))
        .header("x-principal", "development/local")
        .header("x-role", "admin")
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"access_token":"{TEST_TOKEN}","principal":"development/local","role":"admin"}}"#
        )))?;
    rejection(
        development_app()?.oneshot(request).await?,
        StatusCode::UNAUTHORIZED,
    )
    .await?;
    Ok(())
}

#[test]
fn non_ascii_and_oversized_headers_fail_without_exposing_their_value() -> Result<()> {
    for value in [
        HeaderValue::from_bytes(b"Bearer caf\xc3\xa9")?,
        HeaderValue::from_str(&format!("Bearer {}", "a".repeat(8193)))?,
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", value);
        assert!(matches!(
            super::credentials(&headers),
            Err(AuthenticationError::InvalidCredentials)
        ));
    }
    Ok(())
}
