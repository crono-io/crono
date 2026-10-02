//! Isolated HTTP tracing regression: complete request logs never reveal credentials.
//!
//! A dedicated test binary prevents unrelated parallel tracing callsites from
//! changing subscriber interest while this test captures the real API boundary.

mod support;

use anyhow::Result;
use axum::{
    body::{self, Body},
    http::{Request, StatusCode},
};
use crono_server::{
    api::build_router,
    application::{Application, ControlPlaneStore, PermitAllAuthorizer},
    authentication::{BearerToken, DevelopmentAuthProvider},
    infrastructure::{DatabasePoolConfig, NatsPublisher, PostgresStore},
};
use std::{
    io,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
use tracing::instrument::WithSubscriber;

#[derive(Clone)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("log capture poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn authentication_credentials_never_appear_in_complete_request_traces_or_error_bodies()
-> Result<()> {
    let Some(url) = support::database_url()? else {
        return Ok(());
    };
    let postgres = PostgresStore::connect(&url, &DatabasePoolConfig::default()).await?;
    let store: Arc<dyn ControlPlaneStore> = Arc::new(postgres.clone());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = LogCapture(Arc::clone(&captured));
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let token = "test-only-http-log-credential-123456789";
    let provider = DevelopmentAuthProvider::new(BearerToken::new(token.to_string())?)?;
    let app = build_router(
        Application::new(Arc::clone(&store), Arc::new(PermitAllAuthorizer)),
        store,
        NatsPublisher::new("nats://127.0.0.1:1"),
        Arc::new(provider),
    );
    let request = Request::builder()
        .uri("/api/namespaces")
        .header("authorization", format!("Bearer {token}-wrong"))
        .body(Body::empty())?;
    let response = app.oneshot(request).with_subscriber(subscriber).await?;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body::to_bytes(response.into_body(), 4096).await?;
    assert!(!std::str::from_utf8(&body)?.contains(token));
    let bytes = captured
        .lock()
        .map_err(|_| anyhow::anyhow!("capture poisoned"))?
        .clone();
    let logs = String::from_utf8(bytes)?;
    assert!(logs.contains("http.request"), "missing HTTP trace");
    assert!(!logs.contains(token));
    postgres.close().await;
    Ok(())
}
