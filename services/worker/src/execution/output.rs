//! Human and JSON renderers for sanitized execution envelopes.
//!
//! Both modes write to stderr like worker diagnostics. ANSI is intentionally
//! omitted, so redirected output remains readable and machine-safe.

use super::{EventSink, ExecutionEvent, ExecutionPhase, LogFormat};
use crate::execution::event::ExecutionEventEnvelope;
use crono_api::ExecutionTrigger;
use std::{
    fmt::Display,
    io::{self, Write},
};

/// Stateless renderer; stderr's lock keeps each event's lines together across Runs.
pub struct ConsoleSink {
    format: LogFormat,
}

impl ConsoleSink {
    #[must_use]
    pub const fn new(format: LogFormat) -> Self {
        Self { format }
    }
}

impl EventSink for ConsoleSink {
    fn emit(&self, envelope: &ExecutionEventEnvelope) {
        let mut stderr = io::stderr().lock();
        match self.format {
            LogFormat::Json => {
                if let Ok(line) = serde_json::to_string(envelope) {
                    let _ = writeln!(stderr, "{line}");
                }
            }
            LogFormat::Pretty => {
                let stamp = envelope.timestamp.format("%H:%M:%S%.3f");
                let run = envelope.run_id.to_string();
                let short_run = run.get(..8).unwrap_or(&run);
                let (kind, detail) = match &envelope.event {
                    ExecutionEvent::RunReceived {
                        dispatch_id,
                        trigger,
                        scheduled_at,
                    } => (
                        "run received",
                        received_detail(envelope, *dispatch_id, *trigger, *scheduled_at),
                    ),
                    ExecutionEvent::RunStarted { dry_run } => {
                        ("run started", format!("dry_run={dry_run}"))
                    }
                    ExecutionEvent::ContextResolved { values } => {
                        ("context resolved", values.to_string())
                    }
                    ExecutionEvent::CommandResolved {
                        executable,
                        shell_command,
                        template,
                        arguments,
                    } => (
                        "command resolved",
                        format!(
                            "executable={executable:?} shell_command={shell_command:?} template={template:?} argv={arguments:?}"
                        ),
                    ),
                    ExecutionEvent::ProcessStarted { pid } => {
                        ("process started", format!("pid={}", option_display(*pid)))
                    }
                    ExecutionEvent::Stdout { line } => ("stdout │", terminal_safe(line)),
                    ExecutionEvent::Stderr { line } => ("stderr │", terminal_safe(line)),
                    ExecutionEvent::ProcessExited {
                        exit_code,
                        signal,
                        duration_ms,
                    } => (
                        "process exited",
                        format!(
                            "exit={} signal={} duration={duration_ms}ms",
                            option_display(*exit_code),
                            option_display(*signal)
                        ),
                    ),
                    ExecutionEvent::ProcessSkipped { reason } => {
                        ("process skipped", reason.clone())
                    }
                    ExecutionEvent::RunCompleted { total_duration_ms } => {
                        ("✓ completed", format!("in {total_duration_ms}ms"))
                    }
                    ExecutionEvent::RunSkipped {
                        reason,
                        total_duration_ms,
                    } => ("run skipped", format!("{reason} in {total_duration_ms}ms")),
                    ExecutionEvent::RunFailed {
                        phase,
                        error,
                        exit_code,
                        total_duration_ms,
                    } => (
                        "ERROR run failed",
                        format!(
                            "phase={} exit={} duration={total_duration_ms}ms error={}",
                            phase_label(*phase),
                            option_display(*exit_code),
                            terminal_safe(error)
                        ),
                    ),
                    ExecutionEvent::ResultReportingFailed {
                        error,
                        execution_succeeded,
                        execution_skipped,
                        total_duration_ms,
                    } => (
                        "ERROR result report",
                        format!(
                            "execution_succeeded={execution_succeeded} execution_skipped={execution_skipped} duration={total_duration_ms}ms error={}",
                            terminal_safe(error)
                        ),
                    ),
                };
                let _ = writeln!(stderr, "{stamp} [{short_run}] {kind:<18} {detail}");
            }
        }
    }
}

fn received_detail(
    envelope: &ExecutionEventEnvelope,
    dispatch_id: uuid::Uuid,
    trigger: Option<ExecutionTrigger>,
    scheduled_at: Option<impl Display>,
) -> String {
    format!(
        "dispatch={dispatch_id} job={} queue={} worker={} trigger={} scheduled_at={}",
        option_display(envelope.job_id),
        envelope.queue,
        envelope.worker_id,
        trigger.map_or("unknown", trigger_label),
        option_display(scheduled_at),
    )
}

fn option_display<T: Display>(value: Option<T>) -> String {
    value.map_or_else(|| "-".to_string(), |item| item.to_string())
}

const fn trigger_label(trigger: ExecutionTrigger) -> &'static str {
    match trigger {
        ExecutionTrigger::Manual => "manual",
        ExecutionTrigger::Schedule => "schedule",
        ExecutionTrigger::Rerun => "rerun",
    }
}

const fn phase_label(phase: ExecutionPhase) -> &'static str {
    match phase {
        ExecutionPhase::Context => "context",
        ExecutionPhase::Command => "command",
        ExecutionPhase::Spawn => "spawn",
        ExecutionPhase::Execution => "execution",
    }
}

/// Escape child-controlled control bytes so redirected pretty logs never gain
/// terminal commands or untrusted cursor movement from process output.
fn terminal_safe(value: &str) -> String {
    value.escape_debug().to_string()
}

#[cfg(test)]
mod tests {
    use super::terminal_safe;

    #[test]
    fn pretty_output_escapes_child_ansi_sequences() {
        let safe = terminal_safe("\u{1b}[31mhello\u{1b}[0m");
        assert!(safe.contains("hello"));
        assert!(!safe.contains('\u{1b}'));
    }
}
