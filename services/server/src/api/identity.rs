//! Development identity middleware.
//!
//! The transport creates a trusted context rather than accepting identity,
//! roles, or permissions from request data. Replacing this middleware with a
//! credential verifier later leaves handlers and use cases unchanged because
//! they already require the same [`RequestContext`] extension.
//!
//! [`RequestContext`]: crate::application::RequestContext

use super::{error::ApiError, request_id::RequestId};
use crate::application::{ApplicationError, DevelopmentIdentity};
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::error;

/// Attach the fixed development principal under the server-issued request ID.
///
/// The request ID must already be present from the outer correlation layer,
/// so the ID in the context, the trace span, and the `x-request-id` header
/// are the same value. A missing ID means the layers were composed in the
/// wrong order; the request fails with a 500 rather than run under an
/// uncorrelated ID.
pub async fn establish(mut request: Request, next: Next) -> Response {
    let Some(request_id) = request.extensions().get::<RequestId>().copied() else {
        error!("identity middleware ran before a request ID was assigned");
        return ApiError::from(ApplicationError::Internal).into_response();
    };
    request
        .extensions_mut()
        .insert(DevelopmentIdentity.context(request_id.get()));
    next.run(request).await
}
