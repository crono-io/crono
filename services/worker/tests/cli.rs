//! Exercise CLI behavior through the actual process boundary.

use anyhow::Result;
use crono_worker::cli::{
    actions::{Action, run},
    commands, dispatch,
};
use std::process::{Command, Output};

fn invoke(args: &[&str]) -> Result<Output> {
    Ok(Command::new(env!("CARGO_BIN_EXE_crono-worker"))
        .env_clear()
        .args(args)
        .output()?)
}

#[test]
fn help_and_version_do_not_initialize_telemetry() -> Result<()> {
    for flag in ["-h", "--help", "-V", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_crono-worker"))
            .env_clear()
            .env("RUST_LOG", "[invalid")
            .env("OTEL_EXPORTER_OTLP_ENDPOINT", "invalid endpoint")
            .arg(flag)
            .output()?;
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout)?;
        assert!(stdout.contains("crono-worker"));
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
        concat!("crono-worker ", env!("CARGO_PKG_VERSION"))
    );
    let output = invoke(&["--version"])?;
    assert_eq!(
        String::from_utf8(output.stdout)?.trim(),
        format!(
            "crono-worker {} - {}",
            env!("CARGO_PKG_VERSION"),
            crono_worker::built_info::GIT_COMMIT_HASH.unwrap_or("unknown")
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
fn run_reports_unavailable_nats() -> Result<()> {
    let output = invoke(&["run", "--nats-url", "nats://127.0.0.1:1"])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)?.contains("failed to connect worker to NATS"));
    Ok(())
}

#[test]
fn verbosity_and_run_dispatch_are_consistent() -> Result<()> {
    commands::new().debug_assert();
    for args in [
        ["crono-worker", "-vv", "run", "--worker-id", "worker-01"],
        ["crono-worker", "run", "--worker-id", "worker-01", "-vv"],
    ] {
        let matches = commands::new().try_get_matches_from(args)?;
        assert_eq!(matches.get_count("verbose"), 2);
        assert_eq!(
            dispatch::handler(&matches)?,
            Action::Run(run::Args {
                nats_url: "nats://127.0.0.1:4222".to_string(),
                queue: "default".to_string(),
                worker_id: "worker-01".to_string(),
                concurrency: 8,
                dry_run: false,
                log_format: crono_worker::execution::LogFormat::Pretty,
            })
        );
    }
    Ok(())
}

#[test]
fn dry_run_is_independent_of_log_verbosity() -> Result<()> {
    for args in [
        [
            "crono-worker",
            "--dry-run",
            "run",
            "--worker-id",
            "worker-01",
        ],
        [
            "crono-worker",
            "run",
            "--worker-id",
            "worker-01",
            "--dry-run",
        ],
    ] {
        let matches = commands::new().try_get_matches_from(args)?;
        assert_eq!(matches.get_count("verbose"), 0);
        assert!(matches.get_flag("dry-run"));
        assert!(matches!(
            dispatch::handler(&matches)?,
            Action::Run(run::Args { dry_run: true, .. })
        ));
    }
    Ok(())
}

#[test]
fn log_format_selects_event_renderer_without_changing_dispatch() -> Result<()> {
    for args in [
        ["crono-worker", "--log-format", "json", "run"],
        ["crono-worker", "run", "--log-format", "json"],
    ] {
        let matches = commands::new().try_get_matches_from(args)?;
        assert!(matches!(
            dispatch::handler(&matches)?,
            Action::Run(run::Args {
                log_format: crono_worker::execution::LogFormat::Json,
                ..
            })
        ));
    }
    Ok(())
}
