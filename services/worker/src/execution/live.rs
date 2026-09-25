//! Bounded, sanitized output snapshots for the authorized Attempt view.
//!
//! The sink consumes the same typed events as the console renderer. It never
//! retains raw child bytes or waits for the network while draining a pipe.

use super::{EventSink, ExecutionEvent, ExecutionEventEnvelope};
use std::sync::Mutex;

const LIVE_TAIL_BYTES: usize = 16_384;

#[derive(Default)]
struct Tails {
    revision: u64,
    stdout: String,
    stderr: String,
}

/// Latest bounded stdout/stderr, updated only from sanitized timeline events.
#[derive(Default)]
pub struct LiveOutput(Mutex<Tails>);

impl LiveOutput {
    /// Clone a consistent snapshot without holding the lock across network I/O.
    #[must_use]
    pub fn snapshot(&self) -> (u64, String, String) {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (state.revision, state.stdout.clone(), state.stderr.clone())
    }
}

impl EventSink for LiveOutput {
    fn emit(&self, envelope: &ExecutionEventEnvelope) {
        let (line, stderr) = match &envelope.event {
            ExecutionEvent::Stdout { line } => (line, false),
            ExecutionEvent::Stderr { line } => (line, true),
            _ => return,
        };
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let tail = if stderr {
            &mut state.stderr
        } else {
            &mut state.stdout
        };
        tail.push_str(line);
        tail.push('\n');
        if tail.len() > LIVE_TAIL_BYTES {
            let mut start = tail.len() - LIVE_TAIL_BYTES;
            while !tail.is_char_boundary(start) {
                start += 1;
            }
            tail.drain(..start);
        }
        state.revision = state.revision.saturating_add(1);
    }
}

/// Synchronously send an event to two independent bounded renderers.
pub struct FanoutSink {
    pub console: std::sync::Arc<dyn EventSink>,
    pub live: std::sync::Arc<LiveOutput>,
}

impl EventSink for FanoutSink {
    fn emit(&self, event: &ExecutionEventEnvelope) {
        self.live.emit(event);
        self.console.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::{LIVE_TAIL_BYTES, LiveOutput};
    use crate::execution::{EventSink, ExecutionEvent, ExecutionEventEnvelope};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn live_output_keeps_separate_bounded_streams() {
        let live = LiveOutput::default();
        for (index, event) in [
            ExecutionEvent::Stdout {
                line: "hello".to_string(),
            },
            ExecutionEvent::Stderr {
                line: "warning".to_string(),
            },
            ExecutionEvent::Stdout {
                line: "x".repeat(LIVE_TAIL_BYTES),
            },
        ]
        .into_iter()
        .enumerate()
        {
            live.emit(&ExecutionEventEnvelope {
                timestamp: Utc::now(),
                observation_id: Uuid::now_v7(),
                sequence: index as u64,
                run_id: Uuid::now_v7(),
                attempt_id: Uuid::now_v7(),
                job_id: None,
                worker_id: "worker-01".to_string(),
                queue: "default".to_string(),
                event,
            });
        }
        let (revision, stdout, stderr) = live.snapshot();
        assert_eq!(revision, 3);
        assert!(stdout.len() <= LIVE_TAIL_BYTES);
        assert_eq!(stderr, "warning\n");
    }
}
