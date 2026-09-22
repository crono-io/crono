//! Pure control-plane concepts for organizing and addressing Crono workloads.
//!
//! Namespaces organize Jobs, Targets, and Target Sets without participating in
//! execution. Jobs identify what should run; Targets identify where or against
//! what it should run. Target Sets provide deterministic explicit membership.
//! This layer contains no persistence, transport, scheduler, worker, or
//! executor-specific representations.
//!
//! Executor configuration and immutable Target versions are intentionally not
//! modeled yet. Those contracts must be introduced together before a Run can
//! pin an exact Target definition for reproducible execution.

mod id;
mod job;
mod name;
mod namespace;
mod target;
mod target_set;

pub use id::{JobId, NamespaceId, TargetId, TargetSetId};
pub use job::Job;
pub use name::{NameError, NamespaceName, ResourceName};
pub use namespace::{Namespace, NamespaceMismatch, QualifiedName};
pub use target::Target;
pub use target_set::TargetSet;
