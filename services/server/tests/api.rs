//! Verify the public HTTP and `OpenAPI` contracts.

use anyhow::{Context, Result};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use std::process::Command;
use tower::ServiceExt;

#[tokio::test]
async fn process_probes_are_healthy() -> Result<()> {
    let (router, _openapi) = crono_server::api::router().split_for_parts();

    for path in ["/live", "/ready"] {
        let response = router
            .clone()
            .oneshot(Request::get(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(to_bytes(response.into_body(), usize::MAX).await?.is_empty());
    }

    Ok(())
}

#[tokio::test]
async fn detailed_health_reports_build_identity() -> Result<()> {
    let (router, _openapi) = crono_server::api::router().split_for_parts();
    let response = router
        .oneshot(Request::get("/health").body(Body::empty())?)
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let x_app = response
        .headers()
        .get("x-app")
        .context("missing X-App header")?
        .to_str()?;
    let commit = crono_server::built_info::GIT_COMMIT_HASH.unwrap_or("unknown");
    let short_commit: String = commit.chars().take(7).collect();
    assert_eq!(
        x_app,
        format!("crono-server:{}:{short_commit}", env!("CARGO_PKG_VERSION"))
    );

    let body: Value = serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(body.get("commit").and_then(Value::as_str), Some(commit));
    assert_eq!(
        body.get("name").and_then(Value::as_str),
        Some("crono-server")
    );
    assert_eq!(
        body.get("version").and_then(Value::as_str),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(body.as_object().map(serde_json::Map::len), Some(3));
    Ok(())
}

#[tokio::test]
async fn router_rejects_unknown_paths_and_methods() -> Result<()> {
    let (router, _openapi) = crono_server::api::router().split_for_parts();
    for (request, expected) in [
        (
            Request::get("/missing").body(Body::empty())?,
            StatusCode::NOT_FOUND,
        ),
        (
            Request::post("/health").body(Body::empty())?,
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ] {
        let response = router.clone().oneshot(request).await?;
        assert_eq!(response.status(), expected);
    }
    Ok(())
}

#[test]
fn openapi_contains_only_the_health_contract() -> Result<()> {
    let document = crono_server::api::openapi();
    assert_eq!(document.info.title, "crono-server");
    assert_eq!(document.info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(document.paths.paths.len(), 3);
    for path in ["/live", "/ready", "/health"] {
        assert!(document.paths.paths.contains_key(path));
    }
    assert!(
        document
            .components
            .as_ref()
            .is_some_and(|components| components.schemas.contains_key("Health"))
    );
    let value = serde_json::to_value(&document)?;
    assert!(
        value
            .get("paths")
            .and_then(|paths| paths.get("/health"))
            .and_then(|path| path.get("get"))
            .and_then(|operation| operation.get("responses"))
            .and_then(|responses| responses.get("200"))
            .and_then(|response| response.get("headers"))
            .and_then(|headers| headers.get("X-App"))
            .is_some()
    );
    Ok(())
}

#[test]
fn openapi_binary_emits_parseable_document() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_crono-server-openapi")).output()?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let document: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        document
            .get("info")
            .and_then(|info| info.get("title"))
            .and_then(Value::as_str),
        Some("crono-server")
    );
    assert!(
        document
            .get("paths")
            .and_then(|paths| paths.get("/health"))
            .is_some()
    );
    Ok(())
}
