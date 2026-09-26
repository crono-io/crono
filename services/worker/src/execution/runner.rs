//! Process and explicit shell execution with bounded line-oriented child output.
//!
//! Server-rendered argv and merged inputs are immutable; only sanitized copies
//! enter events or Attempt tails. Both pipes are drained concurrently, and
//! dropping this future cancels the pipe readers and kills the child. Shell
//! scripts remain literal while rendered arguments enter as positional data.

use crate::execution::{ExecutionEvent, ExecutionPhase, ExecutionTimeline, Redactor};
use anyhow::{Context, Result, bail};
use crono_api::{ExecutionSnapshot, ExecutorKind};
use crono_execution::MAX_INPUT_BYTES;
use std::{
    env, io::Write, os::unix::process::ExitStatusExt, process::Stdio, sync::Arc, time::Instant,
};
use tempfile::NamedTempFile;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tracing::warn;

pub(crate) const OUTPUT_LIMIT: usize = 65_536;

#[derive(Debug)]
pub(crate) struct ExecutionResult {
    pub(crate) succeeded: bool,
    pub(crate) skipped: bool,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout_tail: String,
    pub(crate) stderr_tail: String,
    pub(crate) error: Option<String>,
    pub(crate) failure_phase: Option<ExecutionPhase>,
}

impl ExecutionResult {
    /// Represent a local setup failure without changing the completion protocol.
    fn failed(phase: ExecutionPhase, error: impl std::fmt::Display) -> Self {
        Self {
            succeeded: false,
            skipped: false,
            exit_code: None,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(error.to_string()),
            failure_phase: Some(phase),
        }
    }
}

/// Emit sanitized context and command details before executing the immutable
/// snapshot. The server has already merged inputs and rendered argv; this
/// function does not re-render or alter the command used by the child.
pub(crate) async fn execute_snapshot(
    execution: ExecutionSnapshot,
    dry_run: bool,
    timeline: Arc<ExecutionTimeline>,
    redactor: Arc<Redactor>,
) -> Result<ExecutionResult> {
    let encoded_inputs = match serde_json::to_vec(&execution.inputs) {
        Ok(inputs) => inputs,
        Err(error) => return Ok(ExecutionResult::failed(ExecutionPhase::Context, error)),
    };
    if encoded_inputs.len() > MAX_INPUT_BYTES {
        return Ok(ExecutionResult::failed(
            ExecutionPhase::Context,
            "execution inputs exceed the worker limit",
        ));
    }
    timeline.emit(ExecutionEvent::ContextResolved {
        values: redactor.values(&execution.inputs),
    });
    if let Some(executable) = execution.executable.as_deref() {
        timeline.emit(ExecutionEvent::CommandResolved {
            executable: redactor.text(executable),
            shell_command: execution
                .shell_command
                .as_deref()
                .map(|script| redactor.text(script)),
            template: redactor.arguments(&execution.argument_templates),
            arguments: redactor.arguments(&execution.arguments),
        });
    }
    if dry_run {
        return Ok(dry_run_snapshot(&execution, &timeline, &redactor)
            .unwrap_or_else(|error| ExecutionResult::failed(ExecutionPhase::Command, error)));
    }
    match execution.executor {
        ExecutorKind::Noop => {
            timeline.emit(ExecutionEvent::ProcessSkipped {
                reason: "no-op executor".to_string(),
            });
            Ok(ExecutionResult {
                succeeded: true,
                skipped: false,
                exit_code: Some(0),
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                error: None,
                failure_phase: None,
            })
        }
        ExecutorKind::Process | ExecutorKind::Shell => {
            execute_process(execution, timeline, redactor).await
        }
    }
}

/// Print the immutable executable and rendered argv without spawning a process.
///
/// # Errors
///
/// Rejects malformed process snapshots before reporting a successful dry run.
fn dry_run_snapshot(
    execution: &ExecutionSnapshot,
    timeline: &ExecutionTimeline,
    redactor: &Redactor,
) -> Result<ExecutionResult> {
    let command = match execution.executor {
        ExecutorKind::Noop => "no-op executor (no command)".to_string(),
        ExecutorKind::Process | ExecutorKind::Shell => {
            let executable = execution
                .executable
                .as_deref()
                .context("process snapshot has no executable")?;
            if !executable.starts_with('/') {
                bail!("process executable is not absolute");
            }
            if execution.executor == ExecutorKind::Shell && execution.shell_command.is_none() {
                bail!("shell snapshot has no script");
            }
            let arguments = redactor
                .arguments(&execution.arguments)
                .iter()
                .map(|argument| format!("{argument:?}"))
                .collect::<Vec<_>>()
                .join(" ");
            let script = execution
                .shell_command
                .as_ref()
                .map(|value| format!(" -c {:?} crono-job", redactor.text(value)))
                .unwrap_or_default();
            format!("{:?}{script} {arguments}", redactor.text(executable))
                .trim_end()
                .to_string()
        }
    };
    let line = format!("DRY RUN (not executed): {command}");
    timeline.emit(ExecutionEvent::ProcessSkipped {
        reason: "dry run".to_string(),
    });
    if let Err(error) = writeln!(std::io::stdout().lock(), "{line}") {
        warn!(%error, "failed to print dry-run command");
    }
    Ok(ExecutionResult {
        succeeded: true,
        skipped: true,
        exit_code: None,
        stdout_tail: bounded_command_prefix(&line),
        stderr_tail: String::new(),
        error: None,
        failure_phase: None,
    })
}

