//! Authorized orchestration of domain parsing and persistence operations.

use super::{
    ApplicationError, Authorizer, Capability, ControlPlaneStore, JobRecord, Overview, Page,
    RequestContext, ResourceScope, RunRecord, TargetRecord,
};
use crate::domain::{Namespace, NamespaceName, QueueName, ResourceName, RunId};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_LIMIT: u16 = 50;
const MAX_LIMIT: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRunOutcome {
    pub run: RunRecord,
    pub created: bool,
}

#[derive(Clone)]
pub struct Application {
    store: Arc<dyn ControlPlaneStore>,
    authorizer: Arc<dyn Authorizer>,
}

impl Application {
    #[must_use]
    pub fn new(store: Arc<dyn ControlPlaneStore>, authorizer: Arc<dyn Authorizer>) -> Self {
        Self { store, authorizer }
    }

    /// Authorize, validate, and persist one Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_namespace(
        &self,
        context: &RequestContext,
        value: &str,
    ) -> Result<Namespace, ApplicationError> {
        self.authorizer
            .authorize(
                context,
                Capability::NamespaceCreate,
                &ResourceScope::ControlPlane,
            )
            .await?;
        let name = NamespaceName::parse(value).map_err(invalid)?;
        Ok(self.store.create_namespace(&name).await?)
    }

    /// List only Namespaces visible to the established principal.
    ///
    /// # Errors
    ///
    /// Returns invalid pagination, authorization, or dependency failures.
    pub async fn list_namespaces(
        &self,
        context: &RequestContext,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<Namespace>, ApplicationError> {
        let visibility = self
            .authorizer
            .visibility(context, Capability::NamespaceRead)
            .await?;
        Ok(self
            .store
            .list_namespaces(&visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read one Namespace after its resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or dependency failures.
    pub async fn get_namespace(
        &self,
        context: &RequestContext,
        value: &str,
    ) -> Result<Namespace, ApplicationError> {
        let name = NamespaceName::parse(value).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::NamespaceRead,
                &ResourceScope::Namespace(name.to_string()),
            )
            .await?;
        Ok(self.store.get_namespace(&name).await?)
    }

    /// Create a Job and immutable version 1 after Namespace authorization.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_job(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
        queue: Option<&str>,
    ) -> Result<JobRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        let queue = QueueName::parse(queue.unwrap_or("default")).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobCreate,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        Ok(self.store.create_job(&namespace, &name, &queue).await?)
    }

    /// List visible Jobs from one authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, pagination, authorization, or dependency failures.
    pub async fn list_jobs(
        &self,
        context: &RequestContext,
        namespace: &str,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobRead,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::JobRead)
            .await?;
        Ok(self
            .store
            .list_jobs(&namespace, &visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read a qualified Job after a resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or dependency failures.
    pub async fn get_job(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
    ) -> Result<JobRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::JobRead,
                &ResourceScope::Job {
                    namespace: namespace.to_string(),
                    job: name.to_string(),
                },
            )
            .await?;
        Ok(self.store.get_job(&namespace, &name).await?)
    }

    /// Create an identity-only Target in an authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, conflict, or dependency failures.
    pub async fn create_target(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
    ) -> Result<TargetRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetCreate,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        Ok(self.store.create_target(&namespace, &name).await?)
    }

    /// List visible Targets from one authorized Namespace.
    ///
    /// # Errors
    ///
    /// Returns validation, pagination, authorization, or dependency failures.
    pub async fn list_targets(
        &self,
        context: &RequestContext,
        namespace: &str,
        limit: Option<u16>,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetRead,
                &ResourceScope::Namespace(namespace.to_string()),
            )
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::TargetRead)
            .await?;
        Ok(self
            .store
            .list_targets(&namespace, &visibility, page_limit(limit)?, after)
            .await?)
    }

    /// Read a qualified Target after a resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns validation, authorization, not-found, or dependency failures.
    pub async fn get_target(
        &self,
        context: &RequestContext,
        namespace: &str,
        name: &str,
    ) -> Result<TargetRecord, ApplicationError> {
        let namespace = NamespaceName::parse(namespace).map_err(invalid)?;
        let name = ResourceName::parse(name).map_err(invalid)?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetRead,
                &ResourceScope::Target {
                    namespace: namespace.to_string(),
                    target: name.to_string(),
                },
            )
            .await?;
        Ok(self.store.get_target(&namespace, &name).await?)
    }

    /// Authorize every Run input and persist an idempotent dispatch request.
    ///
    /// # Errors
    ///
    /// Returns input, authorization, not-found, idempotency, or dependency failures.
    pub async fn create_run(
        &self,
        context: &RequestContext,
        request_id: Uuid,
        job: &str,
        target: &str,
    ) -> Result<CreateRunOutcome, ApplicationError> {
        let (job_namespace, job_name) = qualified(job)?;
        let (target_namespace, target_name) = qualified(target)?;
        if job_namespace != target_namespace {
            return Err(ApplicationError::InvalidInput(
                "Job and Target must belong to the same Namespace".to_string(),
            ));
        }
        self.authorizer
            .authorize(
                context,
                Capability::RunCreate,
                &ResourceScope::Namespace(job_namespace.to_string()),
            )
            .await?;
        self.authorizer
            .authorize(
                context,
                Capability::JobExecute,
                &ResourceScope::Job {
                    namespace: job_namespace.to_string(),
                    job: job_name.to_string(),
                },
            )
            .await?;
        self.authorizer
            .authorize(
                context,
                Capability::TargetUse,
                &ResourceScope::Target {
                    namespace: target_namespace.to_string(),
                    target: target_name.to_string(),
                },
            )
            .await?;
        let (run, created) = self
            .store
            .create_run(
                request_id,
                &job_namespace,
                &job_name,
                &target_namespace,
                &target_name,
            )
            .await?;
        Ok(CreateRunOutcome { run, created })
    }

    /// List Runs after applying principal visibility within the SQL query.
    ///
    /// # Errors
    ///
    /// Returns pagination, authorization, or dependency failures.
    pub async fn list_runs(
        &self,
        context: &RequestContext,
        limit: Option<u16>,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, ApplicationError> {
        let visibility = self
            .authorizer
            .visibility(context, Capability::RunRead)
            .await?;
        Ok(self
            .store
            .list_runs(&visibility, page_limit(limit)?, before)
            .await?)
    }

    /// Read one Run after its resource-specific authorization decision.
    ///
    /// # Errors
    ///
    /// Returns authorization, not-found, or dependency failures.
    pub async fn get_run(
        &self,
        context: &RequestContext,
        id: Uuid,
    ) -> Result<RunRecord, ApplicationError> {
        self.authorizer
            .authorize(context, Capability::RunRead, &ResourceScope::Run(id))
            .await?;
        let visibility = self
            .authorizer
            .visibility(context, Capability::RunRead)
            .await?;
        Ok(self.store.get_run(RunId::new(id), &visibility).await?)
    }

    /// Count only resources visible to the established principal.
    ///
    /// # Errors
    ///
    /// Returns authorization or dependency failures.
    pub async fn overview(&self, context: &RequestContext) -> Result<Overview, ApplicationError> {
        let visibility = self
            .authorizer
            .visibility(context, Capability::NamespaceRead)
            .await?;
        Ok(self.store.overview(&visibility).await?)
    }
}

fn page_limit(limit: Option<u16>) -> Result<u16, ApplicationError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    if limit == 0 || limit > MAX_LIMIT {
        Err(ApplicationError::InvalidInput(format!(
            "limit must be between 1 and {MAX_LIMIT}"
        )))
    } else {
        Ok(limit)
    }
}

fn invalid(error: crate::domain::NameError) -> ApplicationError {
    ApplicationError::InvalidInput(error.to_string())
}

fn qualified(value: &str) -> Result<(NamespaceName, ResourceName), ApplicationError> {
    let Some((namespace, resource)) = value.split_once('/') else {
        return Err(ApplicationError::InvalidInput(
            "qualified resource names must use namespace/resource".to_string(),
        ));
    };
    if resource.contains('/') {
        return Err(ApplicationError::InvalidInput(
            "qualified resource names must contain one slash".to_string(),
        ));
    }
    Ok((
        NamespaceName::parse(namespace).map_err(invalid)?,
        ResourceName::parse(resource).map_err(invalid)?,
    ))
}
