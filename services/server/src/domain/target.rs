//! Directly editable Target arguments.
//!
//! Target arguments are appended to the Job's fixed argument vector without a
//! shell. Runs copy them before dispatch so later edits cannot alter queued work.

use super::{NamespaceId, ResourceName, TargetId};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    id: TargetId,
    namespace_id: NamespaceId,
    name: ResourceName,
    arguments: Vec<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl Target {
    #[must_use]
    pub const fn new(
        id: TargetId,
        namespace_id: NamespaceId,
        name: ResourceName,
        arguments: Vec<String>,
        created_at: OffsetDateTime,
        updated_at: OffsetDateTime,
    ) -> Self {
        Self {
            id,
            namespace_id,
            name,
            arguments,
            created_at,
            updated_at,
        }
    }

    #[must_use]
    pub const fn id(&self) -> TargetId {
        self.id
    }
    #[must_use]
    pub const fn namespace_id(&self) -> NamespaceId {
        self.namespace_id
    }
    #[must_use]
    pub const fn name(&self) -> &ResourceName {
        &self.name
    }
    #[must_use]
    pub fn arguments(&self) -> &[String] {
        &self.arguments
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
