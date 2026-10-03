//! HTTP credential extraction and verified request-context construction.
//!
//! Only the Authorization header carries credentials. A single case-insensitive
//! Bearer scheme with one or more ASCII spaces is accepted; token syntax is
//! checked without interpreting identity or claims. Duplicate headers, combined
//! values, malformed credentials, and verifier errors fail closed before handlers.
//! The four operational endpoints are public; they perform no application use case.
//!
//! # Flow Overview
//!
//! Request correlation runs first. Protected requests extract credentials, call
//! the injected provider, then construct `RequestContext` from its verified
//! caller (identity and normalized grants) and the server-issued ID. Every authorization decision
//! still runs normally. No credential, role, or identity header is logged here.

use super::{error::ApiError, request_id::RequestId};
use crate::{
    application::{ApplicationError, RequestContext},
    authentication::{AuthProvider, AuthenticationError, BearerToken, RequestCredentials},
};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, header::AUTHORIZATION},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;

/// Authenticate before attaching trusted identity and grants; failures never invoke handlers.
///
/// Only GET/HEAD on the exact public probe/metrics paths bypass authentication.
/// A missing server-issued correlation ID is a composition error and fails with 500.
pub async fn establish(
    State(provider): State<Arc<dyn AuthProvider>>,
    mut request: Request,
    next: Next,
) -> Response {
    if matches!(*request.method(), Method::GET | Method::HEAD)
        && matches!(
            request.uri().path(),
            "/live" | "/ready" | "/health" | "/metrics"
        )
    {
        return next.run(request).await;
    }
    let Some(request_id) = request.extensions().get::<RequestId>().copied() else {
        tracing::error!("authentication middleware ran before request correlation");
        return ApiError::from(ApplicationError::Internal).into_response();
    };
    let caller = match credentials(request.headers()) {
        Ok(credentials) => provider.authenticate(&credentials).await,
        Err(error) => Err(error),
    };
    match caller {
        Ok(caller) => {
            request
                .extensions_mut()
                .insert(RequestContext::new(request_id.get(), caller));
            next.run(request).await
        }
        Err(error) => ApiError::Authentication(error).into_response(),
    }
}

/// Parse exactly one Authorization header, preserving case-sensitive token bytes.
///
/// The syntax check establishes no authority. Body/query credentials and all
/// client-supplied principal/role/capability metadata are ignored by this adapter.
fn credentials(headers: &HeaderMap) -> Result<RequestCredentials, AuthenticationError> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values
        .next()
        .ok_or(AuthenticationError::InvalidCredentials)?;
    if values.next().is_some() {
        return Err(AuthenticationError::InvalidCredentials);
    }
    let value = value
        .to_str()
        .map_err(|_| AuthenticationError::InvalidCredentials)?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or(AuthenticationError::InvalidCredentials)?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(AuthenticationError::InvalidCredentials);
    }
    BearerToken::new(token.trim_start_matches(' ').to_string()).map(RequestCredentials::Bearer)
}

#[cfg(test)]
mod tests;
