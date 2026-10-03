//! Bounded, versioned interchange for already-verified IAM authority.
//!
//! Parsing validates structure and scope compatibility, not credential authenticity.
//! Only a trusted verifier may use this module to construct a caller. Permissions
//! remain bound to their assignment scopes; no Cartesian product of Namespace IDs
//! and capabilities is formed. Roles, credentials, and raw provider claims are absent.
//!
//! # Flow Overview
//!
//! Verify the credential and delegation limits first, validate the optional grant
//! document, then normalize duplicates into an immutable grant set. Absent authority
//! becomes empty authority; malformed present authority fails closed.

use super::{ALL_CAPABILITIES, Capability, PermissionScope, VisibilityScope};
use crate::domain::NamespaceId;
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};
use uuid::Uuid;

/// Maximum canonical document size, independent of the Bearer transport limit.
pub const MAX_GRANT_DOCUMENT_BYTES: usize = 64 * 1024;
/// Bound assignment processing even when entries later normalize to the same scope.
pub const MAX_GRANT_ENTRIES: usize = 256;

/// Scope of one assignment; global authority never implies Namespace authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantScope {
    Global,
    /// Includes current and future Namespaces for only the listed permissions.
    AllNamespaces,
    Namespace {
        #[serde(serialize_with = "serialize_namespace")]
        namespace_id: NamespaceId,
    },
}

/// Struct variants enforce unknown-field rejection even for scopes with no payload.
/// Serde's tagged unit variants otherwise ignore extra fields in their scope object.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScopeDocument {
    Global {},
    AllNamespaces {},
    Namespace {
        #[serde(deserialize_with = "deserialize_namespace")]
        namespace_id: NamespaceId,
    },
}

impl<'de> Deserialize<'de> for GrantScope {
    /// Reject extra/duplicate scope fields before turning verified JSON into authority.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match ScopeDocument::deserialize(deserializer)? {
            ScopeDocument::Global {} => Self::Global,
            ScopeDocument::AllNamespaces {} => Self::AllNamespaces,
            ScopeDocument::Namespace { namespace_id } => Self::Namespace { namespace_id },
        })
    }
}

fn serialize_namespace<S: serde::Serializer>(
    id: &NamespaceId,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&id.get().hyphenated().to_string())
}

/// Accept standard hyphenated UUIDs, matching the published schema rather than UUID aliases.
fn deserialize_namespace<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NamespaceId, D::Error> {
    let value = String::deserialize(deserializer)?;
    let id =
        Uuid::parse_str(&value).map_err(|_| serde::de::Error::custom("invalid Namespace UUID"))?;
    if !value.eq_ignore_ascii_case(&id.hyphenated().to_string()) {
        return Err(serde::de::Error::custom("invalid Namespace UUID"));
    }
    Ok(NamespaceId::new(id))
}

/// Safe contract-validation errors; neither documents nor permission values are echoed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantError {
    InvalidDocument,
    UnsupportedVersion,
    UnknownPermission,
    IncompatibleScope,
    TooLarge,
}

impl fmt::Display for GrantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidDocument => "invalid grant document",
            Self::UnsupportedVersion => "unsupported grant contract version",
            Self::UnknownPermission => "unknown permission identifier",
            Self::IncompatibleScope => "permission is incompatible with its assignment scope",
            Self::TooLarge => "grant document exceeds its size or assignment bound",
        })
    }
}

impl Error for GrantError {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantDocument {
    version: u16,
    grants: Vec<Assignment>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    scope: GrantScope,
    permissions: Vec<String>,
}

/// Immutable normalized authority. It cannot be deserialized from HTTP input.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct GrantSet {
    assignments: BTreeMap<GrantScope, BTreeSet<Capability>>,
}

impl fmt::Debug for GrantSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GrantSet([REDACTED])")
    }
}

impl GrantSet {
    /// Normalize trusted assignments after verification, rejecting incompatible scopes.
    ///
    /// Duplicate permissions and assignments combine within the same scope only.
    /// Empty assignments grant nothing. This constructor performs no authentication.
    ///
    /// # Errors
    /// Returns a safe error for excessive entries or invalid scope/permission pairs.
    pub fn new(
        entries: impl IntoIterator<Item = (GrantScope, Vec<Capability>)>,
    ) -> Result<Self, GrantError> {
        let mut assignments: BTreeMap<GrantScope, BTreeSet<Capability>> = BTreeMap::new();
        for (index, (scope, permissions)) in entries.into_iter().enumerate() {
            if index >= MAX_GRANT_ENTRIES {
                return Err(GrantError::TooLarge);
            }
            for permission in permissions {
                let compatible = matches!(
                    (scope, permission.definition().scope),
                    (GrantScope::Global, PermissionScope::Global)
                        | (
                            GrantScope::Namespace { .. } | GrantScope::AllNamespaces,
                            PermissionScope::Namespace
                        )
                );
                if !compatible {
                    return Err(GrantError::IncompatibleScope);
                }
                assignments.entry(scope).or_default().insert(permission);
            }
        }
        Ok(Self { assignments })
    }

