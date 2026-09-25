//! Per-attempt execution history, independent of its terminal representation.
//!
//! A timeline assigns sequence numbers at observation time. Concurrent pipe
//! readers share it, so each event remains attributable to one Run without
//! buffering an entire process output. Values are sanitized before emission;
//! sinks must never receive the raw input object or executable arguments.

mod event;
mod live;
mod output;
mod redaction;
pub(crate) mod runner;

pub use event::{
    EventSink, ExecutionEvent, ExecutionEventEnvelope, ExecutionPhase, ExecutionTimeline, LogFormat,
};
pub use live::{FanoutSink, LiveOutput};
pub use output::ConsoleSink;
pub use redaction::Redactor;
