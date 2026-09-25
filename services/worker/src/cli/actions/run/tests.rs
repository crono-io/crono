//! Behavioral tests for claimed snapshot execution and event emission.

use super::{emit_result, validate_token};
use crate::execution::runner::{OUTPUT_LIMIT, bounded_command_prefix, execute_snapshot};
use crate::execution::{
    EventSink, ExecutionEvent, ExecutionEventEnvelope, ExecutionTimeline, Redactor,
};
use anyhow::Result;
use crono_api::{ExecutionSnapshot, ExecutorKind};
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Default)]
struct CaptureSink(Mutex<Vec<ExecutionEventEnvelope>>);

impl EventSink for CaptureSink {
    fn emit(&self, event: &ExecutionEventEnvelope) {
        if let Ok(mut events) = self.0.lock() {
            events.push(event.clone());
        }
    }
}

impl CaptureSink {
    fn events(&self) -> Result<Vec<ExecutionEventEnvelope>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?
            .clone())
    }
}

fn snapshot(
    executor: ExecutorKind,
    executable: Option<&str>,
    arguments: &[&str],
) -> ExecutionSnapshot {
    ExecutionSnapshot {
        executor,
        executable: executable.map(str::to_string),
        argument_templates: arguments.iter().map(ToString::to_string).collect(),
        arguments: arguments.iter().map(ToString::to_string).collect(),
        inputs: serde_json::json!({}),
        job_id: Some(uuid::Uuid::now_v7()),
        trigger: Some(crono_api::ExecutionTrigger::Manual),
        scheduled_at: None,
        idempotency_key: uuid::Uuid::now_v7(),
        queue_id: uuid::Uuid::now_v7(),
        queue: "default".to_string(),
        idempotent: false,
        dry_run: false,
        retry_initial_seconds: 1,
        retry_max_seconds: 1,
        retry_multiplier: 1.0,
        retry_jitter: 0.0,
    }
}

async fn observed(
    execution: ExecutionSnapshot,
    worker_dry_run: bool,
) -> Result<(super::ExecutionResult, Vec<ExecutionEventEnvelope>)> {
    let capture = Arc::new(CaptureSink::default());
    let sink: Arc<dyn EventSink> = capture.clone();
    let timeline = Arc::new(ExecutionTimeline::new(
        execution.idempotency_key,
        uuid::Uuid::now_v7(),
        execution.job_id,
        "worker-01".to_string(),
        execution.queue.clone(),
        sink,
    ));
    let redactor = Arc::new(Redactor::from_inputs_and_arguments(
        &execution.inputs,
        &execution.arguments,
    ));
    let started = Instant::now();
    timeline.emit(ExecutionEvent::RunReceived {
        dispatch_id: uuid::Uuid::now_v7(),
        trigger: execution.trigger,
        scheduled_at: execution.scheduled_at,
    });
    let dry_run = worker_dry_run || execution.dry_run;
    timeline.emit(ExecutionEvent::RunStarted { dry_run });
    let result = execute_snapshot(
        execution,
        dry_run,
        Arc::clone(&timeline),
        Arc::clone(&redactor),
    )
    .await?;
    emit_result(&timeline, &redactor, started, &result);
    Ok((result, capture.events()?))
}

#[test]
fn validates_subject_tokens() {
    assert!(validate_token("worker-01", "worker").is_ok());
    assert!(validate_token("Worker.01", "worker").is_err());
    assert!(validate_token("-worker", "worker").is_err());
    assert!(validate_token("worker-", "worker").is_err());
}

