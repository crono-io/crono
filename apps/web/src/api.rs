//! Browser-only client for the public Crono HTTP contract.
//!
//! All pages use this module instead of embedding transport details. Requests
//! are same-origin under `/api`; Trunk proxies that prefix during local
//! development, while production can route it to the independently deployed
//! server. Structured error fields are retained so forms can place safe
//! server-side validation messages beside the relevant control.

use crono_api::{
    CreateJobRequest, CreateNamespaceRequest, CreateRunRequest, CreateTargetRequest,
    CreateTargetSetRequest, ErrorEnvelope, ExecutorKind, JobResource, NamespaceResource,
    OverviewResource, Page, RunResource, TargetResource, TargetSetResource, WorkerResource,
};
use gloo_net::http::{Request, Response};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

const API_ROOT: &str = "/api";
const MAX_COLLECTION_PAGES: usize = 100;

/// Browser-safe API failure with optional field placement metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
}

pub type ApiResult<T> = Result<T, ApiError>;

pub async fn overview() -> ApiResult<OverviewResource> {
    get(&format!("{API_ROOT}/overview")).await
}

pub async fn list_namespaces() -> ApiResult<Page<NamespaceResource>> {
    get(&format!("{API_ROOT}/namespaces?limit=100")).await
}

pub async fn all_namespaces() -> ApiResult<Vec<NamespaceResource>> {
    get_all(&format!("{API_ROOT}/namespaces")).await
}

pub async fn create_namespace(name: String) -> ApiResult<NamespaceResource> {
    post(
        &format!("{API_ROOT}/namespaces"),
        &CreateNamespaceRequest { name },
    )
    .await
}

pub async fn list_jobs(namespace_id: Uuid) -> ApiResult<Page<JobResource>> {
    get(&format!("{}?limit=100", jobs_path(namespace_id))).await
}

pub async fn all_jobs(namespace_id: Uuid) -> ApiResult<Vec<JobResource>> {
    get_all(&jobs_path(namespace_id)).await
}

pub async fn create_job(namespace_id: Uuid, name: String, queue: String) -> ApiResult<JobResource> {
    post(
        &jobs_path(namespace_id),
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

pub async fn list_targets(namespace_id: Uuid) -> ApiResult<Page<TargetResource>> {
    get(&format!("{}?limit=100", targets_path(namespace_id))).await
}

pub async fn all_targets(namespace_id: Uuid) -> ApiResult<Vec<TargetResource>> {
    get_all(&targets_path(namespace_id)).await
}

pub async fn create_target(namespace_id: Uuid, name: String) -> ApiResult<TargetResource> {
    post(
        &targets_path(namespace_id),
        &CreateTargetRequest {
            name,
            arguments: Vec::new(),
        },
    )
    .await
}

pub async fn list_target_sets(namespace_id: Uuid) -> ApiResult<Page<TargetSetResource>> {
    get(&format!(
        "{API_ROOT}/namespaces/{namespace_id}/target-sets?limit=100"
    ))
    .await
}

pub async fn create_target_set(
    namespace_id: Uuid,
    name: String,
    target_ids: Vec<Uuid>,
) -> ApiResult<TargetSetResource> {
    post(
        &format!("{API_ROOT}/namespaces/{namespace_id}/target-sets"),
        &CreateTargetSetRequest { name, target_ids },
    )
    .await
}

pub async fn list_runs() -> ApiResult<Page<RunResource>> {
    get(&format!("{API_ROOT}/runs?limit=100")).await
}

pub async fn create_run(job_id: Uuid, target_id: Uuid) -> ApiResult<RunResource> {
    post(&format!("{API_ROOT}/runs"), &run_request(job_id, target_id)).await
}

fn jobs_path(namespace_id: Uuid) -> String {
    format!("{API_ROOT}/namespaces/{namespace_id}/jobs")
}

fn targets_path(namespace_id: Uuid) -> String {
    format!("{API_ROOT}/namespaces/{namespace_id}/targets")
}

fn run_request(job_id: Uuid, target_id: Uuid) -> CreateRunRequest {
    CreateRunRequest {
        request_id: Uuid::now_v7(),
        job_id,
        target_id,
    }
}

pub async fn list_workers() -> ApiResult<Page<WorkerResource>> {
    get(&format!("{API_ROOT}/workers?limit=100")).await
}

async fn get<T: DeserializeOwned>(url: &str) -> ApiResult<T> {
    let response = Request::get(url)
        .send()
        .await
        .map_err(|error| client_error(format!("Crono API is unreachable: {error}")))?;
    decode(response).await
}

async fn post<T, B>(url: &str, body: &B) -> ApiResult<T>
where
    T: DeserializeOwned,
    B: Serialize,
{
    let request = Request::post(url)
        .json(body)
        .map_err(|error| client_error(format!("Could not encode the request: {error}")))?;
    let response = request
        .send()
        .await
        .map_err(|error| client_error(format!("Crono API is unreachable: {error}")))?;
    decode(response).await
}

async fn decode<T: DeserializeOwned>(response: Response) -> ApiResult<T> {
    if response.ok() {
        return response
            .json()
            .await
            .map_err(|error| client_error(format!("Crono API returned invalid JSON: {error}")));
    }
    let status = response.status();
    response.json::<ErrorEnvelope>().await.map_or_else(
        |_| {
            Err(client_error(format!(
                "Crono API request failed with HTTP {status}"
            )))
        },
        |envelope| {
            Err(ApiError {
                code: envelope.error.code,
                message: envelope.error.message,
                field: envelope.error.field,
            })
        },
    )
}

async fn get_all<T: DeserializeOwned>(base_url: &str) -> ApiResult<Vec<T>> {
    let mut items = Vec::new();
    let mut after = None::<String>;
    for _ in 0..MAX_COLLECTION_PAGES {
        let separator = if base_url.contains('?') { '&' } else { '?' };
        let url = after.as_ref().map_or_else(
            || format!("{base_url}{separator}limit=100"),
            |cursor| format!("{base_url}{separator}limit=100&after={cursor}"),
        );
        let page: Page<T> = get(&url).await?;
        items.extend(page.items);
        let Some(cursor) = page.next_cursor else {
            return Ok(items);
        };
        after = Some(cursor);
    }
    Err(client_error(
        "Crono API returned more collection pages than the browser will load".to_string(),
    ))
}

fn client_error(message: String) -> ApiError {
    ApiError {
        code: "client_error".to_string(),
        message,
        field: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{jobs_path, run_request, targets_path};
    use uuid::Uuid;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn relationship_requests_retain_selected_uuids() {
        let namespace_id = Uuid::from_u128(1);
        let job_id = Uuid::from_u128(2);
        let target_id = Uuid::from_u128(3);

        assert_eq!(
            jobs_path(namespace_id),
            format!("/api/namespaces/{namespace_id}/jobs")
        );
        assert_eq!(
            targets_path(namespace_id),
            format!("/api/namespaces/{namespace_id}/targets")
        );
        let request = run_request(job_id, target_id);
        assert_eq!(request.job_id, job_id);
        assert_eq!(request.target_id, target_id);
    }
}
