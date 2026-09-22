//! Verify the public command-line and client-configuration contracts.

use anyhow::Result;
use crono_cli::{
    built_info,
    cli::{actions::Action, commands, dispatch},
    client::{CronoClient, config::ClientConfig},
    config::{Config, DEFAULT_ADDRESS},
};
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_crono"))
        .env_clear()
        .args(args)
        .output()?)
}

#[test]
fn help_does_not_resolve_client_configuration() -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_crono"))
        .env_clear()
        .env("CRONO_ADDR", "not a URL")
        .env("NO_COLOR", "1")
        .arg("--help")
        .output()?;

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("Usage: crono"));
    assert!(stdout.contains("--address <URL>"));
    assert!(!stdout.contains('\u{1b}'));
    Ok(())
}

#[test]
fn short_and_long_versions_report_build_identity() -> Result<()> {
    let output = invoke(&["-V"])?;
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        concat!("crono ", env!("CARGO_PKG_VERSION"))
    );

    let output = invoke(&["--version"])?;
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        format!(
            "crono {} - {}",
            env!("CARGO_PKG_VERSION"),
            built_info::GIT_COMMIT_HASH.unwrap_or("unknown")
        )
    );
    Ok(())
}

#[test]
fn cli_address_overrides_environment() -> Result<()> {
    temp_env::with_var(
        "CRONO_ADDR",
        Some("https://environment.example.com"),
        || -> Result<()> {
            let matches = commands::new().try_get_matches_from([
                "crono",
                "--address",
                "https://explicit.example.com",
            ])?;
            let action = dispatch::handler(&matches)?;
            assert_eq!(
                action.client().base_url().as_str(),
                "https://explicit.example.com/"
            );
            Ok(())
        },
    )
}

#[test]
fn environment_address_overrides_default() -> Result<()> {
    temp_env::with_var(
        "CRONO_ADDR",
        Some("https://environment.example.com/base/"),
        || -> Result<()> {
            let config = Config::load(None)?;
            let client = CronoClient::new(config.into_client());
            assert_eq!(
                client.base_url().as_str(),
                "https://environment.example.com/base/"
            );
            Ok(())
        },
    )
}

#[test]
fn missing_address_uses_local_development_default() -> Result<()> {
    temp_env::with_var("CRONO_ADDR", None::<&str>, || -> Result<()> {
        let config = Config::load(None)?;
        let client = CronoClient::new(config.into_client());
        assert_eq!(client.base_url().as_str(), format!("{DEFAULT_ADDRESS}/"));
        Ok(())
    })
}

#[test]
fn malformed_and_unsupported_urls_are_rejected() {
    for address in [
        "not a URL",
        "http://",
        "http://crono.example.com",
        "file:///tmp/crono.sock",
        "nats://127.0.0.1:4222",
    ] {
        assert!(ClientConfig::parse(address).is_err(), "{address}");
    }
}

#[test]
fn urls_with_user_info_are_rejected() {
    for address in [
        "https://user@crono.example.com",
        "https://user:password@crono.example.com",
    ] {
        let result = ClientConfig::parse(address);
        assert!(result.is_err(), "{address}");
        if let Err(error) = result {
            assert!(error.to_string().contains("must not include user-info"));
        }
    }
}

#[test]
fn valid_http_and_https_urls_construct_clients_without_infrastructure() -> Result<()> {
    for (address, expected) in [
        ("http://127.0.0.1:1", "http://127.0.0.1:1/"),
        ("http://[::1]:8080", "http://[::1]:8080/"),
        (
            "https://crono.example.com/control-plane/",
            "https://crono.example.com/control-plane/",
        ),
    ] {
        let config = ClientConfig::parse(address)?;
        let client = CronoClient::new(config);
        assert_eq!(client.base_url().as_str(), expected);
    }
    Ok(())
}

#[test]
fn process_rejects_malformed_and_credential_bearing_addresses() -> Result<()> {
    for address in ["not a URL", "https://user:password@crono.example.com"] {
        let output = invoke(&["--address", address])?;
        assert_eq!(output.status.code(), Some(1));
        assert!(!output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn valid_configuration_reports_unfinished_commands_without_contacting_server() -> Result<()> {
    let output = invoke(&["--address", "https://127.0.0.1:1"])?;
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stderr.contains("operational commands are not implemented"));
    assert!(!stderr.contains("connection"));
    Ok(())
}

#[test]
fn dispatch_produces_the_explicit_unavailable_action() -> Result<()> {
    temp_env::with_var("CRONO_ADDR", None::<&str>, || -> Result<()> {
        let matches = commands::new().try_get_matches_from(["crono"])?;
        let action = dispatch::handler(&matches)?;
        assert!(matches!(action, Action::Unavailable(_)));
        Ok(())
    })
}
