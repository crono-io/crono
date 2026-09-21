//! Optional native exporter implementation, excluded from default builds.

use anyhow::{Context, Result, bail};
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::{
    Compression, WithExportConfig, WithTonicConfig, tonic_types::transport::ClientTlsConfig,
};
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use std::{env, sync::OnceLock, time::Duration};
use url::Url;

static PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// Validate without including potentially sensitive configuration in errors.
fn endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("invalid OTEL_EXPORTER_OTLP_ENDPOINT URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("OTEL_EXPORTER_OTLP_ENDPOINT requires an http:// or https:// host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("use OTEL_EXPORTER_OTLP_HEADERS for credentials, not endpoint user information");
    }
    Ok(url)
}

/// Build only after the application explicitly opts into export at startup.
pub(super) fn provider(
    service_name: &'static str,
    service_version: &'static str,
) -> Result<Option<SdkTracerProvider>> {
    let value = match env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(_) => bail!("OTEL_EXPORTER_OTLP_ENDPOINT must contain Unicode text"),
    };
    let endpoint = endpoint(&value)?;
    for key in [
        "OTEL_EXPORTER_OTLP_PROTOCOL",
        "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
    ] {
        match env::var(key) {
            Ok(value) if value == "grpc" => {}
            Err(env::VarError::NotPresent) => {}
            _ => bail!("{key} must be grpc; only gRPC trace export is supported"),
        }
    }

    // The SDK handles standard header environment variables. Its TLS transport
    // verifies HTTPS hosts using the enabled WebPKI trust roots.
    let mut builder = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.as_str())
        .with_compression(Compression::Gzip)
        .with_timeout(Duration::from_secs(3));
    if endpoint.scheme() == "https" {
        builder = builder.with_tls_config(ClientTlsConfig::new().with_webpki_roots());
    }
    let exporter = builder
        .build()
        .context("could not initialize the OTLP trace exporter")?;
    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attribute(KeyValue::new("service.version", service_version))
        .build();
    Ok(Some(
        SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(exporter)
            .build(),
    ))
}

/// Retain the provider for explicit shutdown after the action completes.
pub(super) fn install(provider: SdkTracerProvider) -> Result<()> {
    PROVIDER
        .set(provider.clone())
        .map_err(|_| anyhow::anyhow!("trace provider already initialized"))?;
    global::set_tracer_provider(provider);
    global::set_text_map_propagator(TraceContextPropagator::new());
    Ok(())
}

/// Bound the wait and propagate errors instead of claiming traces were sent.
pub(super) fn shutdown() -> Result<()> {
    if let Some(provider) = PROVIDER.get() {
        provider.shutdown_with_timeout(Duration::from_secs(5))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_accepts_http_https_and_ipv6() -> Result<()> {
        for value in [
            "http://localhost:4317",
            "https://collector.example:4317",
            "https://[::1]:4317",
        ] {
            assert!(endpoint(value)?.host_str().is_some());
        }
        Ok(())
    }

    #[test]
    fn endpoint_rejects_missing_hosts_schemes_and_credentials() {
        for value in [
            "",
            "localhost:4317",
            "ftp://localhost",
            "https://",
            "https://user:secret@localhost",
        ] {
            assert!(endpoint(value).is_err());
        }
    }
}
