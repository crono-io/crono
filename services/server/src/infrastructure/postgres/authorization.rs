//! Minimal Namespace membership queries for capability authorization.
//!
//! These read-only queries return UUID metadata only, before application code loads
//! executable configuration, inputs, snapshots, graphs, or output. Runs use their
//! retained Job relationship; Workflow invocations use their stored Namespace.
//! No identity provider, role, or token participates in resource ownership lookup.

use super::PostgresStore;
use crate::{
    application::{AuthorizationError, ResourceNamespaceResolver, ResourceScope},
    domain::NamespaceId,
};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
impl ResourceNamespaceResolver for PostgresStore {
    /// Resolve authoritative Namespace metadata without exposing workload data.
    /// Global resources are invalid inputs; missing rows remain absent and SQL failures fail closed.
    async fn namespace_for(
        &self,
        resource: &ResourceScope,
    ) -> Result<Option<NamespaceId>, AuthorizationError> {
        let (query, id) = match resource {
            ResourceScope::Namespace(id) => return Ok(Some(*id)),
            ResourceScope::Job(id) => (
                "SELECT namespace_id FROM crono.jobs WHERE id = $1",
                id.get(),
            ),
            ResourceScope::Target(id) => (
                "SELECT namespace_id FROM crono.targets WHERE id = $1",
                id.get(),
            ),
            ResourceScope::TargetSet(id) => (
                "SELECT namespace_id FROM crono.target_sets WHERE id = $1",
                id.get(),
            ),
            ResourceScope::Schedule(id) => (
                "SELECT namespace_id FROM crono.schedules WHERE id = $1",
                id.get(),
            ),
            ResourceScope::Workflow(id) => (
                "SELECT namespace_id FROM crono.workflows WHERE id = $1",
                id.get(),
            ),
            ResourceScope::WorkflowRun(id) => (
                "SELECT namespace_id FROM crono.workflow_runs WHERE id = $1",
                id.get(),
            ),
            ResourceScope::Run(id) => (
                "SELECT j.namespace_id FROM crono.runs r JOIN crono.jobs j ON j.id = r.job_id WHERE r.id = $1",
                *id,
            ),
            ResourceScope::ControlPlane | ResourceScope::Queue(_) => {
                return Err(AuthorizationError::Forbidden);
            }
        };
        sqlx::query_scalar::<_, Uuid>(query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map(|id| id.map(NamespaceId::new))
            .map_err(|_| AuthorizationError::Unavailable)
    }
}
