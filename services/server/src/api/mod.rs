//! HTTP API router and process lifecycle.
//!
//! Routes are wrapped in three transport layers. Request-ID assignment runs
//! first and mints a server-owned `UUIDv7` correlation ID; the trace layer then
//! opens a span carrying the method, matched route, and that ID; identity
//! runs last and builds the trusted `RequestContext` from the same ID. The ID
//! is returned to callers in `x-request-id`, and a client-supplied value is
//! never adopted as the correlation ID (see [`request_id`]).
//!
//! `serve` also owns the background loops (NATS connection manager, worker
//! control, outbox dispatcher, scheduler, and reconciler). They share one
//! cancellation token, stop when the listener drains after a shutdown signal,
//! and are joined before `serve` returns so the caller can release shared
//! resources such as the PostgreSQL pool afterwards.

use crate::{
    application::{Application, ControlPlaneStore},
    infrastructure::{DispatcherConfig, NatsPublisher, run_dispatcher, run_worker_control},
    reconciliation::run_reconciler,
    scheduler::run_scheduler,
};
use anyhow::{Context, Result};
use axum::{Router, middleware};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io::{self, ErrorKind},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use utoipa_axum::router::OpenApiRouter;

mod error;
pub(crate) mod handlers;
mod identity;
mod openapi;
mod request_id;
mod state;

pub use openapi::openapi;

/// Build the documented API router.
#[must_use]
fn router(state: state::AppState) -> OpenApiRouter {
    openapi::api_router().with_state(state)
}

/// Wrap routes in the correlation, tracing, and identity layers.
///
/// Axum runs the last-added layer first, so requests pass through
/// [`request_id::assign`], then the trace span, then [`identity::establish`].
/// Both inner layers read the ID the outer one inserted, so this order is
/// required. Applying the layers after routing also exposes `MatchedPath` to
/// the span.
fn with_transport_layers(router: Router) -> Router {
    router
        .layer(middleware::from_fn(identity::establish))
        .layer(TraceLayer::new_for_http().make_span_with(request_id::make_span))
        .layer(middleware::from_fn(request_id::assign))
}

/// Bind and serve the control-plane API until the process receives a shutdown signal.
///
/// # Errors
///
/// Returns an error when the listener cannot be created or the HTTP server fails.
pub async fn serve(
    port: u16,
    application: Application,
    store: Arc<dyn ControlPlaneStore>,
    publisher: NatsPublisher,
    dispatcher_config: DispatcherConfig,
) -> Result<()> {
    let (listener, listen_addr) = bind_listener(port)?;
    let listener = tokio::net::TcpListener::from_std(listener)
        .context("failed to create asynchronous API listener")?;
    let state = state::AppState::new(application, Arc::clone(&store), publisher.clone());
    let (router, _openapi) = router(state).split_for_parts();
    let app = with_transport_layers(router);

    let cancellation = CancellationToken::new();
    let connection_publisher = publisher.clone();
    let connection_cancellation = cancellation.child_token();
    let nats_manager = tokio::spawn(async move {
        connection_publisher
            .run_connection_manager(connection_cancellation)
            .await;
    });
    let worker_control = tokio::spawn(run_worker_control(
        Arc::clone(&store),
        publisher.clone(),
        cancellation.child_token(),
    ));
    let dispatcher = tokio::spawn(run_dispatcher(
        Arc::clone(&store),
        publisher,
        dispatcher_config,
        cancellation.child_token(),
    ));
    let scheduler = tokio::spawn(run_scheduler(
        Arc::clone(&store),
        cancellation.child_token(),
    ));
    let reconciler = tokio::spawn(run_reconciler(store, cancellation.child_token()));

    info!(address = %listen_addr, "Crono API listening over internal HTTP");
    let server = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Crono API server failed");
    cancellation.cancel();
    dispatcher
        .await
        .context("Run dispatch task failed during shutdown")?;
    worker_control
        .await
        .context("worker control task failed during shutdown")?;
    scheduler
        .await
        .context("scheduler task failed during shutdown")?;
    reconciler
        .await
        .context("reconciler task failed during shutdown")?;
    nats_manager
        .await
        .context("NATS connection task failed during shutdown")?;
    server
}

fn bind_listener(port: u16) -> Result<(TcpListener, SocketAddr)> {
    let ipv6_addr = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0));
    match bind_ipv6_dual_stack(ipv6_addr) {
        Ok(listener) => {
            let address = listener
                .local_addr()
                .context("failed to read API listener address")?;
            Ok((listener, address))
        }
        Err(error) if ipv6_unavailable(&error) => {
            let ipv4_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port));
            let listener = bind_socket(Domain::IPV4, ipv4_addr)
                .with_context(|| format!("failed to bind IPv4 API listener on {ipv4_addr}"))?;
            let address = listener
                .local_addr()
                .context("failed to read API listener address")?;
            Ok((listener, address))
        }
        Err(error) => Err(error),
    }
}