/// Keep the dry-run label and executable visible when a command exceeds the
/// Attempt output limit; truncate at a UTF-8 boundary with an explicit marker.
pub(crate) fn bounded_command_prefix(line: &str) -> String {
    const MARKER: &str = "\n[command truncated]";
    if line.len() <= OUTPUT_LIMIT {
        return line.to_string();
    }
    let mut end = OUTPUT_LIMIT - MARKER.len();
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARKER}", line.get(..end).unwrap_or_default())
}

/// Start the immutable argv directly, drain both child pipes concurrently,
/// and retain only sanitized bounded tails for the existing completion API.
async fn execute_process(
    execution: ExecutionSnapshot,
    timeline: Arc<ExecutionTimeline>,
    redactor: Arc<Redactor>,
) -> Result<ExecutionResult> {
    let Some(executable) = execution.executable.as_deref() else {
        return Ok(ExecutionResult::failed(
            ExecutionPhase::Command,
            "process snapshot has no executable",
        ));
    };
    if !executable.starts_with('/') {
        return Ok(ExecutionResult::failed(
            ExecutionPhase::Command,
            "process executable is not absolute",
        ));
    }
    let input_file = match prepare_input_file(&execution) {
        Ok(file) => file,
        Err(result) => return Ok(result),
    };

    let mut command = Command::new(executable);
    if execution.executor == ExecutorKind::Shell {
        let Some(script) = execution.shell_command.as_deref() else {
            return Ok(ExecutionResult::failed(
                ExecutionPhase::Command,
                "shell snapshot has no script",
            ));
        };
        command.args(["-c", script, "crono-job"]);
    }
    command
        .args(&execution.arguments)
        .env_clear()
        .env("CRONO_INPUTS_FILE", input_file.path())
        .env("CRONO_RUN_ID", execution.idempotency_key.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in ["LANG", "LC_ALL", "TZ"] {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    let process_started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Ok(ExecutionResult::failed(
                ExecutionPhase::Spawn,
                format!("failed to spawn executor {executable:?}: {error}"),
            ));
        }
    };
    timeline.emit(ExecutionEvent::ProcessStarted { pid: child.id() });
    let stdout = child
        .stdout
        .take()
        .context("process stdout pipe is missing")?;
    let stderr = child
        .stderr
        .take()
        .context("process stderr pipe is missing")?;
    let wait = async {
        let status = child.wait().await.context("failed to wait for process")?;
        Ok::<_, anyhow::Error>((status, elapsed_ms(process_started)))
    };
    let ((status, process_duration_ms), stdout_tail, stderr_tail) = tokio::try_join!(
        wait,
        read_output(stdout, Arc::clone(&timeline), Arc::clone(&redactor), false),
        read_output(stderr, Arc::clone(&timeline), redactor, true),
    )?;
    timeline.emit(ExecutionEvent::ProcessExited {
        exit_code: status.code(),
        signal: status.signal(),
        duration_ms: process_duration_ms,
    });
    Ok(ExecutionResult {
        succeeded: status.success(),
        skipped: false,
        exit_code: status.code(),
        stdout_tail,
        stderr_tail,
        error: (!status.success()).then(|| "process exited unsuccessfully".to_string()),
        failure_phase: (!status.success()).then_some(ExecutionPhase::Execution),
    })
}

