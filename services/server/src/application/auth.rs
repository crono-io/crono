//! Replaceable identity and authorization contracts.
//!
//! Development uses a server-created principal and a permit-all policy, but
//! every application operation still requests the same typed capability and
//! resource scope that a future RBAC or external policy adapter will evaluate.
//! Decisions are side-effect free and never trust client-provided roles.

use crate::domain::{JobId, NamespaceId, ScheduleId, TargetId, TargetSetId};
use async_trait::async_trait;
use std::{collections::BTreeSet, error::Error, fmt};
use uuid::Uuid;

/// Server-verified caller category; request payloads cannot select this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    Development,
    Human,
    Service,
    System,
}

/// Opaque identity established before application use-case execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    id: String,
    kind: PrincipalKind,
}

impl Principal {
    #[must_use]
    pub fn new(id: String, kind: PrincipalKind) -> Self {
        Self { id, kind }
    }
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    #[must_use]
    pub const fn kind(&self) -> PrincipalKind {
        self.kind
    }
}

/// Trusted caller and correlation information passed to every use case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    request_id: Uuid,
    principal: Principal,
}

impl RequestContext {
    #[must_use]
    pub const fn new(request_id: Uuid, principal: Principal) -> Self {
        Self {
            request_id,
            principal,
        }
    }
    #[must_use]
    pub const fn request_id(&self) -> Uuid {
        self.request_id
    }
    #[must_use]
    pub const fn principal(&self) -> &Principal {
        &self.principal
    }
}

/// Development identity provider used until credential verification exists.
#[derive(Debug, Clone, Copy, Default)]
pub struct DevelopmentIdentity;

impl DevelopmentIdentity {
    /// Build a server-owned development context for one HTTP request.
    #[must_use]
    pub fn context(self) -> RequestContext {
        RequestContext::new(
            Uuid::now_v7(),
            Principal::new("development/local".to_string(), PrincipalKind::Development),
        )
    }
}

/// Stable permission vocabulary independent from future role names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    NamespaceCreate,
    NamespaceRead,
    JobCreate,
    JobRead,
    JobExecute,
    TargetCreate,
    TargetRead,
    TargetUse,
    TargetSetCreate,
    TargetSetRead,
    ScheduleCreate,
    ScheduleRead,
    ScheduleUpdate,
    RunCreate,
    RunRead,
    WorkerRead,
}

/// Resource named by one authorization decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceScope {
    ControlPlane,
    Namespace(NamespaceId),
    Job(JobId),
    Target(TargetId),
    TargetSet(TargetSetId),
    Schedule(ScheduleId),
    Run(Uuid),
}

/// SQL visibility constraint applied before counting and pagination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibilityScope {
    All,
    Namespaces(BTreeSet<NamespaceId>),
    None,
}

/// Denial or policy dependency failure; none of these variants grants access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationError {
    Unauthenticated,
    Forbidden,
    Unavailable,
}

impl fmt::Display for AuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthenticated => formatter.write_str("authentication is required"),
            Self::Forbidden => formatter.write_str("operation is not authorized"),
            Self::Unavailable => formatter.write_str("authorization policy is unavailable"),
        }
    }
}

impl Error for AuthorizationError {}

/// Side-effect-free authorization dependency used by every public use case.
#[async_trait]
pub trait Authorizer: Send + Sync {
    /// Grant or reject one capability over one typed resource.
    async fn authorize(
        &self,
        context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError>;

    /// Return the Namespace visibility to apply before querying list data.
    async fn visibility(
        &self,
        context: &RequestContext,
        capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError>;
}

/// Development policy that exercises authorization while granting all access.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermitAllAuthorizer;

#[async_trait]
impl Authorizer for PermitAllAuthorizer {
    async fn authorize(
        &self,
        _context: &RequestContext,
        _capability: Capability,
        _resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        Ok(())
    }

    async fn visibility(
        &self,
        _context: &RequestContext,
        _capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        Ok(VisibilityScope::All)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Authorizer, Capability, DevelopmentIdentity, PermitAllAuthorizer, PrincipalKind,
        ResourceScope, VisibilityScope,
    };
    use anyhow::Result;

    #[test]
    fn development_identity_is_server_owned_and_request_scoped() {
        let first = DevelopmentIdentity.context();
        let second = DevelopmentIdentity.context();

        assert_eq!(first.principal().id(), "development/local");
        assert_eq!(first.principal().kind(), PrincipalKind::Development);
        assert_ne!(first.request_id(), second.request_id());
    }

    #[tokio::test]
    async fn permit_all_policy_exercises_typed_decisions() -> Result<()> {
        let context = DevelopmentIdentity.context();
        let policy = PermitAllAuthorizer;
        let capabilities = [
            Capability::NamespaceCreate,
            Capability::NamespaceRead,
            Capability::JobCreate,
            Capability::JobRead,
            Capability::JobExecute,
            Capability::TargetCreate,
            Capability::TargetRead,
            Capability::TargetUse,
            Capability::TargetSetCreate,
            Capability::TargetSetRead,
            Capability::ScheduleCreate,
            Capability::ScheduleRead,
            Capability::ScheduleUpdate,
            Capability::RunCreate,
            Capability::RunRead,
            Capability::WorkerRead,
        ];

        for capability in capabilities {
            policy
                .authorize(&context, capability, &ResourceScope::ControlPlane)
                .await?;
        }
        assert_eq!(
            policy.visibility(&context, Capability::RunRead).await?,
            VisibilityScope::All
        );
        Ok(())
    }
}
