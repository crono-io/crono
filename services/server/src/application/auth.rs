//! Replaceable identity and authorization contracts.
//!
//! Authentication providers supply verified principals to a separate policy;
//! development uses a verified static credential and a permit-all policy, but
//! every application operation still requests the same typed capability and
//! resource scope that a future RBAC or external policy adapter will evaluate.
//! Decisions are side-effect free and never trust client-provided roles.

use crate::domain::{
    JobId, NamespaceId, QueueId, ScheduleId, TargetId, TargetSetId, WorkflowId, WorkflowRunId,
};
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

/// Provider-neutral identity established before application use-case execution.
///
/// An external identity is the pair `(issuer, id)`, never an email address.
/// Local identities have no issuer. Only trusted server/provider code may
/// construct this type after verification; it cannot be deserialized from requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    id: String,
    issuer: Option<String>,
    kind: PrincipalKind,
}

impl Principal {
    /// Construct a server-local identity after verification by a trusted adapter.
    #[must_use]
    pub fn new(id: String, kind: PrincipalKind) -> Self {
        Self {
            id,
            issuer: None,
            kind,
        }
    }
    /// Construct an external identity from a verified issuer and subject.
    ///
    /// The verifier must establish the issuer's trust and the subject's validity.
    /// Policies must compare both values: subjects from different issuers differ.
    #[must_use]
    pub fn from_issuer(issuer: String, subject: String, kind: PrincipalKind) -> Self {
        Self {
            id: subject,
            issuer: Some(issuer),
            kind,
        }
    }
    /// Return the opaque subject, scoped to `issuer()` for external identities.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Return the verified issuing authority, if this is an external identity.
    #[must_use]
    pub fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
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
    /// Attach a server-issued correlation ID to an already verified principal.
    ///
    /// Trusted adapters must authenticate first; client request data cannot
    /// construct this context or choose its authority.
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

/// Stable permission vocabulary independent from future role names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    NamespaceCreate,
    NamespaceRead,
    /// Delete an empty Namespace after resource-specific authorization.
    NamespaceDelete,
    QueueCreate,
    QueueRead,
    QueueUpdate,
    QueueDelete,
    JobCreate,
    JobRead,
    JobUpdate,
    JobExecute,
    TargetCreate,
    TargetRead,
    TargetUpdate,
    /// Delete one Target after resource-specific authorization.
    TargetDelete,
    TargetUse,
    TargetSetCreate,
    TargetSetRead,
    TargetSetUpdate,
    TargetSetUse,
    ScheduleCreate,
    ScheduleRead,
    ScheduleUpdate,
    WorkflowCreate,
    WorkflowRead,
    WorkflowUpdate,
    WorkflowDelete,
    WorkflowExecute,
    WorkflowRunRead,
    WorkflowRunCancel,
    RunCreate,
    RunRead,
    WorkerRead,
    /// Read system-wide operational state without exposing execution payloads.
    MonitorRead,
}

/// Resource named by one authorization decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceScope {
    ControlPlane,
    Namespace(NamespaceId),
    Queue(QueueId),
    Job(JobId),
    Target(TargetId),
    TargetSet(TargetSetId),
    Schedule(ScheduleId),
    Workflow(WorkflowId),
    WorkflowRun(WorkflowRunId),
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
        Authorizer, Capability, PermitAllAuthorizer, Principal, PrincipalKind, RequestContext,
        ResourceScope, VisibilityScope,
    };
    use anyhow::Result;
    use uuid::Uuid;

    #[test]
    fn context_retains_verified_identity_and_server_request_id() {
        let request_id = Uuid::now_v7();
        let context = RequestContext::new(
            request_id,
            Principal::new("development/local".to_string(), PrincipalKind::Development),
        );

        assert_eq!(context.principal().id(), "development/local");
        assert_eq!(context.principal().kind(), PrincipalKind::Development);
        assert_eq!(context.request_id(), request_id);
    }

    #[tokio::test]
    async fn permit_all_policy_exercises_typed_decisions() -> Result<()> {
        let context = RequestContext::new(
            Uuid::now_v7(),
            Principal::new("development/local".to_string(), PrincipalKind::Development),
        );
        let policy = PermitAllAuthorizer;
        let capabilities = [
            Capability::NamespaceCreate,
            Capability::NamespaceRead,
            Capability::NamespaceDelete,
            Capability::QueueCreate,
            Capability::QueueRead,
            Capability::QueueUpdate,
            Capability::QueueDelete,
            Capability::JobCreate,
            Capability::JobRead,
            Capability::JobExecute,
            Capability::TargetCreate,
            Capability::TargetRead,
            Capability::TargetDelete,
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
