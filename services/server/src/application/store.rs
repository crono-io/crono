//! Persistence port for authorized catalog, Run, and outbox operations.

use super::{JobRecord, Overview, Page, RunRecord, TargetRecord, VisibilityScope};
use crate::domain::{DispatchId, Namespace, NamespaceName, QueueName, ResourceName, RunId};
use async_trait::async_trait;
use std::{error::Error, fmt};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRecord {
    pub id: DispatchId,
    pub run_id: RunId,
    pub subject: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    NotFound,
    Conflict,
    IdempotencyConflict,
    Unavailable,
    Internal,
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "resource was not found",
            Self::Conflict => "resource already exists",
            Self::IdempotencyConflict => "idempotency key conflicts with an existing request",
            Self::Unavailable => "persistence is unavailable",
            Self::Internal => "persistence failed",
        })
    }
}

impl Error for StoreError {}

#[async_trait]
pub trait ControlPlaneStore: Send + Sync {
    async fn create_namespace(&self, name: &NamespaceName) -> Result<Namespace, StoreError>;
    async fn list_namespaces(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<Namespace>, StoreError>;
    async fn get_namespace(&self, name: &NamespaceName) -> Result<Namespace, StoreError>;
    async fn create_job(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
        queue: &QueueName,
    ) -> Result<JobRecord, StoreError>;
    async fn list_jobs(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<JobRecord>, StoreError>;
    async fn get_job(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<JobRecord, StoreError>;
    async fn create_target(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<TargetRecord, StoreError>;
    async fn list_targets(
        &self,
        namespace: &NamespaceName,
        visibility: &VisibilityScope,
        limit: u16,
        after: Option<&str>,
    ) -> Result<Page<TargetRecord>, StoreError>;
    async fn get_target(
        &self,
        namespace: &NamespaceName,
        name: &ResourceName,
    ) -> Result<TargetRecord, StoreError>;
    async fn create_run(
        &self,
        request_id: Uuid,
        job_namespace: &NamespaceName,
        job_name: &ResourceName,
        target_namespace: &NamespaceName,
        target_name: &ResourceName,
    ) -> Result<(RunRecord, bool), StoreError>;
    async fn list_runs(
        &self,
        visibility: &VisibilityScope,
        limit: u16,
        before: Option<Uuid>,
    ) -> Result<Page<RunRecord>, StoreError>;
    async fn get_run(
        &self,
        id: RunId,
        visibility: &VisibilityScope,
    ) -> Result<RunRecord, StoreError>;
    async fn overview(&self, visibility: &VisibilityScope) -> Result<Overview, StoreError>;
    async fn pending_outbox(&self, limit: u16) -> Result<Vec<OutboxRecord>, StoreError>;
    async fn mark_published(
        &self,
        dispatch_id: DispatchId,
        run_id: RunId,
        stream_sequence: u64,
    ) -> Result<(), StoreError>;
    async fn record_publish_failure(
        &self,
        dispatch_id: DispatchId,
        message: &str,
    ) -> Result<(), StoreError>;
    async fn ready(&self) -> bool;
}