#[tokio::test]
async fn dry_run_prints_rendered_argv_without_spawning_a_process() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/definitely/missing/executable"),
        &["hello world", "quoted\"value"],
    );
    let (result, events) = observed(execution, true).await?;
    assert!(result.succeeded);
    assert!(result.skipped);
    assert_eq!(result.exit_code, None);
    assert_eq!(
        result.stdout_tail,
        "DRY RUN (not executed): \"/definitely/missing/executable\" \"hello world\" \"quoted\\\"value\""
    );
    assert!(result.stderr_tail.is_empty());
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, ExecutionEvent::ProcessSkipped { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, ExecutionEvent::RunSkipped { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, ExecutionEvent::ProcessStarted { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn job_dry_run_skips_process_without_worker_flag_and_redacts_output() -> Result<()> {
    let mut execution = snapshot(
        ExecutorKind::Process,
        Some("/definitely/missing/executable"),
        &["hello {{token}}"],
    );
    execution.dry_run = true;
    execution.inputs = serde_json::json!({"token": "very-secret"});
    execution.arguments =
        crono_execution::render_arguments(&execution.argument_templates, &execution.inputs)?;
    let (result, events) = observed(execution, false).await?;
    assert!(result.succeeded);
    assert!(result.skipped);
    assert_eq!(result.exit_code, None);
    assert!(result.stdout_tail.contains("<redacted>"));
    assert!(!result.stdout_tail.contains("very-secret"));
    assert!(!serde_json::to_string(&events)?.contains("very-secret"));
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, ExecutionEvent::RunStarted { dry_run: true }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, ExecutionEvent::RunSkipped { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.event, ExecutionEvent::ProcessStarted { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn normal_process_execution_still_spawns() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/bin/sh"),
        &["-c", "printf normal"],
    );
    let (result, _) = observed(execution, false).await?;
    assert!(result.succeeded);
    assert!(!result.skipped);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout_tail, "normal");
    Ok(())
}

#[test]
fn long_dry_run_output_keeps_its_label_and_utf8_boundary() {
    let long = format!("DRY RUN (not executed): {}", "é".repeat(OUTPUT_LIMIT));
    let bounded = bounded_command_prefix(&long);
    assert!(bounded.starts_with("DRY RUN (not executed): "));
    assert!(bounded.ends_with("[command truncated]"));
    assert!(bounded.len() <= OUTPUT_LIMIT);
}

#[tokio::test]
async fn successful_run_emits_ordered_lifecycle_and_stdout() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/bin/sh"),
        &["-c", "printf 'hello\\n'"],
    );
    let (result, events) = observed(execution, false).await?;
    assert!(result.succeeded);
    assert_eq!(result.stdout_tail, "hello\n");
    let kinds: Vec<&str> = events
        .iter()
        .map(|entry| match entry.event {
            ExecutionEvent::RunReceived { .. } => "received",
            ExecutionEvent::RunStarted { .. } => "started",
            ExecutionEvent::ContextResolved { .. } => "context",
            ExecutionEvent::CommandResolved { .. } => "command",
            ExecutionEvent::ProcessStarted { .. } => "process",
            ExecutionEvent::Stdout { .. } => "stdout",
            ExecutionEvent::ProcessExited { .. } => "exited",
            ExecutionEvent::RunCompleted { .. } => "completed",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "received",
            "started",
            "context",
            "command",
            "process",
            "stdout",
            "exited",
            "completed"
        ]
    );
    assert!(
        events
            .iter()
            .enumerate()
            .all(|(index, entry)| entry.sequence == u64::try_from(index + 1).unwrap_or_default())
    );
    assert!(
        events
            .iter()
            .any(|entry| matches!(entry.event, ExecutionEvent::ProcessExited { .. }))
    );
    assert!(events.iter().any(|entry| matches!(
        entry.event,
        ExecutionEvent::RunCompleted {
            total_duration_ms: _
        }
    )));
    Ok(())
}

#[tokio::test]
async fn stderr_and_mixed_output_stream_as_independent_events() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/bin/sh"),
        &[
            "-c",
            "printf 'out1\\n'; printf 'err1\\n' >&2; printf 'out2\\n'; printf 'err2\\n' >&2",
        ],
    );
    let (result, events) = observed(execution, false).await?;
    assert!(result.succeeded);
    let stdout: Vec<&str> = events
        .iter()
        .filter_map(|entry| {
            if let ExecutionEvent::Stdout { line } = &entry.event {
                Some(line.as_str())
            } else {
                None
            }
        })
        .collect();
    let stderr: Vec<&str> = events
        .iter()
        .filter_map(|entry| {
            if let ExecutionEvent::Stderr { line } = &entry.event {
                Some(line.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(stdout, ["out1", "out2"]);
    assert_eq!(stderr, ["err1", "err2"]);
    assert_eq!(result.stdout_tail, "out1\nout2\n");
    assert_eq!(result.stderr_tail, "err1\nerr2\n");
    Ok(())
}

#[tokio::test]
async fn nonzero_exit_has_process_exit_and_failed_phase() -> Result<()> {
    let execution = snapshot(ExecutorKind::Process, Some("/bin/sh"), &["-c", "exit 7"]);
    let (result, events) = observed(execution, false).await?;
    assert!(!result.succeeded);
    assert_eq!(result.exit_code, Some(7));
    assert!(events.iter().any(|entry| matches!(
        entry.event,
        ExecutionEvent::ProcessExited {
            exit_code: Some(7),
            ..
        }
    )));
    assert!(events.iter().any(|entry| matches!(
        entry.event,
        ExecutionEvent::RunFailed {
            phase: crate::execution::ExecutionPhase::Execution,
            exit_code: Some(7),
            ..
        }
    )));
    Ok(())
}

#[tokio::test]
async fn missing_executable_is_a_spawn_failure() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/definitely/missing/executable"),
        &[],
    );
    let (result, events) = observed(execution, false).await?;
    assert!(!result.succeeded);
    assert!(events.iter().any(|entry| matches!(
        entry.event,
        ExecutionEvent::RunFailed {
            phase: crate::execution::ExecutionPhase::Spawn,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|entry| matches!(entry.event, ExecutionEvent::ProcessStarted { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn context_and_command_events_redact_secrets() -> Result<()> {
    let mut execution = snapshot(
        ExecutorKind::Process,
        Some("/bin/echo"),
        &["hello {{name}} {{token}}"],
    );
    execution.inputs = serde_json::json!({"name":"world", "password":"foo", "token":"bar"});
    execution.arguments =
        crono_execution::render_arguments(&execution.argument_templates, &execution.inputs)?;
    let (result, events) = observed(execution, false).await?;
    assert_eq!(result.stdout_tail, "hello world <redacted>\n");
    let serialized = serde_json::to_string(&events)?;
    assert!(serialized.contains("hello world"));
    assert!(!serialized.contains("foo"));
    assert!(!serialized.contains("bar"));
    assert!(events.iter().any(|entry| matches!(&entry.event, ExecutionEvent::CommandResolved { arguments, .. } if arguments == &["hello world <redacted>"])));
    Ok(())
}

#[tokio::test]
async fn literal_credential_argument_is_redacted_from_child_output() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/bin/echo"),
        &["--token=literal"],
    );
    let (result, events) = observed(execution, false).await?;
    assert_eq!(result.stdout_tail, "--token=<redacted>\n");
    assert!(!serde_json::to_string(&events)?.contains("literal"));
    Ok(())
}

#[tokio::test]
async fn oversized_context_fails_before_context_event() -> Result<()> {
    let mut execution = snapshot(ExecutorKind::Process, Some("/bin/echo"), &["hello"]);
    execution.inputs = serde_json::json!({"value": "x".repeat(crono_execution::MAX_INPUT_BYTES)});
    let (result, events) = observed(execution, false).await?;
    assert!(!result.succeeded);
    assert!(events.iter().any(|entry| matches!(
        entry.event,
        ExecutionEvent::RunFailed {
            phase: crate::execution::ExecutionPhase::Context,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|entry| matches!(entry.event, ExecutionEvent::ContextResolved { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_runs_keep_independent_ids_and_sequences() -> Result<()> {
    let first = snapshot(
        ExecutorKind::Process,
        Some("/bin/sh"),
        &["-c", "printf first"],
    );
    let second = snapshot(
        ExecutorKind::Process,
        Some("/bin/sh"),
        &["-c", "printf second"],
    );
    let first_id = first.idempotency_key;
    let second_id = second.idempotency_key;
    let (first_result, second_result) =
        tokio::join!(observed(first, false), observed(second, false));
    let (_, first_events) = first_result?;
    let (_, second_events) = second_result?;
    assert!(first_events.iter().all(|entry| entry.run_id == first_id));
    assert!(second_events.iter().all(|entry| entry.run_id == second_id));
    assert_ne!(
        first_events.first().map(|entry| entry.observation_id),
        second_events.first().map(|entry| entry.observation_id)
    );
    assert!(first_events.iter().all(|entry| Some(entry.observation_id)
        == first_events.first().map(|first| first.observation_id)));
    assert!(second_events.iter().all(|entry| Some(entry.observation_id)
        == second_events.first().map(|first| first.observation_id)));
    assert_eq!(first_events.first().map(|entry| entry.sequence), Some(1));
    assert_eq!(second_events.first().map(|entry| entry.sequence), Some(1));
    Ok(())
}

#[tokio::test]
async fn oversized_output_line_is_bounded_and_omitted() -> Result<()> {
    let command = format!("head -c {} /dev/zero | tr '\\000' x", OUTPUT_LIMIT + 1);
    let execution = snapshot(ExecutorKind::Process, Some("/bin/sh"), &["-c", &command]);
    let (result, events) = observed(execution, false).await?;
    assert!(result.succeeded);
    assert!(result.stdout_tail.contains("content omitted"));
    assert!(events.iter().any(|entry| matches!(&entry.event, ExecutionEvent::Stdout { line } if line.contains("content omitted"))));
    Ok(())
}

#[tokio::test]
async fn partial_and_invalid_utf8_output_is_lossy_not_fatal() -> Result<()> {
    let execution = snapshot(
        ExecutorKind::Process,
        Some("/bin/sh"),
        &["-c", "printf 'partial\\377'"],
    );
    let (result, events) = observed(execution, false).await?;
    assert!(result.succeeded);
    assert_eq!(result.stdout_tail, "partial�");
    assert!(events.iter().any(
        |entry| matches!(&entry.event, ExecutionEvent::Stdout { line } if line == "partial�")
    ));
    Ok(())
}

#[test]
fn old_execution_snapshots_default_new_observability_fields() -> Result<()> {
    let snapshot = snapshot(ExecutorKind::Noop, None, &[]);
    let mut value = serde_json::to_value(snapshot)?;
    if let Some(object) = value.as_object_mut() {
        object.remove("job_id");
        object.remove("trigger");
        object.remove("scheduled_at");
        object.remove("argument_templates");
    }
    let restored: ExecutionSnapshot = serde_json::from_value(value)?;
    assert_eq!(restored.job_id, None);
    assert_eq!(restored.trigger, None);
    assert_eq!(restored.scheduled_at, None);
    assert!(restored.argument_templates.is_empty());
    Ok(())
}
