//! Canonical path-safe names shared by control-plane resources.
//!
//! Names use one deterministic lowercase ASCII rule so future CLI, API, and
//! persistence adapters cannot diverge. A name starts and ends with an ASCII
//! letter or digit and may contain lowercase letters, digits, and hyphens in
//! between. Slash-separated qualification is constructed elsewhere from these
//! already validated segments.

use crono_api::{ResourceNameError, validate_resource_name};
use std::fmt;

/// Shared reason a Namespace or resource name was rejected.
pub type NameError = ResourceNameError;

/// Validated canonical Namespace name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamespaceName(String);

impl NamespaceName {
    /// Validate and own a Namespace path segment.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] when the value is empty, has invalid boundaries,
    /// or contains characters outside lowercase ASCII letters, digits, and `-`.
    pub fn parse(value: &str) -> Result<Self, NameError> {
        validate_resource_name(value)?;
        Ok(Self(value.to_string()))
    }

    /// Return the validated path segment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NamespaceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validated canonical name for a Job, Target, or Target Set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceName(String);

impl ResourceName {
    /// Validate and own a resource path segment.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] under the same centralized rules used for
    /// Namespace names.
    pub fn parse(value: &str) -> Result<Self, NameError> {
        validate_resource_name(value)?;
        Ok(Self(value.to_string()))
    }

    /// Return the validated path segment.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Validated canonical lookup and display name for a worker Queue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueueName(String);

impl QueueName {
    /// Name of the always-present system Queue; no other Queue may use it.
    pub const SYSTEM: &'static str = "default";

    /// Validate and own a Queue name under the canonical resource-name rules.
    ///
    /// # Errors
    ///
    /// Returns [`NameError`] for invalid or oversized names.
    pub fn parse(value: &str) -> Result<Self, NameError> {
        validate_resource_name(value)?;
        Ok(Self(value.to_string()))
    }

    /// Return the safe single subject token.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is the reserved name of the system Queue.
    #[must_use]
    pub fn is_system(&self) -> bool {
        self.0 == Self::SYSTEM
    }
}

impl fmt::Display for QueueName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{NamespaceName, QueueName, ResourceName};
    use anyhow::Result;
    use crono_api::RESOURCE_NAME_MAX_LENGTH;

    #[test]
    fn namespace_names_accept_canonical_path_segments() -> Result<()> {
        for value in ["mariadb", "postgres", "patroni", "database-prod", "db2"] {
            assert_eq!(NamespaceName::parse(value)?.as_str(), value);
        }
        Ok(())
    }

    #[test]
    fn namespace_names_reject_unsafe_or_ambiguous_forms() {
        for value in [
            "",
            "/",
            "../foo",
            "MariaDB Test",
            "mariadb/backup",
            "-prod",
            "prod-",
        ] {
            assert!(NamespaceName::parse(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn resource_names_support_jobs_targets_and_sets() -> Result<()> {
        for value in ["backup", "host-123", "mariadb-prod", "restart-deployment"] {
            assert_eq!(ResourceName::parse(value)?.as_str(), value);
        }
        Ok(())
    }

    #[test]
    fn resource_names_reject_noncanonical_forms() {
        for value in ["", "host_123", "host 123", "Host-123", ".", "../host"] {
            assert!(ResourceName::parse(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn names_and_queues_have_one_shared_length_bound() {
        let maximum = "a".repeat(RESOURCE_NAME_MAX_LENGTH);
        let oversized = "a".repeat(RESOURCE_NAME_MAX_LENGTH + 1);
        assert!(NamespaceName::parse(&maximum).is_ok());
        assert!(ResourceName::parse(&maximum).is_ok());
        assert!(QueueName::parse("database-default").is_ok());
        assert!(QueueName::parse("database.default").is_err());
        assert!(NamespaceName::parse(&oversized).is_err());
    }
}
