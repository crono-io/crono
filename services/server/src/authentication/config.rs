//! Authentication startup configuration, kept separate from HTTP and policy.
//!
//! The CLI selects the mode; the action loads its secret from the environment
//! before opening dependencies or a listener. Development requires a configured
//! token. OIDC is a reserved mode that fails explicitly until a provider exists;
//! future provider configuration belongs here rather than in handlers or domain data.

use super::{AuthProvider, BearerToken, DevelopmentAuthProvider};
use std::{env, error::Error, fmt, str::FromStr, sync::Arc};

/// Provider selection; unsupported modes cannot silently enable development access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Development,
    Oidc,
}

impl FromStr for AuthMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "development" => Ok(Self::Development),
            "oidc" => Ok(Self::Oidc),
            _ => Err("expected development or oidc"),
        }
    }
}

/// Validated provider configuration; secret Debug output is redacted.
#[derive(Debug)]
pub enum AuthConfig {
    Development(DevelopmentAuthProvider),
}

impl AuthConfig {
    /// Load the selected provider without exposing environment values in errors.
    ///
    /// OIDC selection fails before reading a development token. Missing,
    /// non-Unicode, short, or invalid secrets fail startup; there is no default.
    ///
    /// # Errors
    /// Returns a safe error for invalid/missing credentials or unimplemented OIDC.
    pub fn from_env(mode: AuthMode) -> Result<Self, AuthConfigError> {
        match mode {
            AuthMode::Development => {
                let value = env::var("CRONO_AUTH_DEVELOPMENT_TOKEN")
                    .map_err(|_| AuthConfigError::MissingDevelopmentToken)?;
                let token = BearerToken::new(value)
                    .map_err(|_| AuthConfigError::InvalidDevelopmentToken)?;
                Ok(Self::Development(DevelopmentAuthProvider::new(token)?))
            }
            AuthMode::Oidc => Err(AuthConfigError::OidcNotImplemented),
        }
    }

    /// Construct the independently injectable verifier; no Authorizer is selected here.
    #[must_use]
    pub fn into_provider(self) -> Arc<dyn AuthProvider> {
        match self {
            Self::Development(provider) => Arc::new(provider),
        }
    }
}

/// Safe startup failures that never contain a credential value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthConfigError {
    MissingDevelopmentToken,
    InvalidDevelopmentToken,
    OidcNotImplemented,
}

impl fmt::Display for AuthConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingDevelopmentToken => {
                "CRONO_AUTH_DEVELOPMENT_TOKEN is required in development mode"
            }
            Self::InvalidDevelopmentToken => {
                "CRONO_AUTH_DEVELOPMENT_TOKEN must be a random Bearer token of 32 to 8192 bytes"
            }
            Self::OidcNotImplemented => {
                "OIDC authentication is not implemented; select development with a configured token"
            }
        })
    }
}

impl Error for AuthConfigError {}
