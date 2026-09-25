//! Structured, worker-observed execution milestones shared across Crono crates.
//!
//! The schema is internal and not yet a public API. A worker sink sanitizes
//! context, commands, and output before constructing these values. Sequence
//! numbers order one observation of a claimed Attempt; timestamps alone cannot
//! establish the byte order of two independent process pipes.

use chrono::{DateTime, Utc};
use crono_api::ExecutionTrigger;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Phase attached to failures, distinguishing child exits from infrastructure errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPhase {
    Context,
    Command,
    Spawn,
    Execution,
}

/// Execution data already sanitized for observation, never raw child inputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ExecutionEvent {
    RunReceived {
        dispatch_id: Uuid,
        trigger: Option<ExecutionTrigger>,
        scheduled_at: Option<time::OffsetDateTime>,
    },
    RunStarted {
        dry_run: bool,
    },
    ContextResolved {
        values: Value,
    },
    CommandResolved {
        executable: String,
        /// Literal shell source when the executor invokes an interpreter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shell_command: Option<String>,
        template: Vec<String>,
        arguments: Vec<String>,
    },
    ProcessStarted {
        pid: Option<u32>,
    },
    Stdout {
        line: String,
    },
    Stderr {
        line: String,
    },
    ProcessExited {
        exit_code: Option<i32>,
        signal: Option<i32>,
        duration_ms: u64,
    },
    ProcessSkipped {
        reason: String,
    },
    RunCompleted {
        total_duration_ms: u64,
    },
    /// The worker intentionally did not execute this Run.
    RunSkipped {
        reason: String,
        total_duration_ms: u64,
    },
    RunFailed {
        phase: ExecutionPhase,
        error: String,
        exit_code: Option<i32>,
        total_duration_ms: u64,
    },
    /// Completion was not confirmed by the server; the durable outcome is
    /// uncertain and must not be reported as an execution failure.
    ResultReportingFailed {
        error: String,
        execution_succeeded: bool,
        execution_skipped: bool,
        total_duration_ms: u64,
    },
}

/// Common identity, chronology, and one structured event for later transport.
/// A delivery may be retried, so `(observation_id, sequence)` distinguishes
/// one worker observation from another for future idempotent persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionEventEnvelope {
    pub timestamp: DateTime<Utc>,
    pub observation_id: Uuid,
    pub sequence: u64,
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub job_id: Option<Uuid>,
    pub worker_id: String,
    pub queue: String,
    pub event: ExecutionEvent,
}

#[cfg(test)]
mod tests {
    use super::{ExecutionEvent, ExecutionEventEnvelope};
    use anyhow::Result;
    use chrono::Utc;
    use crono_api::ExecutionTrigger;
    use uuid::Uuid;

    #[test]
    fn json_event_has_stable_type_and_structured_data() -> Result<()> {
        let event = ExecutionEventEnvelope {
            timestamp: Utc::now(),
            observation_id: Uuid::now_v7(),
            sequence: 3,
            run_id: Uuid::now_v7(),
            attempt_id: Uuid::now_v7(),
            job_id: Some(Uuid::now_v7()),
            worker_id: "worker-01".to_string(),
            queue: "default".to_string(),
            event: ExecutionEvent::Stdout {
                line: "hello world".to_string(),
            },
        };
        let json = serde_json::to_value(&event)?;
        assert!(
            json.get("observation_id")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
        assert_eq!(json.get("sequence"), Some(&serde_json::json!(3)));
        assert_eq!(
            json.pointer("/event/type"),
            Some(&serde_json::json!("stdout"))
        );
        assert_eq!(
            json.pointer("/event/data/line"),
            Some(&serde_json::json!("hello world"))
        );
        assert_eq!(
            serde_json::from_value::<ExecutionEventEnvelope>(json)?,
            event
        );
        Ok(())
    }

    #[test]
    fn unconfirmed_completion_does_not_serialize_as_run_failure() -> Result<()> {
        let event = ExecutionEvent::ResultReportingFailed {
            error: "completion was not confirmed".to_string(),
            execution_succeeded: true,
            execution_skipped: false,
            total_duration_ms: 12,
        };
        let json = serde_json::to_value(event)?;
        assert_eq!(
            json.get("type"),
            Some(&serde_json::json!("result_reporting_failed"))
        );
        assert_eq!(
            json.pointer("/data/execution_succeeded"),
            Some(&serde_json::json!(true))
        );
        Ok(())
    }

    #[test]
    fn scheduled_receipt_round_trips_trigger_and_occurrence_time() -> Result<()> {
        let event = ExecutionEvent::RunReceived {
            dispatch_id: Uuid::now_v7(),
            trigger: Some(ExecutionTrigger::Schedule),
            scheduled_at: Some(time::OffsetDateTime::now_utc()),
        };
        let json = serde_json::to_value(&event)?;
        assert_eq!(
            json.pointer("/data/trigger"),
            Some(&serde_json::json!("schedule"))
        );
        assert_eq!(serde_json::from_value::<ExecutionEvent>(json)?, event);
        Ok(())
    }
}
