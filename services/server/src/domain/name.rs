//! Canonical path-safe names shared by control-plane resources.
//!
//! Names use one deterministic lowercase ASCII rule so future CLI, API, and
//! persistence adapters cannot diverge. A name starts and ends with an ASCII
//! letter or digit and may contain lowercase letters, digits, and hyphens in
//! between. Slash-separated qualification is constructed elsewhere from these
//! already validated segments.

use std::{error::Error, fmt};

/// Reason a Namespace or resource name was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    /// The supplied name contained no characters.
    Empty,
    /// The first or last character was not an ASCII lowercase letter or digit.
    InvalidBoundary,
    /// An interior character was outside the canonical name alphabet.
    InvalidCharacter {
        /// Byte position of the invalid character.
        position: usize,
        /// Invalid character encountered during validation.
        character: char,
    },
}

impl fmt::Display for NameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("name must not be empty"),
            Self::InvalidBoundary => {
                formatter.write_str("name must start and end with a lowercase letter or digit")
            }
            Self::InvalidCharacter {
                position,
                character,
            } => write!(
                formatter,
                "name contains invalid character {character:?} at byte {position}"
            ),
        }
    }
}

impl Error for NameError {}

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
        validate(value)?;
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
        validate(value)?;
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

fn validate(value: &str) -> Result<(), NameError> {
    let mut characters = value.char_indices();
    let Some((_, first)) = characters.next() else {
        return Err(NameError::Empty);
    };
    if !is_boundary(first) {
        return Err(NameError::InvalidBoundary);
    }

    let mut last = first;
    for (position, character) in characters {
        if !is_allowed(character) {
            return Err(NameError::InvalidCharacter {
                position,
                character,
            });
        }
        last = character;
    }

    if !is_boundary(last) {
        return Err(NameError::InvalidBoundary);
    }
    Ok(())
}

const fn is_boundary(character: char) -> bool {
    character.is_ascii_lowercase() || character.is_ascii_digit()
}

const fn is_allowed(character: char) -> bool {
    is_boundary(character) || character == '-'
}

#[cfg(test)]
mod tests {
    use super::{NamespaceName, ResourceName};
    use anyhow::Result;

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
}
