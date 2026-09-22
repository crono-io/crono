//! Application query records composed from pure domain entities.

use crate::domain::{Job, JobVersion, NamespaceName, Run, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRecord {
    pub namespace: NamespaceName,
    pub job: Job,
    pub version: JobVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRecord {
    pub namespace: NamespaceName,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run: Run,
    pub job_namespace: NamespaceName,
    pub job_name: crate::domain::ResourceName,
    pub target_namespace: NamespaceName,
    pub target_name: crate::domain::ResourceName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overview {
    pub namespaces: u64,
    pub jobs: u64,
    pub targets: u64,
    pub runs: u64,
}
