//! Per-Attempt emission of the shared execution-event vocabulary.
//!
//! The worker owns clocks and sink delivery; the serializable envelope lives in
//! `crono-execution` so future server and web consumers need no worker dependency.

use chrono::Utc;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub use crono_execution::event::{ExecutionEvent, ExecutionEventEnvelope, ExecutionPhase};

/// Presentation format for execution events, separate from diagnostic tracing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
}

/// Receives already sanitized events. Implementations must not retain unbounded
/// history; synchronous delivery applies backpressure to a noisy child process.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &ExecutionEventEnvelope);
}

/// Cloneable per-Attempt emitter shared by the process and both pipe readers.
pub struct ExecutionTimeline {
    observation_id: Uuid,
    run_id: Uuid,
    attempt_id: Uuid,
    job_id: Option<Uuid>,
    worker_id: String,
    queue: String,
    next_sequence: Mutex<u64>,
    sink: Arc<dyn EventSink>,
}

impl ExecutionTimeline {
    /// Start a per-Attempt timeline; no cross-Run lock or global sequence exists.
    pub fn new(
        run_id: Uuid,
        attempt_id: Uuid,
        job_id: Option<Uuid>,
        worker_id: String,
        queue: String,
        sink: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            observation_id: Uuid::now_v7(),
            run_id,
            attempt_id,
            job_id,
            worker_id,
            queue,
            next_sequence: Mutex::new(1),
            sink,
        }
    }

    /// Stamp and emit one observed milestone immediately. The per-Attempt lock
    /// keeps sequence assignment and sink delivery ordered across pipe readers.
    pub fn emit(&self, event: ExecutionEvent) {
        let mut sequence = match self.next_sequence.lock() {
            Ok(sequence) => sequence,
            Err(poisoned) => poisoned.into_inner(),
        };
        let envelope = ExecutionEventEnvelope {
            timestamp: Utc::now(),
            observation_id: self.observation_id,
            sequence: *sequence,
            run_id: self.run_id,
            attempt_id: self.attempt_id,
            job_id: self.job_id,
            worker_id: self.worker_id.clone(),
            queue: self.queue.clone(),
            event,
        };
        self.sink.emit(&envelope);
        *sequence = sequence.saturating_add(1);
    }
}