fn bind_ipv6_dual_stack(address: SocketAddr) -> Result<TcpListener> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))
        .context("failed to create IPv6 API socket")?;
    socket
        .set_only_v6(false)
        .context("failed to configure dual-stack API socket")?;
    bind_configured_socket(socket, address)
        .with_context(|| format!("failed to bind dual-stack API listener on {address}"))
}

fn bind_socket(domain: Domain, address: SocketAddr) -> Result<TcpListener> {
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
        .context("failed to create API socket")?;
    bind_configured_socket(socket, address)
}

/// Reuse a recently closed address without permitting two live API listeners.
fn bind_configured_socket(socket: Socket, address: SocketAddr) -> Result<TcpListener> {
    socket
        .set_reuse_address(true)
        .context("failed to configure API address reuse")?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    Ok(socket.into())
}

fn ipv6_unavailable(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        source
            .downcast_ref::<io::Error>()
            .is_some_and(io_error_is_ipv6_unavailable)
    })
}

fn io_error_is_ipv6_unavailable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::AddrNotAvailable | ErrorKind::PermissionDenied | ErrorKind::Unsupported
    ) || matches!(error.raw_os_error(), Some(1 | 43 | 47 | 49 | 93 | 97 | 99))
}

async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            error!(%error, "failed to listen for interrupt signal");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                let _ = signal.recv().await;
            }
            Err(error) => {
                error!(%error, "failed to listen for termination signal");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    tokio::select! {
        () = interrupt => {}
        () = terminate => {}
    }

    #[cfg(not(unix))]
    interrupt.await;

    info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::RequestContext;
    use axum::{
        Extension,
        body::{self, Body},
        http::{Request, StatusCode},
        routing::get,
    };
    use request_id::REQUEST_ID_HEADER;
    use tower::ServiceExt;
    use uuid::Uuid;

    async fn echo_request_id(Extension(context): Extension<RequestContext>) -> String {
        context.request_id().to_string()
    }

    fn probe_app() -> Router {
        with_transport_layers(Router::new().route("/probe", get(echo_request_id)))
    }

    fn issued_request_id(response: &axum::response::Response) -> Result<Uuid> {
        let header = response
            .headers()
            .get(REQUEST_ID_HEADER)
            .context("response is missing x-request-id")?
            .to_str()?;
        Ok(Uuid::parse_str(header)?)
    }

    #[tokio::test]
    async fn responses_carry_server_generated_request_id() -> Result<()> {
        let request = Request::builder()
            .uri("/probe")
            .header(REQUEST_ID_HEADER, "client-chosen")
            .body(Body::empty())?;
        let response = probe_app().oneshot(request).await?;

        assert_eq!(response.status(), StatusCode::OK);
        let issued = issued_request_id(&response)?;
        assert_eq!(issued.get_version_num(), 7);
        let body = body::to_bytes(response.into_body(), 1024).await?;
        assert_eq!(std::str::from_utf8(&body)?, issued.to_string());
        Ok(())
    }

    #[tokio::test]
    async fn each_request_receives_a_distinct_request_id() -> Result<()> {
        let app = probe_app();
        let first = app
            .clone()
            .oneshot(Request::builder().uri("/probe").body(Body::empty())?)
            .await?;
        let second = app
            .oneshot(Request::builder().uri("/probe").body(Body::empty())?)
            .await?;
        assert_ne!(issued_request_id(&first)?, issued_request_id(&second)?);
        Ok(())
    }

    #[tokio::test]
    async fn unmatched_routes_still_carry_request_id() -> Result<()> {
        let response = probe_app()
            .oneshot(Request::builder().uri("/missing").body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        issued_request_id(&response)?;
        Ok(())
    }

    #[test]
    fn listener_rebinds_immediately_after_a_completed_connection() -> Result<()> {
        let (first, address) = bind_listener(0)?;
        let client = std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, address.port()))?;
        let (accepted, _) = first.accept()?;
        drop(accepted);
        drop(client);
        drop(first);

        let (replacement, replacement_address) = bind_listener(address.port())?;
        assert_eq!(replacement_address.port(), address.port());
        drop(replacement);
        Ok(())
    }

    #[test]
    fn listener_does_not_share_a_port_with_an_active_listener() -> Result<()> {
        let (first, address) = bind_listener(0)?;
        let duplicate = bind_listener(address.port());
        assert!(duplicate.is_err());
        drop(first);
        Ok(())
    }

    #[test]
    fn permission_denied_ipv6_socket_uses_ipv4_fallback() {
        let error = io::Error::from(ErrorKind::PermissionDenied);
        assert!(io_error_is_ipv6_unavailable(&error));
    }
}
