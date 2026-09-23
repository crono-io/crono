//! Browser-only client for the public Crono HTTP contract.
//!
//! All pages use this module instead of embedding transport details. Requests
//! are same-origin under `/api`; Trunk proxies that prefix during local
//! development, while production can route it to the independently deployed
//! server. Error envelopes are reduced to safe user-facing messages.

use crono_api::{
    CreateJobRequest, CreateNamespaceRequest, CreateRunRequest, CreateTargetRequest, ErrorEnvelope,
    ExecutorKind, JobResource, NamespaceResource, OverviewResource, Page, RunResource,
    TargetResource, WorkerResource,
};
use gloo_net::http::{Request, Response};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

const API_ROOT: &str = "/api";

/// Browser-safe API failure message.
pub type ApiResult<T> = Result<T, String>;

pub async fn overview() -> ApiResult<OverviewResource> {
    get(&format!("{API_ROOT}/overview")).await
}

pub async fn list_namespaces() -> ApiResult<Page<NamespaceResource>> {
    get(&format!("{API_ROOT}/namespaces?limit=100")).await
}

pub async fn create_namespace(name: String) -> ApiResult<NamespaceResource> {
    post(
        &format!("{API_ROOT}/namespaces"),
        &CreateNamespaceRequest { name },
    )
    .await
}

pub async fn list_jobs(namespace: &str) -> ApiResult<Page<JobResource>> {
    get(&format!("{API_ROOT}/namespaces/{namespace}/jobs?limit=100")).await
}

pub async fn create_job(namespace: &str, name: String, queue: String) -> ApiResult<JobResource> {
    post(
        &format!("{API_ROOT}/namespaces/{namespace}/jobs"),
        &CreateJobRequest {
            name,
            queue,
            executor: ExecutorKind::Noop,
            executable: None,
            arguments: Vec::new(),
            idempotent: false,
            max_attempts: 1,
            retry_initial_seconds: 1,
            retry_max_seconds: 60,
            retry_multiplier: 2.0,
            retry_jitter: 0.2,
        },
    )
    .await
}

pub async fn list_targets(namespace: &str) -> ApiResult<Page<TargetResource>> {
    get(&format!(
        "{API_ROOT}/namespaces/{namespace}/targets?limit=100"
    ))
    .await
}

pub async fn create_target(namespace: &str, name: String) -> ApiResult<TargetResource> {
    post(
        &format!("{API_ROOT}/namespaces/{namespace}/targets"),
        &CreateTargetRequest {
            name,
            arguments: Vec::new(),
        },
    )
    .await
}

pub async fn list_runs() -> ApiResult<Page<RunResource>> {
    get(&format!("{API_ROOT}/runs?limit=100")).await
}

pub async fn create_run(job: String, target: String) -> ApiResult<RunResource> {
    post(
        &format!("{API_ROOT}/runs"),
        &CreateRunRequest {
            request_id: Uuid::now_v7(),
            job,
            target,
        },
    )
    .await
}

pub async fn list_workers() -> ApiResult<Page<WorkerResource>> {
    get(&format!("{API_ROOT}/workers?limit=100")).await
}

async fn get<T: DeserializeOwned>(url: &str) -> ApiResult<T> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|error| format!("Crono API is unreachable: {error}"))?;
    decode(response).await
}

async fn post<T, B>(url: &str, body: &B) -> ApiResult<T>
where
    T: DeserializeOwned,
    B: Serialize,
{
    let request = Request::post(url)
        .json(body)
        .map_err(|error| format!("Could not encode the request: {error}"))?;
    let response = request
        .send()
        .await
        .map_err(|error| format!("Crono API is unreachable: {error}"))?;
    decode(response).await
}

async fn decode<T: DeserializeOwned>(response: Response) -> ApiResult<T> {
    if response.ok() {
        return response
            .json()
            .await
            .map_err(|error| format!("Crono API returned invalid JSON: {error}"));
    }
    let status = response.status();
    let message = response.json::<ErrorEnvelope>().await.map_or_else(
        |_| format!("Crono API request failed with HTTP {status}"),
        |envelope| envelope.error.message,
    );
    Err(message)
}
