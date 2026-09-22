//! HTTP API router and process lifecycle.

use anyhow::{Context, Result};
use axum::Router;
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    io::{self, ErrorKind},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, TcpListener},
};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use utoipa_axum::router::OpenApiRouter;

pub(crate) mod handlers;
mod openapi;

pub use openapi::openapi;

/// Build the documented API router.
#[must_use]
pub fn router() -> OpenApiRouter {
    openapi::api_router()
}

/// Bind and serve the control-plane API until the process receives a shutdown signal.
///
/// # Errors
///
/// Returns an error when the listener cannot be created or the HTTP server fails.
pub async fn serve(port: u16) -> Result<()> {
    let (listener, listen_addr) = bind_listener(port)?;
    let listener = tokio::net::TcpListener::from_std(listener)
        .context("failed to create asynchronous API listener")?;
    let (router, _openapi) = router().split_for_parts();
    let app: Router = router.layer(TraceLayer::new_for_http());

    info!(address = %listen_addr, "Crono API listening over internal HTTP");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Crono API server failed")
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

fn bind_configured_socket(socket: Socket, address: SocketAddr) -> Result<TcpListener> {
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

    #[test]
    fn permission_denied_ipv6_socket_uses_ipv4_fallback() {
        let error = io::Error::from(ErrorKind::PermissionDenied);
        assert!(io_error_is_ipv6_unavailable(&error));
    }
}
