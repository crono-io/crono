//! Browser-only client for the public Crono HTTP contract.
//!
//! All pages use this module instead of embedding transport details. Requests
//! are same-origin under `/api`; Trunk proxies that prefix during local
//! development, while production can route it to the independently deployed
//! server. Structured error fields are retained so forms can place safe
//! server-side validation messages beside the relevant control.
//! The only credential source is `crono.access_token` in browser session storage.
//! It is attached centrally as a Bearer header, never embedded in a URL, page,
//! bundle, or error. No login, token issuance, or refresh flow is implemented.

use crono_api::{
    CreateJobRequest, CreateNamespaceRequest, CreateQueueRequest, CreateRunRequest,
    CreateScheduleRequest, CreateTargetRequest, CreateTargetSetRequest, CreateWorkflowRequest,
    ErrorEnvelope, ExecutionTarget, JobResource, MonitorResource, NamespaceResource,
    OverviewResource, Page, QueueResource, RerunRequest, RunAttemptResource, RunBatchResource,
    RunEventResource, RunResource, RunStatus, ScheduleResource, StartWorkflowRequest,
    TargetResource, TargetSetResource, UpdateJobRequest, UpdateQueueRequest, UpdateScheduleRequest,
    UpdateTargetRequest, UpdateTargetSetRequest, UpdateWorkflowRequest, WorkerResource,
    WorkflowResource, WorkflowRunResource,
};
use gloo_net::http::{Headers, Request, RequestBuilder, Response};
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Write;
use uuid::Uuid;

const API_ROOT: &str = "/api";
const MAX_COLLECTION_PAGES: usize = 100;
const ACCESS_TOKEN_KEY: &str = "crono.access_token";

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

