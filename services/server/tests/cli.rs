//! Exercise CLI behavior through the actual process boundary.

use anyhow::Result;
use crono_server::cli::{
    actions::{Action, server},
    commands, dispatch,
};
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_crono-server"))
        .env_clear()
        .args(args)
        .output()?)
}

#[test]
fn help_and_version_do_not_initialize_telemetry() -> Result<()> {
    for flag in ["-h", "--help", "-V", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_crono-server"))
            .env_clear()
            .env("RUST_LOG", "[invalid")
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", "invalid endpoint")
            .arg(flag)
            .output()?;
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout)?;
        assert!(stdout.contains("crono-server"));
        assert!(
            !stdout.contains('\u{1b}'),
            "piped output must not contain ANSI escapes"
        );
        assert_eq!(output.stderr, Vec::<u8>::new());
    }
    Ok(())
}

#[test]
fn short_and_long_versions_report_build_identity() -> Result<()> {
    let output = invoke(&["-V"])?;
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        concat!("crono-server ", env!("CARGO_PKG_VERSION"))
    );
    let output = invoke(&["--version"])?;
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        format!(
            "crono-server {} - {}",
            env!("CARGO_PKG_VERSION"),
            crono_server::built_info::GIT_COMMIT_HASH.unwrap_or("unknown")
        )
    );
    Ok(())
}

#[test]
fn obsolete_commands_and_unknown_arguments_are_rejected() -> Result<()> {
    for args in [&["run"][..], &["unknown"][..], &["--unknown"][..]] {
        let output = invoke(args)?;
        assert_eq!(output.status.code(), Some(2));
        assert_ne!(output.stderr, Vec::<u8>::new());
    }
    Ok(())
}

#[test]
fn port_and_verbosity_dispatch_to_server_action() -> Result<()> {
    commands::new().debug_assert();
    let matches = commands::new().try_get_matches_from([
        "crono-server",
        "-vv",
        "--port",
        "9000",
        "--auth-mode",
        "development",
    ])?;
    assert_eq!(matches.get_count("verbose"), 2);
    assert_eq!(
        dispatch::handler(&matches)?,
        Action::Server(server::Args {
            port: 9000,
            auth_mode: crono_server::authentication::AuthMode::Development
        })
    );
    Ok(())
}

#[test]
fn port_uses_default_and_environment_values() -> Result<()> {
    temp_env::with_vars(
        [
            ("CRONO_AUTH_MODE", None::<&str>),
            ("CRONO_SERVER_PORT", None),
        ],
        || -> Result<()> {
            let matches = commands::new().try_get_matches_from(["crono-server"])?;
            assert_eq!(
                dispatch::handler(&matches)?,
                Action::Server(server::Args {
                    port: 8080,
                    auth_mode: crono_server::authentication::AuthMode::Development
                })
            );

            temp_env::with_var("CRONO_SERVER_PORT", Some("9001"), || -> Result<()> {
                let matches = commands::new().try_get_matches_from(["crono-server"])?;
                assert_eq!(
                    dispatch::handler(&matches)?,
                    Action::Server(server::Args {
                        port: 9001,
                        auth_mode: crono_server::authentication::AuthMode::Development
                    })
                );
                Ok(())
            })?;
            Ok(())
        },
    )
}

#[test]
fn authentication_modes_resolve_from_environment_and_command_line_without_loading_secrets()
-> Result<()> {
    use crono_server::authentication::AuthMode;
    temp_env::with_var("CRONO_AUTH_MODE", Some("oidc"), || -> Result<()> {
        let matches = commands::new().try_get_matches_from(["crono-server"])?;
        assert_eq!(
            matches.get_one::<AuthMode>("auth-mode"),
            Some(&AuthMode::Oidc)
        );
        let matches =
            commands::new().try_get_matches_from(["crono-server", "--auth-mode", "development"])?;
        assert_eq!(
            matches.get_one::<AuthMode>("auth-mode"),
            Some(&AuthMode::Development)
        );
        Ok(())
    })?;
    assert!(
        commands::new()
            .try_get_matches_from(["crono-server", "--auth-mode", "disabled"])
            .is_err()
    );
    Ok(())
}

#[test]
fn missing_development_token_and_unimplemented_oidc_fail_before_dependencies() -> Result<()> {
    for (args, message) in [
        (&[][..], "CRONO_AUTH_DEVELOPMENT_TOKEN is required"),
        (
            &["--auth-mode", "oidc"][..],
            "OIDC authentication is not implemented",
        ),
    ] {
        let output = invoke(args)?;
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains(message));
        assert!(!stderr.contains("failed to initialize PostgreSQL"));
    }
    Ok(())
}

#[test]
fn invalid_development_token_is_not_exposed_in_startup_logs() -> Result<()> {
    let secret = "sensitive-short-token";
    let output = Command::new(env!("CARGO_BIN_EXE_crono-server"))
        .env_clear()
        .env("CRONO_AUTH_DEVELOPMENT_TOKEN", secret)
        .args(["-vvv"])
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    for bytes in [output.stdout, output.stderr] {
        let text = String::from_utf8(bytes)?;
        assert!(!text.contains(secret));
    }
    Ok(())
}
