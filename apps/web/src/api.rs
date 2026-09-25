//! Browser-only client for the public Crono HTTP contract.
//!
//! All pages use this module instead of embedding transport details. Requests
//! are same-origin under `/api`; Trunk proxies that prefix during local
//! development, while production can route it to the independently deployed
//! server. Structured error fields are retained so forms can place safe
//! server-side validation messages beside the relevant control.

use crono_api::{
    CreateJobRequest, CreateNamespaceRequest, CreateQueueRequest, CreateRunRequest,
    CreateScheduleRequest, CreateTargetRequest, CreateTargetSetRequest, ErrorEnvelope,
    ExecutionTarget, JobResource, MonitorResource, NamespaceResource, OverviewResource, Page,
    QueueResource, RunAttemptResource, RunBatchResource, RunResource, ScheduleResource,
    TargetResource, TargetSetResource, UpdateJobRequest, UpdateQueueRequest, UpdateScheduleRequest,
    UpdateTargetRequest, UpdateTargetSetRequest, WorkerResource,
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

/// Read the operator-only system snapshot through the typed API contract.
pub async fn monitor() -> ApiResult<MonitorResource> {
    get(&format!("{API_ROOT}/monitor")).await
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

pub async fn list_queues() -> ApiResult<Page<QueueResource>> {
    get(&format!("{API_ROOT}/queues?limit=100")).await
}

pub async fn all_queues() -> ApiResult<Vec<QueueResource>> {
    get_all(&format!("{API_ROOT}/queues")).await
}

pub async fn create_queue(name: String, description: Option<String>) -> ApiResult<QueueResource> {
    post(
        &format!("{API_ROOT}/queues"),
        &CreateQueueRequest { name, description },
    )
    .await
}

pub async fn update_queue(
    id: Uuid,
    name: String,
    description: Option<String>,
    enabled: bool,
) -> ApiResult<QueueResource> {
    put(
        &format!("{API_ROOT}/queues/{id}"),
        &UpdateQueueRequest {
            name,
            description,
            enabled,
        },
    )
    .await
}

pub async fn delete_queue(id: Uuid) -> ApiResult<()> {
    delete_empty(&format!("{API_ROOT}/queues/{id}")).await
}

pub async fn list_jobs(namespace_id: Uuid) -> ApiResult<Page<JobResource>> {
    get(&format!("{}?limit=100", jobs_path(namespace_id))).await
}

pub async fn all_jobs(namespace_id: Uuid) -> ApiResult<Vec<JobResource>> {
    get_all(&jobs_path(namespace_id)).await
}

/// Fetch one authorized Job so an edit URL works without prior list state.
pub async fn get_job(id: Uuid) -> ApiResult<JobResource> {
    get(&format!("{API_ROOT}/jobs/{id}")).await
}

pub async fn create_job(namespace_id: Uuid, request: &CreateJobRequest) -> ApiResult<JobResource> {
    post(&jobs_path(namespace_id), request).await
}

pub async fn update_job(id: Uuid, request: &UpdateJobRequest) -> ApiResult<JobResource> {
    put(&format!("{API_ROOT}/jobs/{id}"), request).await
}

pub async fn list_targets(namespace_id: Uuid) -> ApiResult<Page<TargetResource>> {
    get(&format!("{}?limit=100", targets_path(namespace_id))).await
}

pub async fn all_targets(namespace_id: Uuid) -> ApiResult<Vec<TargetResource>> {
    get_all(&targets_path(namespace_id)).await
}

pub async fn create_target(
    namespace_id: Uuid,
    request: &CreateTargetRequest,
) -> ApiResult<TargetResource> {
    post(&targets_path(namespace_id), request).await
}

pub async fn update_target(id: Uuid, request: &UpdateTargetRequest) -> ApiResult<TargetResource> {
    put(&format!("{API_ROOT}/targets/{id}"), request).await
}

pub async fn list_target_sets(namespace_id: Uuid) -> ApiResult<Page<TargetSetResource>> {
    get(&format!(
        "{API_ROOT}/namespaces/{namespace_id}/target-sets?limit=100"
    ))
    .await
}

pub async fn create_target_set(
    namespace_id: Uuid,
    request: &CreateTargetSetRequest,
) -> ApiResult<TargetSetResource> {
    post(
        &format!("{API_ROOT}/namespaces/{namespace_id}/target-sets"),
        request,
    )
    .await
}

pub async fn update_target_set(
    id: Uuid,
    request: &UpdateTargetSetRequest,
) -> ApiResult<TargetSetResource> {
    put(&format!("{API_ROOT}/target-sets/{id}"), request).await
}

pub async fn all_target_sets(namespace_id: Uuid) -> ApiResult<Vec<TargetSetResource>> {
    get_all(&format!("{API_ROOT}/namespaces/{namespace_id}/target-sets")).await
}

pub async fn list_schedules(namespace_id: Uuid) -> ApiResult<Page<ScheduleResource>> {
    get(&format!(
        "{API_ROOT}/namespaces/{namespace_id}/schedules?limit=100"
    ))
    .await
}

pub async fn create_schedule(
    namespace_id: Uuid,
    request: &CreateScheduleRequest,
) -> ApiResult<ScheduleResource> {
    post(
        &format!("{API_ROOT}/namespaces/{namespace_id}/schedules"),
        request,
    )
    .await
}

pub async fn update_schedule(
    id: Uuid,
    request: &UpdateScheduleRequest,
) -> ApiResult<ScheduleResource> {
    patch(&format!("{API_ROOT}/schedules/{id}"), request).await
}

pub async fn list_runs() -> ApiResult<Page<RunResource>> {
    get(&format!("{API_ROOT}/runs?limit=100")).await
}

/// Fetch the bounded output of every Attempt on one authorized Run.
pub async fn list_run_attempts(run_id: Uuid) -> ApiResult<Vec<RunAttemptResource>> {
    get(&format!("{API_ROOT}/runs/{run_id}/attempts")).await
}

pub async fn create_run(
    job_id: Uuid,
    target: ExecutionTarget,
    inputs: serde_json::Value,
) -> ApiResult<RunBatchResource> {
    post(
        &format!("{API_ROOT}/runs"),
        &CreateRunRequest {
            request_id: Uuid::now_v7(),
            job_id,
            target,
            inputs,
        },
    )
    .await
}

fn jobs_path(namespace_id: Uuid) -> String {
    format!("{API_ROOT}/namespaces/{namespace_id}/jobs")
}

fn targets_path(namespace_id: Uuid) -> String {
    format!("{API_ROOT}/namespaces/{namespace_id}/targets")
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

async fn put<T, B>(url: &str, body: &B) -> ApiResult<T>
where
    T: DeserializeOwned,
    B: Serialize,
{
    let request = Request::put(url)
        .json(body)
        .map_err(|error| client_error(format!("Could not encode the request: {error}")))?;
    let response = request
        .send()
        .await
        .map_err(|error| client_error(format!("Crono API is unreachable: {error}")))?;
    decode(response).await
}

async fn patch<T, B>(url: &str, body: &B) -> ApiResult<T>
where
    T: DeserializeOwned,
    B: Serialize,
{
    let request = Request::patch(url)
        .json(body)
        .map_err(|error| client_error(format!("Could not encode the request: {error}")))?;
    let response = request
        .send()
        .await
        .map_err(|error| client_error(format!("Crono API is unreachable: {error}")))?;
    decode(response).await
}

async fn delete_empty(url: &str) -> ApiResult<()> {
    let response = Request::delete(url)
        .send()
        .await
        .map_err(|error| client_error(format!("Crono API is unreachable: {error}")))?;
    if response.ok() {
        return Ok(());
    }
    Err(decode_error(response).await)
}

async fn decode<T: DeserializeOwned>(response: Response) -> ApiResult<T> {
    if response.ok() {
        return response
            .json()
            .await
            .map_err(|error| client_error(format!("Crono API returned invalid JSON: {error}")));
    }
    Err(decode_error(response).await)
}

async fn decode_error(response: Response) -> ApiError {
    let status = response.status();
    response.json::<ErrorEnvelope>().await.map_or_else(
        |_| client_error(format!("Crono API request failed with HTTP {status}")),
        |envelope| ApiError {
            code: envelope.error.code,
            message: envelope.error.message,
            field: envelope.error.field,
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
    use super::{jobs_path, targets_path};
    use uuid::Uuid;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn relationship_paths_retain_selected_namespace_uuid() {
        let namespace_id = Uuid::from_u128(1);

        assert_eq!(
            jobs_path(namespace_id),
            format!("/api/namespaces/{namespace_id}/jobs")
        );
        assert_eq!(
            targets_path(namespace_id),
            format!("/api/namespaces/{namespace_id}/targets")
        );
    }
}
