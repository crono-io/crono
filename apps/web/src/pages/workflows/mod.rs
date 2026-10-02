//! Workflow catalog, structured authoring, and orchestration visibility.
//!
//! Pages submit only existing Job references, dependency conditions, and normal
//! execution selections. The server owns graph validity, authorization, snapshots,
//! and execution. Graph rendering has no API dependency; ordinary Run routes own
//! Attempts/output. History and polling are bounded to the viewed invocation.

mod detail;
mod editor;
mod form;
mod graph;
#[cfg(test)]
mod graph_tests;
mod list;
mod node_list;
mod run;
mod run_detail;

pub use detail::WorkflowDetailsPage;
pub use form::{CreateWorkflowPage, EditWorkflowPage};
pub use list::WorkflowsPage;
pub use run::RunWorkflowPage;
pub use run_detail::WorkflowRunDetailsPage;

use crate::{api, components::QUIET_ACTION_CLASS};
use crono_api::{DependencyCondition, RunStatus, WorkflowNodeRunState, WorkflowRunState};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use uuid::Uuid;

const ACTION_CLASS: &str = super::runs::ACTION_CLASS;
const PANEL_CLASS: &str =
    "min-w-0 space-y-4 rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6";

/// Friendly labels are shared by form controls, arrows, and structured dependencies.
const fn condition_label(condition: DependencyCondition) -> &'static str {
    match condition {
        DependencyCondition::Success => "On success",
        DependencyCondition::Failure => "On failure",
        DependencyCondition::Always => "Always",
    }
}

/// Display every backend node disposition, without inferring success from overall state.
const fn node_state_label(state: WorkflowNodeRunState) -> &'static str {
    match state {
        WorkflowNodeRunState::Pending => "Pending",
        WorkflowNodeRunState::Ready => "Ready",
        WorkflowNodeRunState::Running => "Running",
        WorkflowNodeRunState::Succeeded => "Succeeded",
        WorkflowNodeRunState::Failed => "Failed",
        WorkflowNodeRunState::Skipped => "Skipped",
        WorkflowNodeRunState::Cancelled => "Cancelled",
        WorkflowNodeRunState::Unknown => "Unknown",
    }
}

/// Match ordinary Run badge colors while retaining Workflow-specific state text.
const fn node_state_class(state: WorkflowNodeRunState) -> &'static str {
    super::runs::status_class(match state {
        WorkflowNodeRunState::Pending | WorkflowNodeRunState::Ready => RunStatus::PendingDispatch,
        WorkflowNodeRunState::Running => RunStatus::Running,
        WorkflowNodeRunState::Succeeded => RunStatus::Succeeded,
        WorkflowNodeRunState::Failed => RunStatus::Failed,
        WorkflowNodeRunState::Skipped => RunStatus::Skipped,
        WorkflowNodeRunState::Cancelled => RunStatus::Cancelled,
        WorkflowNodeRunState::Unknown => RunStatus::Unknown,
    })
}

/// Overall states are server decisions; the client does not derive recovery semantics.
const fn workflow_state(state: WorkflowRunState) -> WorkflowNodeRunState {
    match state {
        WorkflowRunState::Pending => WorkflowNodeRunState::Pending,
        WorkflowRunState::Running => WorkflowNodeRunState::Running,
        WorkflowRunState::Succeeded => WorkflowNodeRunState::Succeeded,
        WorkflowRunState::Failed => WorkflowNodeRunState::Failed,
        WorkflowRunState::Cancelled => WorkflowNodeRunState::Cancelled,
    }
}

/// Render text alongside status colors in cards, tables, and graph nodes.
#[component]
fn StateBadge(state: WorkflowNodeRunState) -> impl IntoView {
    view! { <span class=format!("inline-flex rounded-full px-2.5 py-1 text-xs font-medium {}", node_state_class(state))>{node_state_label(state)}</span> }
}

/// Validate a deep link locally without issuing a request with a fabricated identity.
fn route_id(parameter: &'static str) -> Signal<Option<Uuid>> {
    let params = use_params_map();
    Signal::derive(move || {
        params
            .get()
            .get(parameter)
            .and_then(|value| Uuid::parse_str(&value).ok())
    })
}

/// Invalid URLs use the same visible error shape as API rejection.
fn invalid_url() -> api::ApiError {
    api::ApiError {
        code: "invalid_workflow_id".to_string(),
        message: "Invalid Workflow URL.".to_string(),
        field: None,
    }
}

/// Keep server errors visible and provide an explicit bounded retry.
#[component]
fn ApiFailure(message: String, on_retry: Callback<()>) -> impl IntoView {
    view! { <section class=PANEL_CLASS><p class="break-words text-sm text-crono-failed" role="alert">{message}</p><button type="button" class=QUIET_ACTION_CLASS on:click=move |_| on_retry.run(())>"Retry"</button></section> }
}

/// Use API timestamps and the browser clock; native `SystemTime` is unavailable on WASM.
/// Invalid or negative intervals stay unknown instead of fabricating elapsed time.
fn duration(started: Option<&str>, finished: Option<&str>) -> String {
    let Some(start) = started.and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
    else {
        return "Not started".to_string();
    };
    let current: String = js_sys::Date::new_0().to_iso_string().into();
    let Ok(end) = chrono::DateTime::parse_from_rfc3339(finished.unwrap_or(&current)) else {
        return super::runs::display_duration(None);
    };
    super::runs::display_duration(
        u64::try_from(end.signed_duration_since(start).num_milliseconds()).ok(),
    )
}

#[cfg(test)]
mod tests {
    use super::{condition_label, duration, node_state_label, workflow_state};
    use crono_api::{DependencyCondition, WorkflowNodeRunState, WorkflowRunState};
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn active_duration_reads_browser_clock_without_system_time_panics() {
        let active = duration(Some("2020-01-01T00:00:00Z"), None);
        assert_ne!(active, "");
        assert_ne!(active, "Not started");
        assert_eq!(
            duration(Some("2020-01-01T00:00:00Z"), Some("2020-01-01T00:00:05Z")),
            "5.0s"
        );
    }

    #[wasm_bindgen_test]
    fn dependency_labels_and_every_actual_status_remain_explicit() {
        assert_eq!(condition_label(DependencyCondition::Success), "On success");
        assert_eq!(condition_label(DependencyCondition::Failure), "On failure");
        assert_eq!(condition_label(DependencyCondition::Always), "Always");
        for (state, label) in [
            (WorkflowNodeRunState::Pending, "Pending"),
            (WorkflowNodeRunState::Ready, "Ready"),
            (WorkflowNodeRunState::Running, "Running"),
            (WorkflowNodeRunState::Succeeded, "Succeeded"),
            (WorkflowNodeRunState::Failed, "Failed"),
            (WorkflowNodeRunState::Skipped, "Skipped"),
            (WorkflowNodeRunState::Cancelled, "Cancelled"),
            (WorkflowNodeRunState::Unknown, "Unknown"),
        ] {
            assert_eq!(node_state_label(state), label);
        }
        assert_eq!(
            workflow_state(WorkflowRunState::Cancelled),
            WorkflowNodeRunState::Cancelled
        );
    }
}
