//! Strong UUID identities for control-plane resource kinds.
//!
//! PostgreSQL 18 allocates `UUIDv7` values, while separate Rust wrappers prevent
//! accidental interchange between Namespace, Queue, Job, Target, Schedule, Run,
//! Attempt, and dispatch identities.

use uuid::Uuid;

macro_rules! identifier {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Construct an identity from an allocator-provided UUID.
            #[must_use]
            pub const fn new(value: Uuid) -> Self {
                Self(value)
            }

            /// Return the transport-neutral UUID representation.
            #[must_use]
            pub const fn get(self) -> Uuid {
                self.0
            }
        }
    };
}

identifier!(NamespaceId, "Stable internal identity of a Namespace.");
identifier!(QueueId, "Stable internal identity of a worker Queue.");
identifier!(JobId, "Stable internal identity of a Job.");
identifier!(TargetId, "Stable internal identity of a Target.");
identifier!(TargetSetId, "Stable internal identity of a `TargetSet`.");
identifier!(ScheduleId, "Stable internal identity of a Schedule.");
identifier!(RunId, "Stable internal identity of a Run.");
identifier!(AttemptId, "Stable internal identity of a Run attempt.");
identifier!(
    DispatchId,
    "Stable internal identity of an outbox dispatch."
);
