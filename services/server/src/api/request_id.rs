//! Server-owned request correlation.
//!
//! Every HTTP request gets a fresh `UUIDv7` before any other middleware runs.
//! The same value is stored as a request extension, recorded on the request's
//! trace span, copied into the [`RequestContext`] that use cases receive, and
//! returned in the `x-request-id` response header. An operator can therefore
//! match a user's failing response to its log lines without asking for more
//! detail.
//!
//! A client-supplied `x-request-id` is never adopted: the correlation ID is a
//! server-issued value, and accepting caller input there would let one client
//! collide with or impersonate another request's log trail. A client value is
//! kept only as the separate `client_request_id` span field, and only when it
//! is at most 128 visible ASCII characters, so it cannot inject control
//! characters or unbounded data into logs. This header is unrelated to the
//! `request_id` idempotency key in Run creation and re-run request bodies.
//!
//! # Flow Overview
//!
//! 1. [`assign`] mints the ID and inserts [`RequestId`].
//! 2. [`make_span`] opens the trace span with method, matched route, and ID.
//! 3. The identity middleware builds the [`RequestContext`] from the ID.
//! 4. [`assign`] writes the ID into the response header on the way out, which
//!    also covers error responses produced by inner layers.
//!
//! [`RequestContext`]: crate::application::RequestContext

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tracing::{Span, field};
use uuid::Uuid;

/// Response header that carries the server-issued correlation ID.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Longest client-supplied request ID recorded in logs.
const CLIENT_REQUEST_ID_MAX_LEN: usize = 128;

/// Correlation ID issued by this server for one HTTP request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestId(Uuid);

impl RequestId {
    #[must_use]
    pub const fn get(self) -> Uuid {
        self.0
    }
}

/// Mint a request ID, expose it to inner layers, and echo it in the response.
///
/// Any `x-request-id` a handler or inner layer set is replaced so the header
/// always matches the logged ID.
pub async fn assign(mut request: Request, next: Next) -> Response {
    let request_id = RequestId(Uuid::now_v7());
    request.extensions_mut().insert(request_id);
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id.0.to_string()) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    response
}

/// Build the per-request trace span.
///
/// The route comes from [`MatchedPath`], such as `/api/runs/{run_id}`, rather
/// than the raw URI, so resource IDs and query strings stay out of the field.
/// Unmatched requests leave `http.route` empty.
pub fn make_span(request: &Request) -> Span {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str);
    let request_id = request
        .extensions()
        .get::<RequestId>()
        .map(|id| field::display(id.0));
    tracing::info_span!(
        "http.request",
        http.method = %request.method(),
        http.route = route,
        request_id = request_id,
        client_request_id = client_request_id(request.headers()),
    )
}

/// Return the caller's `x-request-id` when it is safe to log verbatim.
fn client_request_id(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= CLIENT_REQUEST_ID_MAX_LEN)
}

#[cfg(test)]
mod tests {
    use super::{CLIENT_REQUEST_ID_MAX_LEN, REQUEST_ID_HEADER, client_request_id};
    use anyhow::Result;
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn client_request_id_is_logged_only_when_bounded() -> Result<()> {
        let mut headers = HeaderMap::new();
        assert_eq!(client_request_id(&headers), None);

        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static("trace-42"));
        assert_eq!(client_request_id(&headers), Some("trace-42"));

        let oversized = "a".repeat(CLIENT_REQUEST_ID_MAX_LEN + 1);
        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_str(&oversized)?);
        assert_eq!(client_request_id(&headers), None);

        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_static(""));
        assert_eq!(client_request_id(&headers), None);

        headers.insert(REQUEST_ID_HEADER, HeaderValue::from_bytes(b"caf\xc3\xa9")?);
        assert_eq!(client_request_id(&headers), None);
        Ok(())
    }
}
