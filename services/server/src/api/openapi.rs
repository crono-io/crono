//! `OpenAPI` document and route registration.
//!
//! Routes are registered once through `utoipa-axum`, so the served router and
//! the published document cannot list different operations. Handler
//! annotations describe success bodies and operation-specific failures;
//! [`document_common_responses`] then adds the failures every operation shares
//! (authorization, dependency outages, request rejections) as reusable
//! `components.responses`. Keeping those in one place means a new handler
//! inherits a truthful error contract instead of silently omitting statuses
//! that contract tests such as Schemathesis would report.

use super::{
    handlers::{control_plane, health, monitor},
    state::AppState,
};
use utoipa::openapi::{
    Components, ContentBuilder, InfoBuilder, License, OpenApi, OpenApiBuilder, PathItem, Ref,
    RefOr, Response, ResponseBuilder, Tag, path::Operation,
};
use utoipa_axum::{router::OpenApiRouter, routes};

/// Shared failure responses as `(status, component name, description)`.
///
/// Every `/api` operation can return 401, 403, 500, and 503. Operations with
/// parameters or a body can also return 400 from request rejection, and
/// operations with a body can return 413 and 415.
const COMMON_RESPONSES: [(&str, &str, &str); 7] = [
    (
        "400",
        "InvalidRequest",
        "The request is invalid; `field` names the offending input when known.",
    ),
    ("401", "Unauthenticated", "Authentication is required."),
    (
        "403",
        "Forbidden",
        "The caller is not authorized for this operation.",
    ),
    (
        "413",
        "PayloadTooLarge",
        "The request body exceeds the 2 MiB limit.",
    ),
    (
        "415",
        "UnsupportedMediaType",
        "The request body must be sent with an `application/json` content type.",
    ),
    (
        "500",
        "InternalError",
        "The server failed unexpectedly; the response carries no internal detail.",
    ),
    (
        "503",
        "DependencyUnavailable",
        "A required dependency such as PostgreSQL is unavailable; retry later.",
    ),
];

/// Generate the `OpenAPI` document from the same routes used by the server.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let (_router, openapi) = api_router().split_for_parts();
    openapi
}

/// Register every documented API route.
pub(crate) fn api_router() -> OpenApiRouter<AppState> {
    let mut router = OpenApiRouter::with_openapi(cargo_openapi())
        .routes(routes!(health::live))
        .routes(routes!(health::ready))
        .routes(routes!(health::health))
        .routes(routes!(health::metrics))
        .routes(routes!(
            control_plane::create_namespace,
            control_plane::list_namespaces
        ))
        .routes(routes!(control_plane::get_namespace))
        .routes(routes!(
            control_plane::create_queue,
            control_plane::list_queues
        ))
        .routes(routes!(
            control_plane::get_queue,
            control_plane::update_queue,
            control_plane::delete_queue
        ))
        .routes(routes!(control_plane::create_job, control_plane::list_jobs))
        .routes(routes!(control_plane::get_job, control_plane::update_job))
        .routes(routes!(
            control_plane::create_target,
            control_plane::list_targets
        ))
        .routes(routes!(
            control_plane::get_target,
            control_plane::update_target
        ))
        .routes(routes!(
            control_plane::create_target_set,
            control_plane::list_target_sets
        ))
        .routes(routes!(
            control_plane::get_target_set,
            control_plane::update_target_set
        ))
        .routes(routes!(
            control_plane::create_schedule,
            control_plane::list_schedules
        ))
        .routes(routes!(
            control_plane::get_schedule,
            control_plane::update_schedule
        ))
        .routes(routes!(control_plane::create_run, control_plane::list_runs))
        .routes(routes!(control_plane::get_run))
        .routes(routes!(control_plane::rerun_run))
        .routes(routes!(control_plane::list_run_events))
        .routes(routes!(control_plane::list_run_attempts))
        .routes(routes!(control_plane::list_workers))
        .routes(routes!(control_plane::get_worker))
        .routes(routes!(control_plane::overview))
        .routes(routes!(monitor::monitor));

    let mut health_tag = Tag::new("health");
    health_tag.description = Some("Process liveness, readiness, and health".to_string());
    let mut control_plane_tag = Tag::new("control-plane");
    control_plane_tag.description =
        Some("Authorization-checked resources, Runs, and worker presence".to_string());
    router.get_openapi_mut().tags = Some(vec![health_tag, control_plane_tag]);
    document_common_responses(router.get_openapi_mut());

    router
}

/// Register the shared failure responses and reference them from operations.
///
/// Existing entries win, so an operation's own, more specific description of
/// a status is never replaced. Operational endpoints outside `/api` are
/// unauthenticated and document their own statuses.
fn document_common_responses(openapi: &mut OpenApi) {
    let components = openapi.components.get_or_insert_with(Components::default);
    for (_, name, description) in COMMON_RESPONSES {
        components
            .responses
            .insert(name.to_string(), RefOr::T(error_response(description)));
    }
    for (path, item) in &mut openapi.paths.paths {
        if !path.starts_with("/api/") {
            continue;
        }
        for operation in operations_mut(item) {
            let has_body = operation.request_body.is_some();
            let has_inputs = has_body
                || operation
                    .parameters
                    .as_ref()
                    .is_some_and(|parameters| !parameters.is_empty());
            for (status, name, _) in COMMON_RESPONSES {
                let applies = match status {
                    "400" => has_inputs,
                    "413" | "415" => has_body,
                    _ => true,
                };
                if applies {
                    operation
                        .responses
                        .responses
                        .entry(status.to_string())
                        .or_insert_with(|| RefOr::Ref(Ref::from_response_name(name)));
                }
            }
        }
    }
}

fn operations_mut(item: &mut PathItem) -> impl Iterator<Item = &mut Operation> {
    [
        &mut item.get,
        &mut item.put,
        &mut item.post,
        &mut item.delete,
        &mut item.options,
        &mut item.head,
        &mut item.patch,
        &mut item.trace,
    ]
    .into_iter()
    .flatten()
}

fn error_response(description: &str) -> Response {
    ResponseBuilder::new()
        .description(description)
        .content(
            "application/json",
            ContentBuilder::new()
                .schema(Some(Ref::from_schema_name("ErrorEnvelope")))
                .build(),
        )
        .build()
}

fn cargo_openapi() -> utoipa::openapi::OpenApi {
    let mut info = InfoBuilder::new()
        .title(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .description(Some(env!("CARGO_PKG_DESCRIPTION")))
        .build();
    let identifier = env!("CARGO_PKG_LICENSE");
    let mut license = License::new(identifier);
    license.identifier = Some(identifier.to_string());
    info.license = Some(license);

    OpenApiBuilder::new().info(info).build()
}
