//! Replaceable identity and authorization contracts.
//!
//! Authentication supplies verified identity and scoped grants to an independent
//! policy. Every application operation checks a stable capability against the
//! authoritative resource, and reads apply capability-specific Namespace visibility.
//! IAM role names and raw claims never enter use cases. Development supplies explicit
//! full grants to the same evaluator; permit-all policy is only a test fixture.
//!
//! # Flow Overview
//!
//! A trusted verifier normalizes grants, the request context binds them to identity,
//! and the authorizer resolves resource Namespace metadata before making a decision.
//! Failure never establishes additional authority; decisions never mutate state.

use crate::domain::{
    JobId, NamespaceId, QueueId, ScheduleId, TargetId, TargetSetId, WorkflowId, WorkflowRunId,
};
use async_trait::async_trait;
use std::{collections::BTreeSet, error::Error, fmt};
use uuid::Uuid;

mod capabilities;
mod grants;
mod policy;
pub use capabilities::{ALL_CAPABILITIES, Capability, PermissionDefinition, PermissionScope};
pub use grants::{GrantError, GrantScope, GrantSet};
pub use policy::{GrantAuthorizer, ResourceNamespaceResolver};

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

/// Identity and normalized authority supplied only after credential verification.
///
/// This type is never deserialized from client requests. Providers must validate
/// issuer, audience, validity, and delegated limits before constructing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedCaller {
    principal: Principal,
    grants: GrantSet,
}

impl AuthenticatedCaller {
    /// Bind verified identity to verified grants; no role names or token bytes are retained.
    #[must_use]
    pub const fn new(principal: Principal, grants: GrantSet) -> Self {
        Self { principal, grants }
    }

    /// Read the verified, issuer-scoped identity.
    #[must_use]
    pub const fn principal(&self) -> &Principal {
        &self.principal
    }

    /// Read normalized authority without raw provider claims.
    #[must_use]
    pub const fn grants(&self) -> &GrantSet {
        &self.grants
    }
}

impl From<Principal> for AuthenticatedCaller {
    /// Identity alone supplies no permissions; retained for identity-only test adapters.
    fn from(principal: Principal) -> Self {
        Self::new(principal, GrantSet::default())
    }
}

/// Trusted caller and correlation information passed to every use case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    request_id: Uuid,
    caller: AuthenticatedCaller,
}

impl RequestContext {
    /// Attach a server-issued correlation ID to verified identity and grants.
    ///
    /// Trusted adapters must authenticate first; client request data cannot
    /// construct this context or choose its authority.
    #[must_use]
    pub fn new(request_id: Uuid, caller: impl Into<AuthenticatedCaller>) -> Self {
        Self {
            request_id,
            caller: caller.into(),
        }
    }
    #[must_use]
    pub const fn request_id(&self) -> Uuid {
        self.request_id
    }
    #[must_use]
    pub const fn principal(&self) -> &Principal {
        &self.caller.principal
    }

    /// Return only the authority established by trusted verification code.
    #[must_use]
    pub const fn grants(&self) -> &GrantSet {
        &self.caller.grants
    }
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

/// Resource kinds accepted by the stable permission registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    ControlPlane,
    Namespace,
    Queue,
    Job,
    Target,
    TargetSet,
    Schedule,
    Workflow,
    WorkflowRun,
    Run,
}

impl ResourceScope {
    /// Classify a typed resource without trusting any caller-supplied Namespace.
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        match self {
            Self::ControlPlane => ResourceKind::ControlPlane,
            Self::Namespace(_) => ResourceKind::Namespace,
            Self::Queue(_) => ResourceKind::Queue,
            Self::Job(_) => ResourceKind::Job,
            Self::Target(_) => ResourceKind::Target,
            Self::TargetSet(_) => ResourceKind::TargetSet,
            Self::Schedule(_) => ResourceKind::Schedule,
            Self::Workflow(_) => ResourceKind::Workflow,
            Self::WorkflowRun(_) => ResourceKind::WorkflowRun,
            Self::Run(_) => ResourceKind::Run,
        }
    }
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
    /// Missing scope metadata or a read whose Namespace must remain hidden.
    NotFound,
    Unavailable,
}

impl fmt::Display for AuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthenticated => formatter.write_str("authentication is required"),
            Self::Forbidden => formatter.write_str("operation is not authorized"),
            Self::NotFound => formatter.write_str("resource was not found"),
            Self::Unavailable => formatter.write_str("authorization policy is unavailable"),
        }
    }
}

impl Error for AuthorizationError {}

/// Side-effect-free authorization dependency used by every public use case.
#[async_trait]
pub trait Authorizer: Send + Sync {
    /// Grant only verified authority for this capability and authoritative resource scope.
    async fn authorize(
        &self,
        context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError>;

    /// Return Namespaces with this read permission; absent grants yield no visibility.
    async fn visibility(
        &self,
        context: &RequestContext,
        capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError>;
}

/// Explicit permit-all fixture for tests; production startup uses `GrantAuthorizer`.
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
