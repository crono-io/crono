//! Provider-independent verification of credentials before application execution.
//!
//! The HTTP adapter extracts an opaque credential; an injected [`AuthProvider`]
//! verifies it and returns the application-owned [`Principal`]. Providers never
//! access Crono resources or grant capabilities. Authorization remains a separate
//! application dependency, so JWT verification, introspection, or workload
//! credentials can later replace the development verifier without changing use cases.
//!
//! # Flow Overview
//!
//! The adapter validates credential syntax, the provider verifies authenticity,
//! and only success permits construction of a request context. Failures carry
//! no credential or provider detail. Development uses one configured secret;
//! there is no unauthenticated or arbitrary-token fallback.

use crate::application::{Principal, PrincipalKind};
use async_trait::async_trait;
use std::{error::Error, fmt};
use subtle::ConstantTimeEq;

mod config;
pub use config::{AuthConfig, AuthConfigError, AuthMode};

/// Bound credential allocation independently of the token's representation.
pub const MAX_BEARER_TOKEN_BYTES: usize = 8192;

/// Opaque Bearer value whose syntax is valid, but whose identity is unverified.
///
/// Debug output is redacted. Only authentication providers should inspect its
/// contents; parsing this type does not verify the token or grant authority.
pub struct BearerToken(String);

impl BearerToken {
    /// Validate the RFC 6750 token alphabet and an 8 KiB size bound.
    ///
    /// Tokens are case sensitive, contain no whitespace, and may have trailing
    /// `=` padding. No JWT, identity, role, or scope interpretation takes place.
    /// Errors contain no supplied value.
    ///
    /// # Errors
    /// Returns `InvalidCredentials` when syntax or size is invalid.
    pub fn new(value: String) -> Result<Self, AuthenticationError> {
        let unpadded = value.trim_end_matches('=');
        if value.len() > MAX_BEARER_TOKEN_BYTES
            || unpadded.is_empty()
            || !unpadded.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/')
            })
        {
            return Err(AuthenticationError::InvalidCredentials);
        }
        Ok(Self(value))
    }

    /// Expose the credential solely to the configured verifier; never log it.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerToken([REDACTED])")
    }
}

/// Transport-independent, unverified credentials supported by this server.
#[derive(Debug)]
pub enum RequestCredentials {
    Bearer(BearerToken),
}

/// Authentication rejection or verifier outage; neither establishes a principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationError {
    InvalidCredentials,
    Unavailable,
}

impl fmt::Display for AuthenticationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidCredentials => "valid authentication credentials are required",
            Self::Unavailable => "authentication provider is unavailable",
        })
    }
}

impl Error for AuthenticationError {}

/// Establish verified identity independently of HTTP and Crono authorization.
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Verify the credential and return only trusted, provider-neutral identity.
    ///
    /// A provider must reject invalid credentials and fail closed on dependency
    /// failures. It must never copy unverified identity/roles into a principal,
    /// log credentials, query Crono resources, or make capability decisions.
    ///
    /// # Errors
    /// Returns rejection for invalid credentials or unavailability for verifier failure.
    async fn authenticate(
        &self,
        credentials: &RequestCredentials,
    ) -> Result<Principal, AuthenticationError>;
}

/// Temporary verifier for one operator-configured development Bearer secret.
#[derive(Debug)]
pub struct DevelopmentAuthProvider {
    token: BearerToken,
}

impl DevelopmentAuthProvider {
    /// Require a syntactically valid secret of at least 32 bytes.
    ///
    /// Operators must generate a random token; length alone cannot guarantee
    /// entropy. Validation errors and Debug output never include the value.
    ///
    /// # Errors
    /// Returns a safe configuration error when the secret is too short.
    pub fn new(token: BearerToken) -> Result<Self, AuthConfigError> {
        if token.expose_secret().len() < 32 {
            return Err(AuthConfigError::InvalidDevelopmentToken);
        }
        Ok(Self { token })
    }
}

#[async_trait]
impl AuthProvider for DevelopmentAuthProvider {
    async fn authenticate(
        &self,
        credentials: &RequestCredentials,
    ) -> Result<Principal, AuthenticationError> {
        let RequestCredentials::Bearer(token) = credentials;
        // Content comparison is constant time for equal lengths. Token length
        // is not concealed; no content prefix affects the comparison duration.
        if !bool::from(
            token
                .expose_secret()
                .as_bytes()
                .ct_eq(self.token.expose_secret().as_bytes()),
        ) {
            return Err(AuthenticationError::InvalidCredentials);
        }
        Ok(Principal::new(
            "development/local".to_string(),
            PrincipalKind::Development,
        ))
    }
}

#[cfg(test)]
mod tests;
