//! Request extractors that reject malformed input with the API error envelope.
//!
//! Axum's `Json`, `Path`, and `Query` extractors answer bad input with
//! plain-text bodies and a spread of statuses (400, 413, 415, 422). These
//! wrappers delegate to them unchanged and only translate the rejection, so
//! every error a client sees is the same `ErrorEnvelope` JSON the use cases
//! return. Syntax and data errors collapse to `400 invalid_request`, matching
//! how application-level validation already reports bad input; body-size and
//! content-type failures keep their distinct statuses because a client fixes
//! them differently. A 5xx rejection means the extractor itself was misused by
//! the server (for example a path type that cannot match the route), so it is
//! logged and reported as an internal error instead of blaming the caller.
//!
//! Handlers must still declare their parameters in `#[utoipa::path]`. utoipa's
//! axum integration recognizes axum extractors by type name, and in particular
//! only marks `IntoParams` structs as query parameters because the argument is
//! literally `Query<T>`; its default location is the path. Query structs used
//! with [`ApiQuery`] therefore carry `#[into_params(parameter_in = Query)]`, and
//! a test asserts the generated document keeps them in the query string.

use super::error::ApiError;
use crate::application::ApplicationError;
use axum::{
    Json,
    extract::{
        FromRequest, FromRequestParts, Path, Query, Request,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{StatusCode, request::Parts},
};
use tracing::error;

/// JSON request body whose rejection is an [`ApiError`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiJson<T>(pub T);

/// Path parameters whose rejection is an [`ApiError`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiPath<T>(pub T);

/// Query-string parameters whose rejection is an [`ApiError`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ApiQuery<T>(pub T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| rejected(rejection.status(), &rejection.body_text()))
    }
}

impl<T, S> FromRequestParts<S> for ApiPath<T>
where
    Path<T>: FromRequestParts<S, Rejection = PathRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|rejection| rejected(rejection.status(), &rejection.body_text()))
    }
}

impl<T, S> FromRequestParts<S> for ApiQuery<T>
where
    Query<T>: FromRequestParts<S, Rejection = QueryRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|rejection| rejected(rejection.status(), &rejection.body_text()))
    }
}

/// Classify an axum rejection by status; its enums are non-exhaustive.
fn rejected(status: StatusCode, detail: &str) -> ApiError {
    match status {
        StatusCode::PAYLOAD_TOO_LARGE => ApiError::rejected(status, "payload_too_large", detail),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            ApiError::rejected(status, "unsupported_media_type", detail)
        }
        status if status.is_server_error() => {
            error!(%status, detail, "request extractor failed on the server side");
            ApiError::Application(ApplicationError::Internal)
        }
        _ => ApiError::rejected(StatusCode::BAD_REQUEST, "invalid_request", detail),
    }
}
