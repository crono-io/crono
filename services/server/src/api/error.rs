//! Stable and non-sensitive mapping from failures to HTTP error envelopes.
//!
//! Every error response the API produces uses the same `ErrorEnvelope` JSON
//! body, whether a use case failed or the transport rejected the request
//! before any use case ran (malformed JSON, a non-UUID path segment, an
//! unknown route). Clients can therefore parse one shape and branch on its
//! stable `code`. Messages never include internal error details; transport
//! rejection messages describe only the caller's own input and are bounded
//! so a hostile request cannot inflate the response.

use crate::application::{ApplicationError, AuthorizationError};
use axum::{Json, http::StatusCode, response::IntoResponse};
use crono_api::{ErrorBody, ErrorEnvelope};
use tracing::error;

/// Longest transport rejection message returned to a caller, in characters.
const REJECTION_MESSAGE_MAX_CHARS: usize = 256;

/// HTTP-safe error response.
#[derive(Debug)]
pub enum ApiError {
    /// A use case failed; mapped by its stable application meaning.
    Application(ApplicationError),
    /// The transport refused the request before any use case ran.
    Rejected {
        status: StatusCode,
        code: &'static str,
        message: String,
    },
}

impl ApiError {
    /// Build a transport rejection, truncating the message to a safe length.
    pub fn rejected(status: StatusCode, code: &'static str, message: &str) -> Self {
        Self::Rejected {
            status,
            code,
            message: message.chars().take(REJECTION_MESSAGE_MAX_CHARS).collect(),
        }
    }
}

impl From<ApplicationError> for ApiError {
    fn from(value: ApplicationError) -> Self {
        Self::Application(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let application = match self {
            Self::Application(application) => application,
            Self::Rejected {
                status,
                code,
                message,
            } => return envelope(status, code, message, None),
        };
        let (status, code, message, field) = match &application {
            ApplicationError::InvalidInput { field, message } => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                message.clone(),
                field.map(str::to_string),
            ),
            ApplicationError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "resource was not found".to_string(),
                None,
            ),
            ApplicationError::Conflict => (
                StatusCode::CONFLICT,
                "already_exists",
                "resource already exists".to_string(),
                None,
            ),
            ApplicationError::InUse => (
                StatusCode::CONFLICT,
                "resource_in_use",
                "resource is still in use and cannot be deleted".to_string(),
                None,
            ),
            ApplicationError::IdempotencyConflict => (
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "request ID was reused with different inputs".to_string(),
                None,
            ),
            ApplicationError::Authorization(AuthorizationError::Unauthenticated) => (
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "authentication is required".to_string(),
                None,
            ),
            ApplicationError::Authorization(AuthorizationError::Forbidden) => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "operation is not authorized".to_string(),
                None,
            ),
            ApplicationError::Authorization(AuthorizationError::Unavailable)
            | ApplicationError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "dependency_unavailable",
                "a required service is unavailable".to_string(),
                None,
            ),
            ApplicationError::Internal => {
                error!("internal application failure returned by HTTP API");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error".to_string(),
                    None,
                )
            }
        };
        envelope(status, code, message, field)
    }
}

fn envelope(
    status: StatusCode,
    code: &str,
    message: String,
    field: Option<String>,
) -> axum::response::Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code: code.to_string(),
                message,
                field,
            },
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{ApiError, REJECTION_MESSAGE_MAX_CHARS};
    use axum::http::StatusCode;

    #[test]
    fn rejection_messages_are_bounded() {
        let long = "é".repeat(REJECTION_MESSAGE_MAX_CHARS * 4);
        let message = match ApiError::rejected(StatusCode::BAD_REQUEST, "invalid_request", &long) {
            ApiError::Rejected { message, .. } => Some(message),
            ApiError::Application(_) => None,
        };
        assert_eq!(
            message.map(|message| message.chars().count()),
            Some(REJECTION_MESSAGE_MAX_CHARS)
        );
    }
}
