//! Side-effect-free enforcement of provider-neutral capability/scope grants.
//!
//! UUID resource references are resolved through a narrow read-only port; neither
//! full workload configuration nor client-provided Namespace ownership is trusted.
//! All-Namespace grants avoid unnecessary lookups, while scoped authority checks
//! the server-owned membership. Visibility is independent for every read permission.

use super::{
    AuthorizationError, Authorizer, Capability, PermissionScope, RequestContext, ResourceScope,
    VisibilityScope,
};
use crate::domain::NamespaceId;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only authoritative membership lookup, independent of identity providers.
#[async_trait]
pub trait ResourceNamespaceResolver: Send + Sync {
    /// Resolve only Namespace ownership for a namespaced UUID resource.
    ///
    /// Missing records return `None`; dependency failures return `Unavailable`.
    /// Implementations never expose payloads or accept client ownership assertions.
    /// Global resources and direct Namespace references are handled without this port.
    async fn namespace_for(
        &self,
        resource: &ResourceScope,
    ) -> Result<Option<NamespaceId>, AuthorizationError>;
}

/// Deny-by-default policy using only verified grants and authoritative membership.
pub struct GrantAuthorizer {
    resolver: Arc<dyn ResourceNamespaceResolver>,
}

impl GrantAuthorizer {
    /// Inject resource membership independently of the configured authentication provider.
    #[must_use]
    pub fn new(resolver: Arc<dyn ResourceNamespaceResolver>) -> Self {
        Self { resolver }
    }
}

/// Keep read-protected graphs and execution history indistinguishable from missing data.
fn denial(capability: Capability) -> AuthorizationError {
    match capability {
        Capability::RunRead | Capability::WorkflowRead | Capability::WorkflowRunRead => {
            AuthorizationError::NotFound
        }
        _ => AuthorizationError::Forbidden,
    }
}

#[async_trait]
impl Authorizer for GrantAuthorizer {
    /// Authorize an exact compatible resource using verified global or Namespace grants.
    ///
    /// No role names imply authority. Missing grants deny before resource lookup;
    /// missing metadata and hidden history return not-found, and outages fail closed.
    async fn authorize(
        &self,
        context: &RequestContext,
        capability: Capability,
        resource: &ResourceScope,
    ) -> Result<(), AuthorizationError> {
        if !capability.accepts_resource(resource) {
            return Err(AuthorizationError::Forbidden);
        }
        let grants = context.grants();
        if capability.definition().scope == PermissionScope::Global {
            return if grants.allows_global(capability) {
                Ok(())
            } else {
                Err(AuthorizationError::Forbidden)
            };
        }
        let visibility = grants.visibility(capability);
        match visibility {
            VisibilityScope::None => return Err(denial(capability)),
            VisibilityScope::All => return Ok(()),
            VisibilityScope::Namespaces(_) => {}
        }
        let namespace = match resource {
            ResourceScope::Namespace(id) => *id,
            _ => self
                .resolver
                .namespace_for(resource)
                .await?
                .ok_or(AuthorizationError::NotFound)?,
        };
        if grants.allows_namespace(capability, namespace) {
            Ok(())
        } else {
            Err(denial(capability))
        }
    }

    /// Restrict Namespace reads by this capability alone; global or mutation requests deny.
    async fn visibility(
        &self,
        context: &RequestContext,
        capability: Capability,
    ) -> Result<VisibilityScope, AuthorizationError> {
        if capability.definition().scope != PermissionScope::Namespace
            || capability.definition().identifier.rsplit('.').next() != Some("read")
        {
            return Err(AuthorizationError::Forbidden);
        }
        Ok(context.grants().visibility(capability))
    }
}

#[cfg(test)]
mod tests;
