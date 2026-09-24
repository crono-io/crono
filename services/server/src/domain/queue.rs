//! Global worker Queue identity and editable operator metadata.
//!
//! Queue UUIDs are the durable relationship and NATS routing identity. The
//! canonical name remains a human-facing lookup key and may change without
//! rerouting existing Jobs or Runs. Disabling a Queue prevents new Job
//! assignments while allowing existing work to drain.

use super::{QueueId, QueueName};
use time::OffsetDateTime;

/// One globally named worker pool definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    id: QueueId,
    name: QueueName,
    description: Option<String>,
    enabled: bool,
    system: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl Queue {
    /// Construct Queue state loaded from the authoritative store.
    #[must_use]
    pub const fn new(
        id: QueueId,
        name: QueueName,
        description: Option<String>,
        enabled: bool,
        system: bool,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Self {
        Self {
            id,
            name,
            description,
            enabled,
            system,
            created_at,
            updated_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> QueueId {
        self.id
    }

    #[must_use]
    pub const fn name(&self) -> &QueueName {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    /// Return whether Crono protects this bootstrap Queue from removal.
    #[must_use]
    pub const fn system(&self) -> bool {
        self.system
    }

    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    #[must_use]
    pub const fn updated_at(&self) -> OffsetDateTime {
        self.updated_at
    }
}
