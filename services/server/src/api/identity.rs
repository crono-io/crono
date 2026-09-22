//! Development identity middleware.
//!
//! The transport creates a trusted context rather than accepting identity,
//! roles, or permissions from request data. Replacing this middleware with a
//! credential verifier later leaves handlers and use cases unchanged because
//! they already require the same [`RequestContext`] extension.

use crate::application::DevelopmentIdentity;
use axum::{extract::Request, middleware::Next, response::Response};

/// Attach the fixed development principal and a fresh correlation ID.
pub async fn establish(mut request: Request, next: Next) -> Response {
    request
        .extensions_mut()
        .insert(DevelopmentIdentity.context());
    next.run(request).await
}
