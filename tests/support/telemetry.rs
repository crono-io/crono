//! Shared subprocess checks; every service exercises its own binary and metadata.

use anyhow::Result;
use std::time::Duration;
use tokio::{process::Command, time::timeout};

fn command() -> Command {
    let mut command = Command::new(super::BINARY);
    command
        .env_clear()
        .env("RUST_LOG", "info")
        .arg("run")
        .kill_on_drop(true);
    command
}

#[tokio::test]
async fn no_endpoint_keeps_local_logging_available() -> Result<()> {
    let output = timeout(Duration::from_secs(5), command().output()).await??;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("\"level\":\"ERROR\""));
    assert!(stderr.contains("runtime not implemented"));
    assert!(!stderr.contains("Telemetry shutdown failed"));
    Ok(())
}

#[cfg(not(feature = "telemetry"))]
#[tokio::test]
async fn default_build_ignores_exporter_configuration() -> Result<()> {
    let output = timeout(
        Duration::from_secs(5),
        command()
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", "invalid endpoint")
            .output(),
    )
    .await??;
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)?.contains("runtime not implemented"));
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
        net::TcpListener,
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
        let listener = TcpListener::bind("127.0.0.1:0").await?;
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
            command()
                .env("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{address}"))
                .env("OTEL_EXPORTER_OTLP_HEADERS", "x-crono-test=present")
                .output(),
        )
        .await??;
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("runtime not implemented"));
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
        for (key, expected) in [
            ("service.name", super::super::SERVICE),
            ("service.version", super::super::VERSION),
        ] {
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
                .any(|span| span.name == super::super::SPAN)
        );

        let _ = stop.send(());
        collector.await??;
        Ok(())
    }

    #[tokio::test]
    async fn configured_invalid_endpoint_fails_before_action() -> Result<()> {
        let output = command()
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", "invalid endpoint")
            .output()
            .await?;
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("invalid OTEL_EXPORTER_OTLP_ENDPOINT"));
        assert!(!stderr.contains("runtime not implemented"));
        Ok(())
    }

    #[tokio::test]
    async fn unresponsive_collector_cannot_prevent_process_exit() -> Result<()> {
        // A bound socket that never serves gRPC forces the exporter deadline.
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let output = timeout(
            Duration::from_secs(10),
            command()
                .env("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{address}"))
                .output(),
        )
        .await??;
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8(output.stderr)?.contains("runtime not implemented"));
        Ok(())
    }
}
