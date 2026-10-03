//! Verify credential syntax, secret redaction, and fail-closed provider configuration.

use super::*;
use anyhow::Result;

const TEST_TOKEN: &str = "test-only-development-token-1234567890";

#[tokio::test]
async fn only_the_configured_token_establishes_the_fixed_development_identity() -> Result<()> {
    let provider = DevelopmentAuthProvider::new(BearerToken::new(TEST_TOKEN.to_string())?)?;
    let caller = provider
        .authenticate(&RequestCredentials::Bearer(BearerToken::new(
            TEST_TOKEN.to_string(),
        )?))
        .await?;
    let principal = caller.principal();
    assert_eq!(principal.id(), "development/local");
    assert_eq!(principal.issuer(), None);
    assert_eq!(principal.kind(), PrincipalKind::Development);
    assert_eq!(caller.grants(), &GrantSet::development());
    for value in [
        "arbitrary",
        "development/local",
        "test-only-development-token-1234567891",
        "TEST-only-development-token-1234567890",
    ] {
        assert_eq!(
            provider
                .authenticate(&RequestCredentials::Bearer(BearerToken::new(
                    value.to_string()
                )?))
                .await,
            Err(AuthenticationError::InvalidCredentials)
        );
    }
    Ok(())
}

#[test]
fn credentials_config_and_provider_debug_output_redact_secrets() -> Result<()> {
    let credential = RequestCredentials::Bearer(BearerToken::new(TEST_TOKEN.to_string())?);
    let provider = DevelopmentAuthProvider::new(BearerToken::new(TEST_TOKEN.to_string())?)?;
    for output in [
        format!("{credential:?}"),
        format!("{provider:?}"),
        format!("{:?}", AuthConfig::Development(provider)),
    ] {
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains(TEST_TOKEN));
    }
    for error in [
        AuthConfigError::MissingDevelopmentToken,
        AuthConfigError::InvalidDevelopmentToken,
        AuthConfigError::OidcNotImplemented,
    ] {
        assert!(!error.to_string().contains(TEST_TOKEN));
    }
    Ok(())
}

#[test]
fn bearer_syntax_is_bounded_and_does_not_assume_jwt() -> Result<()> {
    for token in ["opaque", "Abc-._~+/09==", "a.b.c"] {
        assert_eq!(BearerToken::new(token.to_string())?.expose_secret(), token);
    }
    for token in [
        "", "=", "a=b", "value ", " value", "a,b", "a\tb", "a\nb", "café",
    ] {
        assert!(matches!(
            BearerToken::new(token.to_string()),
            Err(AuthenticationError::InvalidCredentials)
        ));
    }
    assert!(BearerToken::new("a".repeat(MAX_BEARER_TOKEN_BYTES)).is_ok());
    assert!(BearerToken::new("a".repeat(MAX_BEARER_TOKEN_BYTES + 1)).is_err());
    Ok(())
}

#[test]
fn startup_configuration_requires_a_valid_secret_and_never_falls_back_from_oidc() -> Result<()> {
    temp_env::with_var("CRONO_AUTH_DEVELOPMENT_TOKEN", None::<&str>, || {
        assert!(matches!(
            AuthConfig::from_env(AuthMode::Development),
            Err(AuthConfigError::MissingDevelopmentToken)
        ));
        assert!(matches!(
            AuthConfig::from_env(AuthMode::Oidc),
            Err(AuthConfigError::OidcNotImplemented)
        ));
    });
    for token in [
        "",
        "short",
        "bad secret with whitespace that is long enough",
        "non-ascii-café-that-is-long-enough",
    ] {
        temp_env::with_var("CRONO_AUTH_DEVELOPMENT_TOKEN", Some(token), || {
            assert!(matches!(
                AuthConfig::from_env(AuthMode::Development),
                Err(AuthConfigError::InvalidDevelopmentToken)
            ));
        });
    }
    temp_env::with_var(
        "CRONO_AUTH_DEVELOPMENT_TOKEN",
        Some(TEST_TOKEN),
        || -> Result<()> {
            assert!(matches!(
                AuthConfig::from_env(AuthMode::Development)?,
                AuthConfig::Development(_)
            ));
            assert!(matches!(
                AuthConfig::from_env(AuthMode::Oidc),
                Err(AuthConfigError::OidcNotImplemented)
            ));
            Ok(())
        },
    )
}

#[test]
fn external_subjects_are_scoped_to_verified_issuers_and_support_service_identities() {
    let first = Principal::from_issuer(
        "https://first.example".to_string(),
        "service_123".to_string(),
        PrincipalKind::Service,
    );
    let second = Principal::from_issuer(
        "https://second.example".to_string(),
        "service_123".to_string(),
        PrincipalKind::Service,
    );
    assert_ne!(first, second);
    assert_eq!(first.id(), "service_123");
    assert_eq!(first.issuer(), Some("https://first.example"));
}
