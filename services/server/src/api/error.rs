//! Stable and non-sensitive mapping from application failures to HTTP.

use crate::application::{ApplicationError, AuthorizationError};
use axum::{Json, http::StatusCode, response::IntoResponse};
use crono_api::{ErrorBody, ErrorEnvelope};
use tracing::error;

/// HTTP-safe application error response.
#[derive(Debug)]
pub struct ApiError(ApplicationError);

impl From<ApplicationError> for ApiError {
    fn from(value: ApplicationError) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, message) = match &self.0 {
            ApplicationError::InvalidInput(message) => {
                (StatusCode::BAD_REQUEST, "invalid_request", message.clone())
            }
            ApplicationError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource was not found".to_string(),
            ),
            ApplicationError::Conflict => (
                StatusCode::CONFLICT,
                "already_exists",
                "resource already exists".to_string(),
            ),
            ApplicationError::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "request ID was reused with different inputs".to_string(),
            ),
            ApplicationError::Authorization(AuthorizationError::Unauthenticated) => (
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "authentication is required".to_string(),
            ),
            ApplicationError::Authorization(AuthorizationError::Forbidden) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "operation is not authorized".to_string(),
            ),
            ApplicationError::Authorization(AuthorizationError::Unavailable)
            | ApplicationError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "dependency_unavailable",
                "a required service is unavailable".to_string(),
            ),
            ApplicationError::Internal => {
                error!("internal application failure returned by HTTP API");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error".to_string(),
                )
            }
        };
        (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: code.to_string(),
                    message,
                },
            }),
        )
            .into_response()
    }
}
