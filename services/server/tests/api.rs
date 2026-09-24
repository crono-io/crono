//! Verify the public HTTP and `OpenAPI` contracts.

use anyhow::Result;
use serde_json::Value;
use std::process::Command;

#[test]
fn openapi_contains_health_and_control_plane_contracts() -> Result<()> {
    let document = crono_server::api::openapi();
    assert_eq!(document.info.title, "crono-server");
    assert_eq!(document.info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(document.paths.paths.len(), 20);
    for path in [
        "/live",
        "/ready",
        "/health",
        "/metrics",
        "/api/overview",
        "/api/namespaces",
        "/api/namespaces/{namespace_id}",
        "/api/queues",
        "/api/queues/{queue_id}",
        "/api/namespaces/{namespace_id}/jobs",
        "/api/jobs/{job_id}",
        "/api/namespaces/{namespace_id}/targets",
        "/api/targets/{target_id}",
        "/api/namespaces/{namespace_id}/target-sets",
        "/api/target-sets/{target_set_id}",
        "/api/namespaces/{namespace_id}/schedules",
        "/api/schedules/{schedule_id}",
        "/api/runs",
        "/api/runs/{run_id}",
        "/api/workers",
    ] {
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