    /// Validate a canonical document obtained only from already-verified authority.
    ///
    /// Missing documents yield no permissions. Present invalid documents reject the
    /// caller; they never become empty or full development grants. Provider adapters
    /// must preserve issuer/audience validation and delegated scope limits first.
    ///
    /// # Errors
    /// Rejects malformed, unsupported, oversized, or incompatible authority.
    pub fn from_verified_json(document: Option<&[u8]>) -> Result<Self, GrantError> {
        let Some(bytes) = document else {
            return Ok(Self::default());
        };
        if bytes.len() > MAX_GRANT_DOCUMENT_BYTES {
            return Err(GrantError::TooLarge);
        }
        let document: GrantDocument =
            serde_json::from_slice(bytes).map_err(|_| GrantError::InvalidDocument)?;
        if document.version != 1 {
            return Err(GrantError::UnsupportedVersion);
        }
        if document.grants.len() > MAX_GRANT_ENTRIES {
            return Err(GrantError::TooLarge);
        }
        let entries = document
            .grants
            .into_iter()
            .map(|assignment| {
                let permissions = assignment
                    .permissions
                    .iter()
                    .map(|identifier| {
                        Capability::from_permission_id(identifier)
                            .ok_or(GrantError::UnknownPermission)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((assignment.scope, permissions))
            })
            .collect::<Result<Vec<_>, GrantError>>()?;
        Self::new(entries)
    }

    /// Encode normalized authority for trusted adapter interchange, never into an API response.
    ///
    /// # Errors
    /// Rejects serialization failure or a typed grant set exceeding the wire-size bound.
    pub fn to_json(&self) -> Result<Vec<u8>, GrantError> {
        let grants = self
            .assignments
            .iter()
            .map(|(scope, permissions)| Assignment {
                scope: *scope,
                permissions: permissions
                    .iter()
                    .map(|permission| permission.definition().identifier.to_string())
                    .collect(),
            })
            .collect();
        let bytes = serde_json::to_vec(&GrantDocument { version: 1, grants })
            .map_err(|_| GrantError::InvalidDocument)?;
        if bytes.len() > MAX_GRANT_DOCUMENT_BYTES {
            return Err(GrantError::TooLarge);
        }
        Ok(bytes)
    }

    /// Explicit authority for the fixed development identity, never a failure fallback.
    #[must_use]
    pub fn development() -> Self {
        let mut assignments: BTreeMap<GrantScope, BTreeSet<Capability>> = BTreeMap::new();
        for &permission in ALL_CAPABILITIES {
            let scope = match permission.definition().scope {
                PermissionScope::Global => GrantScope::Global,
                PermissionScope::Namespace => GrantScope::AllNamespaces,
            };
            assignments.entry(scope).or_default().insert(permission);
        }
        Self { assignments }
    }

    /// Authorize a global operation only through an explicit compatible global grant.
    #[must_use]
    pub fn allows_global(&self, capability: Capability) -> bool {
        self.contains(GrantScope::Global, capability)
    }

    /// Authorize only the listed Namespace permission, or its explicit all-Namespace grant.
    #[must_use]
    pub fn allows_namespace(&self, capability: Capability, namespace_id: NamespaceId) -> bool {
        self.contains(GrantScope::AllNamespaces, capability)
            || self.contains(GrantScope::Namespace { namespace_id }, capability)
    }

    /// Return only Namespaces assigned this capability; other permissions cannot widen it.
    #[must_use]
    pub fn visibility(&self, capability: Capability) -> VisibilityScope {
        if self.contains(GrantScope::AllNamespaces, capability) {
            return VisibilityScope::All;
        }
        let ids: BTreeSet<_> = self
            .assignments
            .iter()
            .filter_map(|(scope, permissions)| {
                if let GrantScope::Namespace { namespace_id } = scope
                    && permissions.contains(&capability)
                {
                    return Some(*namespace_id);
                }
                None
            })
            .collect();
        if ids.is_empty() {
            VisibilityScope::None
        } else {
            VisibilityScope::Namespaces(ids)
        }
    }

    fn contains(&self, scope: GrantScope, capability: Capability) -> bool {
        self.assignments
            .get(&scope)
            .is_some_and(|permissions| permissions.contains(&capability))
    }
}

#[cfg(test)]
mod tests;
