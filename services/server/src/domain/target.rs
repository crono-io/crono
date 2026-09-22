//! Executor-agnostic Target identity within a Namespace.
//!
//! A Target describes where or against what a Job runs. It intentionally does
//! not contain Ansible inventory, SSH, database, Kubernetes, or Terraform
//! fields. Executor configuration and immutable `TargetVersion` semantics must be
//! designed together before either is attached to Runs.

use super::{NamespaceId, ResourceName, TargetId};
use time::OffsetDateTime;

/// Stable identity of an execution destination or resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    id: TargetId,
    namespace_id: NamespaceId,
    name: ResourceName,
    created_at: OffsetDateTime,
}

impl Target {
    /// Construct a Target without interpreting executor-specific configuration.
    #[must_use]
    pub const fn new(
        id: TargetId,
        namespace_id: NamespaceId,
        name: ResourceName,
        created_at: OffsetDateTime,
    ) -> Self {
        Self {
            id,
            namespace_id,
            name,
            created_at,
        }
    }

    /// Return the stable internal identity.
    #[must_use]
    pub const fn id(&self) -> TargetId {
        self.id
    }

    /// Return the owning Namespace identity.
    #[must_use]
    pub const fn namespace_id(&self) -> NamespaceId {
        self.namespace_id
    }

    /// Return the canonical name within its Namespace.
    #[must_use]
    pub const fn name(&self) -> &ResourceName {
        &self.name
    }

    /// Return when the Target identity was created.
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::Target;
    use crate::domain::{NamespaceId, ResourceName, TargetId};
    use anyhow::Result;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn targets_capture_where_without_executor_semantics() -> Result<()> {
        let target = Target::new(
            TargetId::new(Uuid::from_u128(21)),
            NamespaceId::new(Uuid::from_u128(1)),
            ResourceName::parse("host-123")?,
            OffsetDateTime::UNIX_EPOCH,
        );

        assert_eq!(target.id(), TargetId::new(Uuid::from_u128(21)));
        assert_eq!(target.namespace_id(), NamespaceId::new(Uuid::from_u128(1)));
        assert_eq!(target.name().as_str(), "host-123");
        assert_eq!(target.created_at(), OffsetDateTime::UNIX_EPOCH);
        Ok(())
    }
}
