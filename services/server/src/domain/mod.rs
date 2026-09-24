//! Pure control-plane concepts for organizing and addressing Crono workloads.
//!
//! Namespaces organize Jobs, Targets, and Target Sets without participating in
//! execution. Queues identify worker pools globally. Jobs identify what should
//! run; Targets identify where or against what it should run. Target Sets
//! provide deterministic explicit membership.
//! This layer contains no persistence or transport representations. Jobs and
//! Targets are directly editable while Crono is a draft; Runs snapshot their
//! exact execution definition before dispatch.

mod id;
mod job;
mod name;
mod namespace;
mod queue;
mod run;
mod schedule;
mod target;
mod target_set;

pub use id::{
    AttemptId, DispatchId, JobId, NamespaceId, QueueId, RunId, ScheduleId, TargetId, TargetSetId,
};
pub use job::{ExecutorKind, Job, JobData};
pub use name::{NameError, NamespaceName, QueueName, ResourceName};
pub use namespace::{Namespace, NamespaceMismatch, QualifiedName};
pub use queue::Queue;
pub use run::{Run, RunData, RunStatus};
pub use schedule::{CatchupPolicy, MisfirePolicy, Schedule, ScheduleTiming, TargetSelection};
pub use target::Target;
pub use target_set::TargetSet;
