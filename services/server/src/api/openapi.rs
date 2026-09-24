//! `OpenAPI` document and route registration.

use super::{
    handlers::{control_plane, health},
    state::AppState,
};
use utoipa::openapi::{InfoBuilder, License, OpenApiBuilder, Tag};
use utoipa_axum::{router::OpenApiRouter, routes};

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
        .routes(routes!(control_plane::list_workers))
        .routes(routes!(control_plane::overview));

    let mut health_tag = Tag::new("health");
    health_tag.description = Some("Process liveness, readiness, and health".to_string());
    let mut control_plane_tag = Tag::new("control-plane");
    control_plane_tag.description =
        Some("Authorization-checked resources, Runs, and worker presence".to_string());
    router.get_openapi_mut().tags = Some(vec![health_tag, control_plane_tag]);

    router
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
