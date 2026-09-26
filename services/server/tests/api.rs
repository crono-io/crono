//! Verify the public HTTP and `OpenAPI` contracts.

use anyhow::Result;
use serde_json::Value;
use std::{path::Path, process::Command};

#[test]
fn openapi_contains_health_and_control_plane_contracts() -> Result<()> {
    let document = crono_server::api::openapi();
    assert_eq!(document.info.title, "crono-server");
    assert_eq!(document.info.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(document.paths.paths.len(), 25);
    for path in [
        "/live",
        "/ready",
        "/health",
        "/metrics",
        "/api/overview",
        "/api/monitor",
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
        "/api/runs/{run_id}/rerun",
        "/api/runs/{run_id}/events",
        "/api/runs/{run_id}/attempts",
        "/api/workers",
        "/api/workers/{worker_id}",
    ] {
        assert!(document.paths.paths.contains_key(path));
    }
    assert!(
        document
            .components
            .as_ref()
            .is_some_and(|components| components.schemas.contains_key("Health"))
    );
    assert!(
        document
            .components
            .as_ref()
            .is_some_and(|components| components.schemas.contains_key("MonitorResource"))
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

/// Iterate `(path, method, operation)` for every operation in the document.
fn operations(document: &Value) -> Vec<(&str, &str, &Value)> {
    document
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .flat_map(|(path, item)| {
            item.as_object()
                .into_iter()
                .flatten()
                .map(move |(method, operation)| (path.as_str(), method.as_str(), operation))
        })
        .collect()
}

/// Wrapper extractors hide axum's `Query` type from utoipa, so query structs
/// must declare their location explicitly; a path-located `limit` or cursor
/// would describe a different API than the server implements.
#[test]
fn list_query_parameters_are_documented_in_query() -> Result<()> {
    let document = serde_json::to_value(crono_server::api::openapi())?;
    let mut checked = 0;
    for (path, method, operation) in operations(&document) {
        let parameters = operation
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for parameter in parameters {
            let name = parameter.get("name").and_then(Value::as_str).unwrap_or("");
            let location = parameter.get("in").and_then(Value::as_str).unwrap_or("");
            if location == "path" {
                assert!(
                    path.contains(&format!("{{{name}}}")),
                    "{method} {path} declares path parameter {name} missing from its route"
                );
            }
            if matches!(name, "limit" | "after" | "before") {
                assert_eq!(location, "query", "{method} {path} parameter {name}");
                checked += 1;
            }
        }
    }
    // Seven name-cursor lists expose `limit` and `after`; Runs expose `limit` and `before`.
    assert_eq!(
        checked, 16,
        "every list endpoint exposes its paging parameters"
    );
    Ok(())
}

fn generated_document() -> Result<Value> {
    Ok(serde_json::to_value(crono_server::api::openapi())?)
}

fn documented_statuses(operation: &Value) -> Vec<&str> {
    operation
        .get("responses")
        .and_then(Value::as_object)
        .map(|responses| responses.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

fn has_inputs(operation: &Value) -> bool {
    operation.get("requestBody").is_some()
        || operation
            .get("parameters")
            .and_then(Value::as_array)
            .is_some_and(|parameters| !parameters.is_empty())
}

#[test]
fn every_api_operation_documents_authorization_and_dependency_failures() -> Result<()> {
    let document = generated_document()?;
    for (path, method, operation) in operations(&document) {
        if !path.starts_with("/api/") {
            continue;
        }
        let statuses = documented_statuses(operation);
        for status in ["401", "403", "500", "503"] {
            assert!(
                statuses.contains(&status),
                "{method} {path} does not document {status}"
            );
        }
    }
    Ok(())
}

#[test]
fn operations_with_inputs_document_invalid_request() -> Result<()> {
    let document = generated_document()?;
    for (path, method, operation) in operations(&document) {
        if path.starts_with("/api/") && has_inputs(operation) {
            assert!(
                documented_statuses(operation).contains(&"400"),
                "{method} {path} accepts input but does not document 400"
            );
        }
    }
    Ok(())
}

#[test]
fn operations_with_bodies_document_payload_and_media_type_failures() -> Result<()> {
    let document = generated_document()?;
    for (path, method, operation) in operations(&document) {
        if operation.get("requestBody").is_some() {
            let statuses = documented_statuses(operation);
            for status in ["413", "415"] {
                assert!(
                    statuses.contains(&status),
                    "{method} {path} accepts a body but does not document {status}"
                );
            }
        }
    }
    Ok(())
}

/// `OpenAPI` 3.1 requires a description on every response object.
#[test]
fn every_response_has_a_description() -> Result<()> {
    let document = generated_document()?;
    let components = document.pointer("/components/responses");
    for (path, method, operation) in operations(&document) {
        let responses = operation
            .get("responses")
            .and_then(Value::as_object)
            .into_iter()
            .flatten();
        for (status, response) in responses {
            let resolved = match response.get("$ref").and_then(Value::as_str) {
                Some(reference) => reference
                    .strip_prefix("#/components/responses/")
                    .and_then(|name| components.and_then(|components| components.get(name))),
                None => Some(response),
            };
            assert!(
                resolved
                    .and_then(|response| response.get("description"))
                    .and_then(Value::as_str)
                    .is_some_and(|description| !description.is_empty()),
                "{method} {path} response {status} has no description"
            );
        }
    }
    Ok(())
}

#[test]
fn list_limit_parameters_declare_documented_bounds() -> Result<()> {
    let document = generated_document()?;
    let mut checked = 0;
    for (path, method, operation) in operations(&document) {
        let parameters = operation
            .get("parameters")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for parameter in parameters {
            if parameter.get("name").and_then(Value::as_str) == Some("limit") {
                let schema = parameter.get("schema");
                let bound = |key: &str| {
                    schema
                        .and_then(|schema| schema.get(key))
                        .and_then(Value::as_u64)
                };
                assert_eq!(bound("minimum"), Some(1), "{method} {path} limit minimum");
                assert_eq!(bound("maximum"), Some(100), "{method} {path} limit maximum");
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 8, "every list endpoint documents its limit");
    Ok(())
}

/// Request names are validated as DNS-1123 labels; the schema must say so, or
/// clients and fuzzers will send values the server always rejects.
#[test]
fn request_names_declare_resource_name_constraints() -> Result<()> {
    let document = generated_document()?;
    let schemas = document
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .into_iter()
        .flatten();
    let mut checked = 0;
    for (schema_name, schema) in schemas {
        if !schema_name.ends_with("Request") {
            continue;
        }
        if let Some(name) = schema.pointer("/properties/name") {
            assert_eq!(
                name.get("maxLength").and_then(Value::as_u64),
                u64::try_from(crono_api::RESOURCE_NAME_MAX_LENGTH).ok(),
                "{schema_name}.name maxLength"
            );
            assert_eq!(
                name.get("pattern").and_then(Value::as_str),
                Some("^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"),
                "{schema_name}.name pattern"
            );
            checked += 1;
        }
    }
    assert_eq!(
        checked, 10,
        "every named create or update request is constrained"
    );
    Ok(())
}

/// The committed contract is what docs, oasdiff, and Schemathesis consume, so
/// it must be byte-identical to the document generated from the routes.
#[test]
fn committed_openapi_document_matches_generated_output() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_crono-server-openapi")).output()?;
    assert!(output.status.success());
    let committed = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/openapi/crono-server.json"),
    )?;
    assert!(
        output.stdout == committed,
        "docs/openapi/crono-server.json is out of date; run `just openapi` and commit the result"
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
