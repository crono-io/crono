//! `OpenAPI` document and route registration.

use super::handlers::health;
use utoipa::openapi::{InfoBuilder, License, OpenApiBuilder, Tag};
use utoipa_axum::{router::OpenApiRouter, routes};

/// Generate the `OpenAPI` document from the same routes used by the server.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    let (_router, openapi) = api_router().split_for_parts();
    openapi
}

/// Register every documented API route.
pub(crate) fn api_router() -> OpenApiRouter {
    let mut router = OpenApiRouter::with_openapi(cargo_openapi())
        .routes(routes!(health::live))
        .routes(routes!(health::ready))
        .routes(routes!(health::health));

    let mut health_tag = Tag::new("health");
    health_tag.description = Some("Process liveness, readiness, and health".to_string());
    router.get_openapi_mut().tags = Some(vec![health_tag]);

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
