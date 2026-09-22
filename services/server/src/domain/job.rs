//! Job identity within a Namespace.
//!
//! A Job describes what Crono should execute and never identifies a host,
//! inventory, cluster, or other destination. Immutable `JobVersion` definitions
//! will carry executable behavior once that contract is implemented.

use super::{JobId, NamespaceId, ResourceName};
use time::OffsetDateTime;

/// Stable identity and Namespace relationship for executable behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    id: JobId,
    namespace_id: NamespaceId,
    name: ResourceName,
    created_at: OffsetDateTime,
}

impl Job {
    /// Construct a Job independently from any Target or execution request.
    #[must_use]
    pub const fn new(
        id: JobId,
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
    pub const fn id(&self) -> JobId {
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

    /// Return when the Job identity was created.
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::Job;
    use crate::domain::{JobId, NamespaceId, ResourceName};
    use anyhow::Result;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn jobs_capture_what_without_a_target_relationship() -> Result<()> {
        let job = Job::new(
            JobId::new(Uuid::from_u128(11)),
            NamespaceId::new(Uuid::from_u128(1)),
            ResourceName::parse("backup")?,
            OffsetDateTime::UNIX_EPOCH,
        );

        assert_eq!(job.id(), JobId::new(Uuid::from_u128(11)));
        assert_eq!(job.namespace_id(), NamespaceId::new(Uuid::from_u128(1)));
        assert_eq!(job.name().as_str(), "backup");
        assert_eq!(job.created_at(), OffsetDateTime::UNIX_EPOCH);
        Ok(())
    }
}
