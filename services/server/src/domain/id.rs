//! Strong identifiers for distinct control-plane resource kinds.
//!
//! Each identifier wraps the same transport-neutral integer representation but
//! remains a separate Rust type, preventing accidental interchange between
//! Namespaces, Jobs, Targets, and Target Sets. Allocation and persistence are
//! deliberately outside this pure domain layer.

/// Stable internal identity of a [`super::Namespace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NamespaceId(u128);

impl NamespaceId {
    /// Construct an identifier from an allocator-provided value.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Return the transport-neutral numeric representation.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// Stable internal identity of a [`super::Job`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(u128);

impl JobId {
    /// Construct an identifier from an allocator-provided value.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Return the transport-neutral numeric representation.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// Stable internal identity of a [`super::Target`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(u128);

impl TargetId {
    /// Construct an identifier from an allocator-provided value.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Return the transport-neutral numeric representation.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// Stable internal identity of a [`super::TargetSet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetSetId(u128);

impl TargetSetId {
    /// Construct an identifier from an allocator-provided value.
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    /// Return the transport-neutral numeric representation.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}