/// Validate and write the exact server-snapshot inputs for the child only;
/// observable events receive a separate sanitized copy.
fn prepare_input_file(
    execution: &ExecutionSnapshot,
) -> std::result::Result<NamedTempFile, ExecutionResult> {
    let inputs = serde_json::to_vec(&execution.inputs)
        .map_err(|error| ExecutionResult::failed(ExecutionPhase::Context, error))?;
    if inputs.len() > MAX_INPUT_BYTES {
        return Err(ExecutionResult::failed(
            ExecutionPhase::Context,
            "execution inputs exceed the worker limit",
        ));
    }
    let mut file = NamedTempFile::new().map_err(|error| {
        ExecutionResult::failed(
            ExecutionPhase::Context,
            format!("failed to create execution input file: {error}"),
        )
    })?;
    file.write_all(&inputs).map_err(|error| {
        ExecutionResult::failed(
            ExecutionPhase::Context,
            format!("failed to write execution input file: {error}"),
        )
    })?;
    file.flush().map_err(|error| {
        ExecutionResult::failed(
            ExecutionPhase::Context,
            format!("failed to flush execution input file: {error}"),
        )
    })?;
    Ok(file)
}

pub(crate) fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Drain one pipe concurrently with the other. Emit complete lines promptly;
/// unterminated lines are flushed at EOF. Lines over 64 KiB are omitted from
/// telemetry and Attempt tails while their bytes are still drained. Non-UTF-8
/// text is decoded lossily, and stored tails contain only sanitized text.
async fn read_output(
    mut reader: impl AsyncRead + Unpin,
    timeline: Arc<ExecutionTimeline>,
    redactor: Arc<Redactor>,
    stderr: bool,
) -> Result<String> {
    let mut tail = String::new();
    let mut line = Vec::new();
    let mut chunk = vec![0_u8; 8192];
    let mut oversized = false;
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        for &byte in chunk.get(..read).context("invalid output read size")? {
            if byte == b'\n' {
                emit_line(
                    &mut tail, &line, oversized, true, stderr, &timeline, &redactor,
                );
                line.clear();
                oversized = false;
            } else if !oversized {
                if line.len() < OUTPUT_LIMIT {
                    line.push(byte);
                } else {
                    line.clear();
                    oversized = true;
                }
            }
        }
    }
    if !line.is_empty() || oversized {
        emit_line(
            &mut tail, &line, oversized, false, stderr, &timeline, &redactor,
        );
    }
    Ok(tail)
}

fn emit_line(
    tail: &mut String,
    bytes: &[u8],
    oversized: bool,
    terminated: bool,
    stderr: bool,
    timeline: &ExecutionTimeline,
    redactor: &Redactor,
) {
    let decoded = if oversized {
        "[output line exceeded 64 KiB; content omitted]".to_string()
    } else {
        redactor.text(&String::from_utf8_lossy(bytes))
    };
    let line = if decoded.len() > OUTPUT_LIMIT {
        "[decoded output line exceeded 64 KiB; content omitted]".to_string()
    } else {
        decoded.trim_end_matches('\r').to_string()
    };
    timeline.emit(if stderr {
        ExecutionEvent::Stderr { line: line.clone() }
    } else {
        ExecutionEvent::Stdout { line: line.clone() }
    });
    tail.push_str(&line);
    if terminated {
        tail.push('\n');
    }
    if tail.len() > OUTPUT_LIMIT {
        let mut start = tail.len() - OUTPUT_LIMIT;
        while !tail.is_char_boundary(start) {
            start += 1;
        }
        tail.drain(..start);
    }
}

#[cfg(test)]
mod tests {
    use super::{OUTPUT_LIMIT, emit_line};
    use crate::execution::{
        EventSink, ExecutionEvent, ExecutionEventEnvelope, ExecutionTimeline, Redactor,
    };
    use anyhow::{Context, Result};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Capture(Mutex<Vec<ExecutionEventEnvelope>>);

    impl EventSink for Capture {
        fn emit(&self, event: &ExecutionEventEnvelope) {
            if let Ok(mut events) = self.0.lock() {
                events.push(event.clone());
            }
        }
    }

    #[test]
    fn lossy_decoding_cannot_expand_an_event_beyond_the_line_limit() -> Result<()> {
        let capture = Arc::new(Capture::default());
        let sink: Arc<dyn EventSink> = capture.clone();
        let timeline = ExecutionTimeline::new(
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
            None,
            "worker-01".to_string(),
            "default".to_string(),
            sink,
        );
        let mut tail = String::new();
        emit_line(
            &mut tail,
            &vec![0xff_u8; 30_000],
            false,
            false,
            false,
            &timeline,
            &Redactor::from_inputs(&serde_json::json!({})),
        );
        assert!(tail.contains("content omitted"));
        assert!(tail.len() <= OUTPUT_LIMIT);
        let events = capture
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?;
        let event = events.first().context("no stdout event")?;
        assert!(
            matches!(&event.event, ExecutionEvent::Stdout { line } if line.len() <= OUTPUT_LIMIT)
        );
        Ok(())
    }
}
