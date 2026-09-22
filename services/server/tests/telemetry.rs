//! Test server logging and trace export in an isolated process.

use anyhow::Result;
use std::{io::ErrorKind, net::TcpListener, time::Duration};
use tokio::{process::Command, time::timeout};

const BINARY: &str = env!("CARGO_BIN_EXE_crono-server");
#[cfg(feature = "telemetry")]
const SERVICE: &str = "crono-server";
#[cfg(feature = "telemetry")]
const SPAN: &str = "server.serve";
#[cfg(feature = "telemetry")]
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn blocked_port() -> Result<Option<(TcpListener, u16)>> {
    match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)) {
        Ok(listener) => {
            let port = listener.local_addr()?.port();
            Ok(Some((listener, port)))
        }
        Err(error) if error.kind() == ErrorKind::PermissionDenied => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn command(port: u16) -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("RUST_LOG", "info")
        .args(["--port", &port.to_string()])
        .kill_on_drop(true);
    command
}

#[tokio::test]
async fn no_endpoint_keeps_local_logging_available() -> Result<()> {
    let Some((_listener, port)) = blocked_port()? else {
        return Ok(());
    };
    let output = timeout(Duration::from_secs(5), command(port).output()).await??;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("\"level\":\"ERROR\""), "{stderr}");
    assert!(stderr.contains("failed to bind"), "{stderr}");
    assert!(!stderr.contains("Telemetry shutdown failed"), "{stderr}");
    Ok(())
}

#[cfg(not(feature = "telemetry"))]
#[tokio::test]
async fn default_build_ignores_exporter_configuration() -> Result<()> {
    let Some((_listener, port)) = blocked_port()? else {
        return Ok(());
    };
    let output = timeout(
        Duration::from_secs(5),
        command(port)
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", "invalid endpoint")
            .output(),
    )
    .await??;
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)?.contains("failed to bind"));
    Ok(())
}

#[cfg(feature = "telemetry")]
mod otlp {
    use super::*;
    use opentelemetry_proto::tonic::{
        collector::trace::v1::{
            ExportTraceServiceRequest, ExportTraceServiceResponse,
            trace_service_server::{TraceService, TraceServiceServer},
        },
        common::v1::any_value::Value,
    };
    use tokio::{
        net::TcpListener as TokioTcpListener,
        sync::{mpsc, oneshot},
    };
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status, codec::CompressionEncoding, transport::Server};

    struct Collector(mpsc::Sender<ExportTraceServiceRequest>);

    #[tonic::async_trait]
    impl TraceService for Collector {
        async fn export(
            &self,
            request: Request<ExportTraceServiceRequest>,
        ) -> Result<Response<ExportTraceServiceResponse>, Status> {
            if request
                .metadata()
                .get("x-crono-test")
                .and_then(|value| value.to_str().ok())
                != Some("present")
            {
                return Err(Status::unauthenticated("missing test header"));
            }
            self.0
                .send(request.into_inner())
                .await
                .map_err(|_| Status::unavailable("test collector closed"))?;
            Ok(Response::new(ExportTraceServiceResponse::default()))
        }
    }

    #[tokio::test]
    async fn exports_service_metadata_and_flushes_on_action_error() -> Result<()> {
        let Some((_blocked, port)) = blocked_port()? else {
            return Ok(());
        };
        let listener = TokioTcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (sender, mut receiver) = mpsc::channel(8);
        let (stop, stopped) = oneshot::channel();
        let collector = tokio::spawn(async move {
            Server::builder()
                .add_service(
                    TraceServiceServer::new(Collector(sender))
                        .accept_compressed(CompressionEncoding::Gzip),
                )
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                })
                .await
        });
        let output = timeout(
            Duration::from_secs(10),
            command(port)
                .env("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{address}"))
                .env("OTEL_EXPORTER_OTLP_HEADERS", "x-crono-test=present")
                .output(),
        )
        .await??;
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("failed to bind"), "{stderr}");
        assert!(!stderr.contains("Telemetry shutdown failed"), "{stderr}");

        let request = timeout(Duration::from_secs(2), receiver.recv())
            .await?
            .ok_or_else(|| anyhow::anyhow!("no spans exported before exit"))?;
        let resource_spans = request
            .resource_spans
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing resource spans"))?;
        let resource = resource_spans
            .resource
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing service resource"))?;
        for (key, expected) in [("service.name", SERVICE), ("service.version", VERSION)] {
            let actual = resource
                .attributes
                .iter()
                .find(|attribute| attribute.key == key)
                .and_then(|attribute| attribute.value.as_ref())
                .and_then(|value| value.value.as_ref());
            assert_eq!(actual, Some(&Value::StringValue(expected.to_owned())));
        }
        assert!(
            resource_spans
                .scope_spans
                .iter()
                .flat_map(|scope| &scope.spans)
                .any(|span| span.name == SPAN)
        );

        let _ = stop.send(());
        collector.await??;
        Ok(())
    }

    #[tokio::test]
    async fn configured_invalid_endpoint_fails_before_action() -> Result<()> {
        let output = command(0)
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", "invalid endpoint")
            .output()
            .await?;
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("invalid OTEL_EXPORTER_OTLP_ENDPOINT"));
        assert!(!stderr.contains("failed to bind"));
        Ok(())
    }

    #[tokio::test]
    async fn unresponsive_collector_cannot_prevent_process_exit() -> Result<()> {
        let Some((_blocked, port)) = blocked_port()? else {
            return Ok(());
        };
        let listener = TokioTcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let output = timeout(
            Duration::from_secs(10),
            command(port)
                .env("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{address}"))
                .output(),
        )
        .await??;
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8(output.stderr)?.contains("failed to bind"));
        drop(listener);
        Ok(())
    }
}
