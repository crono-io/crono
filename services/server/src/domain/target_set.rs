//! Explicit deterministic Target Set membership.
//!
//! Target Sets are named collections inside one Namespace. Membership is added
//! from complete Target entities so cross-Namespace relationships are rejected
//! at the domain boundary. A sorted set normalizes duplicates and yields stable
//! iteration without introducing selectors, labels, or placement semantics.

use super::{NamespaceId, NamespaceMismatch, ResourceName, Target, TargetId, TargetSetId};
use std::{collections::BTreeSet, time::SystemTime};

/// Named explicit selection of Targets in one Namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSet {
    id: TargetSetId,
    namespace_id: NamespaceId,
    name: ResourceName,
    members: BTreeSet<TargetId>,
    created_at: SystemTime,
}

impl TargetSet {
    /// Construct an empty Target Set ready for explicit membership.
    #[must_use]
    pub const fn new(
        id: TargetSetId,
        namespace_id: NamespaceId,
        name: ResourceName,
        created_at: SystemTime,
    ) -> Self {
        Self {
            id,
            namespace_id,
            name,
            members: BTreeSet::new(),
            created_at,
        }
    }

    /// Return the stable internal identity.
    #[must_use]
    pub const fn id(&self) -> TargetSetId {
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

    /// Return the deterministic unique membership set.
    #[must_use]
    pub const fn members(&self) -> &BTreeSet<TargetId> {
        &self.members
    }

    /// Return when the Target Set was created.
    #[must_use]
    pub const fn created_at(&self) -> SystemTime {
        self.created_at
    }

    /// Add a Target after enforcing same-Namespace membership.
    ///
    /// Returns `true` when inserted and `false` when the Target was already a
    /// member, providing deterministic duplicate normalization.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceMismatch`] when the Target belongs to another
    /// Namespace; the membership set remains unchanged.
    pub fn add_target(&mut self, target: &Target) -> Result<bool, NamespaceMismatch> {
        if self.namespace_id != target.namespace_id() {
            return Err(NamespaceMismatch::new(
                self.namespace_id,
                target.namespace_id(),
            ));
        }
        Ok(self.members.insert(target.id()))
    }
}

#[cfg(test)]
mod tests {
    use super::TargetSet;
    use crate::domain::{NamespaceId, ResourceName, Target, TargetId, TargetSetId};
    use anyhow::Result;
    use std::{collections::BTreeSet, time::SystemTime};

    fn target(id: u128, namespace_id: NamespaceId, name: &str) -> Result<Target> {
        Ok(Target::new(
            TargetId::new(id),
            namespace_id,
            ResourceName::parse(name)?,
            SystemTime::UNIX_EPOCH,
        ))
    }

    #[test]
    fn membership_is_explicit_unique_and_deterministic() -> Result<()> {
        let namespace_id = NamespaceId::new(1);
        let mut set = TargetSet::new(
            TargetSetId::new(31),
            namespace_id,
            ResourceName::parse("mariadb-prod")?,
            SystemTime::UNIX_EPOCH,
        );
        let host_124 = target(24, namespace_id, "host-124")?;
        let host_123 = target(23, namespace_id, "host-123")?;

        assert!(set.add_target(&host_124)?);
        assert!(set.add_target(&host_123)?);
        assert!(!set.add_target(&host_123)?);
        assert_eq!(
            set.members(),
            &BTreeSet::from([TargetId::new(23), TargetId::new(24)])
        );
        Ok(())
    }

    #[test]
    fn membership_rejects_targets_from_another_namespace() -> Result<()> {
        let namespace_id = NamespaceId::new(1);
        let mut set = TargetSet::new(
            TargetSetId::new(31),
            namespace_id,
            ResourceName::parse("mariadb-prod")?,
            SystemTime::UNIX_EPOCH,
        );
        let postgres = target(41, NamespaceId::new(2), "pg-cluster-01")?;

        let error = set.add_target(&postgres);
        assert!(error.is_err());
        assert!(set.members().is_empty());
        if let Err(error) = error {
            assert_eq!(error.expected(), namespace_id);
            assert_eq!(error.actual(), NamespaceId::new(2));
        }
        Ok(())
    }
}