/// Delete an empty Namespace; the server protects bootstrap and referenced resources.
pub async fn delete_namespace(id: Uuid) -> ApiResult<()> {
    delete_empty(&format!("{API_ROOT}/namespaces/{id}")).await
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

/// Browse one bounded page of visible Workflows using the server's name cursor.
pub async fn list_workflows(
    namespace_id: Uuid,
    after: Option<&str>,
) -> ApiResult<Page<WorkflowResource>> {
    let mut url = format!("{API_ROOT}/namespaces/{namespace_id}/workflows?limit=25");
    if let Some(cursor) = after {
        let _ = write!(url, "&after={cursor}");
    }
    get(&url).await
}

/// Load an authorized graph by identity, independently of browser list state.
pub async fn get_workflow(id: Uuid) -> ApiResult<WorkflowResource> {
    get(&format!("{API_ROOT}/workflows/{id}")).await
}

/// Submit references and dependencies for authoritative server DAG validation.
pub async fn create_workflow(
    namespace_id: Uuid,
    request: &CreateWorkflowRequest,
) -> ApiResult<WorkflowResource> {
    post(
        &format!("{API_ROOT}/namespaces/{namespace_id}/workflows"),
        request,
    )
    .await
}

/// Replace the requested revision; conflicts leave the editor's input intact.
pub async fn update_workflow(
    id: Uuid,
    request: &UpdateWorkflowRequest,
) -> ApiResult<WorkflowResource> {
    put(&format!("{API_ROOT}/workflows/{id}"), request).await
}

/// Delete an unused definition; the server protects existing invocation history.
pub async fn delete_workflow(id: Uuid) -> ApiResult<()> {
    delete_empty(&format!("{API_ROOT}/workflows/{id}")).await
}

/// Launch with a retained request UUID so a transport retry cannot duplicate work.
pub async fn create_workflow_run(
    id: Uuid,
    request: &StartWorkflowRequest,
) -> ApiResult<WorkflowRunResource> {
    post(&format!("{API_ROOT}/workflows/{id}/runs"), request).await
}

/// Browse server-ordered history with the returned newest-first UUID cursor.
pub async fn list_workflow_runs(
    id: Uuid,
    before: Option<Uuid>,
) -> ApiResult<Page<WorkflowRunResource>> {
    let mut url = format!("{API_ROOT}/workflows/{id}/runs?limit=25");
    if let Some(cursor) = before {
        let _ = write!(url, "&before={cursor}");
    }
    get(&url).await
}

/// Read the launch snapshot and durable node progress, without Attempt output.
pub async fn get_workflow_run(id: Uuid) -> ApiResult<WorkflowRunResource> {
    get(&format!("{API_ROOT}/workflow-runs/{id}")).await
}

/// Stop future nodes; existing child Runs drain according to server semantics.
pub async fn cancel_workflow_run(id: Uuid) -> ApiResult<WorkflowRunResource> {
    post(
        &format!("{API_ROOT}/workflow-runs/{id}/cancel"),
        &serde_json::json!({}),
    )
    .await
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

/// Fetch one alphabetical page without silently truncating large Target lists.
pub async fn list_targets(
    namespace_id: Uuid,
    after: Option<&str>,
) -> ApiResult<Page<TargetResource>> {
    let base = format!("{}?limit=25", targets_path(namespace_id));
    let url = after.map_or(base.clone(), |cursor| format!("{base}&after={cursor}"));
    get(&url).await
}

pub async fn all_targets(namespace_id: Uuid) -> ApiResult<Vec<TargetResource>> {
    get_all(&targets_path(namespace_id)).await
}

/// Fetch one authorized Target so an edit URL survives refresh or direct entry.
pub async fn get_target(id: Uuid) -> ApiResult<TargetResource> {
    get(&format!("{API_ROOT}/targets/{id}")).await
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

/// Delete an unused Target, preserving the server's protection and in-use errors.
pub async fn delete_target(id: Uuid) -> ApiResult<()> {
    delete_empty(&format!("{API_ROOT}/targets/{id}")).await
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

/// Query one authorized page; filters are applied by the server before pagination.
pub async fn filtered_runs(
    namespace_id: Option<Uuid>,
    status: Option<RunStatus>,
    job_id: Option<Uuid>,
    target_id: Option<Uuid>,
    target_set_id: Option<Uuid>,
    before: Option<Uuid>,
) -> ApiResult<Page<RunResource>> {
    let mut url = format!("{API_ROOT}/runs?limit=50");
    if let Some(id) = namespace_id {
        let _ = write!(url, "&namespace_id={id}");
    }
    if let Some(status) = status {
        let _ = write!(url, "&status={}", run_status_parameter(status));
    }
    if let Some(id) = job_id {
        let _ = write!(url, "&job_id={id}");
    }
    if let Some(id) = target_id {
        let _ = write!(url, "&target_id={id}");
    }
    if let Some(id) = target_set_id {
        let _ = write!(url, "&target_set_id={id}");
    }
    if let Some(id) = before {
        let _ = write!(url, "&before={id}");
    }
    get(&url).await
}

/// Read one authorized Run without embedding attempt output in the list.
pub async fn get_run(run_id: Uuid) -> ApiResult<RunResource> {
    get(&format!("{API_ROOT}/runs/{run_id}")).await
}

/// Repeat the server's immutable invocation snapshot; no secret-bearing snapshot reaches the browser.
pub async fn rerun_run(run_id: Uuid, request_id: Uuid) -> ApiResult<RunResource> {
    post(
        &format!("{API_ROOT}/runs/{run_id}/rerun"),
        &RerunRequest { request_id },
    )
    .await
}

/// Read server-observed lifecycle timestamps after `RunRead` authorization.
pub async fn list_run_events(run_id: Uuid) -> ApiResult<Vec<RunEventResource>> {
    get(&format!("{API_ROOT}/runs/{run_id}/events")).await
}

const fn run_status_parameter(status: RunStatus) -> &'static str {
    match status {
        RunStatus::PendingDispatch => "pending_dispatch",
        RunStatus::Queued => "queued",
        RunStatus::Running => "running",
        RunStatus::RetryWait => "retry_wait",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Dead => "dead",
        RunStatus::Skipped => "skipped",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Unknown => "unknown",
    }
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

/// Fetch authorized, allowlisted diagnostics for a single worker.
pub async fn get_worker(worker_id: &str) -> ApiResult<crono_api::WorkerDetailsResource> {
    get(&format!("{API_ROOT}/workers/{worker_id}")).await
}

async fn get<T: DeserializeOwned>(url: &str) -> ApiResult<T> {
    let response = authenticated(Request::get(url))?
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
    let request = authenticated(Request::post(url))?
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
    let request = authenticated(Request::put(url))?
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
    let request = authenticated(Request::patch(url))?
        .json(body)
        .map_err(|error| client_error(format!("Could not encode the request: {error}")))?;
    let response = request
        .send()
        .await
        .map_err(|error| client_error(format!("Crono API is unreachable: {error}")))?;
    decode(response).await
}

async fn delete_empty(url: &str) -> ApiResult<()> {
    let response = authenticated(Request::delete(url))?
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

/// Attach the session's opaque credential to every API verb before encoding a body.
///
/// Missing credentials are sent without a header so the server returns its 401
/// envelope. Inaccessible storage or invalid header bytes fail with a fixed,
/// credential-free error. The browser does not verify identity or interpret claims.
fn authenticated(request: RequestBuilder) -> ApiResult<RequestBuilder> {
    let storage = web_sys::window()
        .ok_or_else(|| client_error("Browser session storage is unavailable".to_string()))?
        .session_storage()
        .map_err(|_| client_error("Browser session storage is unavailable".to_string()))?;
    let token = storage
        .map(|storage| storage.get_item(ACCESS_TOKEN_KEY))
        .transpose()
        .map_err(|_| client_error("Could not read the API credential".to_string()))?
        .flatten();
    with_bearer(request, token.as_deref())
}

/// Build a header without propagating browser errors that could contain a secret.
fn with_bearer(request: RequestBuilder, token: Option<&str>) -> ApiResult<RequestBuilder> {
    let headers = web_sys::Headers::new()
        .map_err(|_| client_error("Could not prepare API headers".to_string()))?;
    if let Some(token) = token {
        headers
            .set("Authorization", &format!("Bearer {token}"))
            .map_err(|_| {
                client_error("The API credential cannot be used as a header".to_string())
            })?;
    }
    Ok(request.headers(Headers::from_raw(headers)))
}

#[cfg(test)]
mod tests {
    use super::{ACCESS_TOKEN_KEY, authenticated, jobs_path, targets_path, with_bearer};
    use gloo_net::http::Request;
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

    #[wasm_bindgen_test]
    fn all_http_verbs_read_session_credentials_without_embedding_them_in_urls() -> Result<(), String>
    {
        let window = web_sys::window().ok_or("missing window")?;
        let storage = window
            .session_storage()
            .map_err(|_| "storage unavailable")?
            .ok_or("storage unavailable")?;
        let previous = storage
            .get_item(ACCESS_TOKEN_KEY)
            .map_err(|_| "could not read storage")?;
        let token = "test-only-browser-credential-123456789";
        storage
            .set_item(ACCESS_TOKEN_KEY, token)
            .map_err(|_| "could not set storage")?;
        for builder in [
            Request::get("/api/probe"),
            Request::post("/api/probe"),
            Request::put("/api/probe"),
            Request::patch("/api/probe"),
            Request::delete("/api/probe"),
        ] {
            let request = authenticated(builder)
                .map_err(|error| error.message)?
                .build()
                .map_err(|_| "request invalid")?;
            assert_eq!(
                request.headers().get("authorization"),
                Some(format!("Bearer {token}"))
            );
            assert!(!request.url().contains(token));
        }
        storage
            .remove_item(ACCESS_TOKEN_KEY)
            .map_err(|_| "could not clear storage")?;
        let request = authenticated(Request::get("/api/probe"))
            .map_err(|error| error.message)?
            .build()
            .map_err(|_| "request invalid")?;
        assert_eq!(request.headers().get("authorization"), None);
        if let Some(previous) = previous {
            storage
                .set_item(ACCESS_TOKEN_KEY, &previous)
                .map_err(|_| "could not restore storage")?;
        }
        Ok(())
    }

    #[wasm_bindgen_test]
    fn invalid_header_errors_do_not_expose_the_credential() {
        let secret = "secret-with\ninvalid-header";
        let result = with_bearer(Request::get("/api/probe"), Some(secret));
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(!error.message.contains(secret));
        }
    }
}
