//! Namespace entities and derived qualified resource names.
//!
//! A Namespace is an organizational boundary, not an execution primitive.
//! Qualified names combine validated Namespace and resource names for external
//! CLI/API identity while stable typed IDs remain authoritative internally.

use super::{NamespaceId, NamespaceName, ResourceName};
use std::{error::Error, fmt};
use time::OffsetDateTime;

/// Organizational boundary containing Jobs, Targets, and Target Sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    id: NamespaceId,
    name: NamespaceName,
    created_at: OffsetDateTime,
}

impl Namespace {
    /// Construct a Namespace from validated identity and creation metadata.
    #[must_use]
    pub const fn new(id: NamespaceId, name: NamespaceName, created_at: OffsetDateTime) -> Self {
        Self {
            id,
            name,
            created_at,
        }
    }

    /// Return the stable internal identity.
    #[must_use]
    pub const fn id(&self) -> NamespaceId {
        self.id
    }

    /// Return the canonical external name.
    #[must_use]
    pub const fn name(&self) -> &NamespaceName {
        &self.name
    }

    /// Return when the Namespace was created.
    #[must_use]
    pub const fn created_at(&self) -> OffsetDateTime {
        self.created_at
    }

    /// Derive a canonical qualified name for a resource in this Namespace.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceMismatch`] rather than constructing an identity from
    /// a resource relationship belonging to another Namespace.
    pub fn qualify<'a>(
        &'a self,
        resource_namespace_id: NamespaceId,
        resource_name: &'a ResourceName,
    ) -> Result<QualifiedName<'a>, NamespaceMismatch> {
        if self.id == resource_namespace_id {
            Ok(QualifiedName {
                namespace: &self.name,
                resource: resource_name,
            })
        } else {
            Err(NamespaceMismatch::new(self.id, resource_namespace_id))
        }
    }
}

/// Borrowed canonical `namespace/resource` identity for external presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedName<'a> {
    namespace: &'a NamespaceName,
    resource: &'a ResourceName,
}

impl QualifiedName<'_> {
    /// Return the Namespace component.
    #[must_use]
    pub const fn namespace(&self) -> &NamespaceName {
        self.namespace
    }

    /// Return the resource component.
    #[must_use]
    pub const fn resource(&self) -> &ResourceName {
        self.resource
    }
}

impl fmt::Display for QualifiedName<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.namespace, self.resource)
    }
}

/// Attempt to combine a Namespace with a resource owned elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamespaceMismatch {
    expected: NamespaceId,
    actual: NamespaceId,
}

impl NamespaceMismatch {
    pub(super) const fn new(expected: NamespaceId, actual: NamespaceId) -> Self {
        Self { expected, actual }
    }

    /// Return the Namespace required by the operation.
    #[must_use]
    pub const fn expected(self) -> NamespaceId {
        self.expected
    }

    /// Return the Namespace carried by the resource.
    #[must_use]
    pub const fn actual(self) -> NamespaceId {
        self.actual
    }
}

impl fmt::Display for NamespaceMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("resource belongs to a different Namespace")
    }
}

impl Error for NamespaceMismatch {}

#[cfg(test)]
mod tests {
    use super::Namespace;
    use crate::domain::{NamespaceId, NamespaceName, ResourceName};
    use anyhow::Result;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[test]
    fn qualified_names_are_derived_from_validated_relationships() -> Result<()> {
        let namespace = Namespace::new(
            NamespaceId::new(Uuid::nil()),
            NamespaceName::parse("mariadb")?,
            OffsetDateTime::UNIX_EPOCH,
        );
        let job_name = ResourceName::parse("backup")?;

        let qualified = namespace.qualify(namespace.id(), &job_name)?;
        assert_eq!(qualified.to_string(), "mariadb/backup");
        assert_eq!(qualified.namespace(), namespace.name());
        assert_eq!(qualified.resource(), &job_name);
        Ok(())
    }

    #[test]
    fn qualified_names_reject_cross_namespace_relationships() -> Result<()> {
        let namespace = Namespace::new(
            NamespaceId::new(Uuid::nil()),
            NamespaceName::parse("mariadb")?,
            OffsetDateTime::UNIX_EPOCH,
        );
        let target_name = ResourceName::parse("host-123")?;

        let other_id = NamespaceId::new(Uuid::from_u128(2));
        let error = namespace.qualify(other_id, &target_name);
        assert!(error.is_err());
        if let Err(error) = error {
            assert_eq!(error.expected(), NamespaceId::new(Uuid::nil()));
            assert_eq!(error.actual(), other_id);
        }
        Ok(())
    }
}
