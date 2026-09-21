//! Exercise CLI behavior through the actual process boundary.

use anyhow::Result;
use crono_server::cli::{actions::Action, commands, dispatch};
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
        assert!(output.stderr.is_empty());
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
fn missing_or_unknown_commands_are_rejected() -> Result<()> {
    for args in [&[][..], &["unknown"][..], &["run", "--unknown"][..]] {
        let output = invoke(args)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(!output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn run_reports_unimplemented_runtime() -> Result<()> {
    let output = invoke(&["run"])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("crono-server runtime not implemented"));
    Ok(())
}

#[test]
fn verbosity_and_run_dispatch_are_consistent() -> Result<()> {
    commands::new().debug_assert();
    for args in [
        ["crono-server", "-vv", "run"],
        ["crono-server", "run", "-vv"],
    ] {
        let matches = commands::new().try_get_matches_from(args)?;
        assert_eq!(matches.get_count("verbose"), 2);
        assert_eq!(dispatch::handler(&matches)?, Action::Run);
    }
    Ok(())
}
