//! Explicit Workflow launch through normal destination and invocation input controls.
//!
//! The backend snapshots the graph and prepares ordinary Runs. A request identity
//! is reused when retrying the same launch, including after an ambiguous network
//! failure; changing destination or inputs creates a new identity. Successful
//! launch opens the returned invocation instead of guessing its execution state.

use super::{ApiFailure, PANEL_CLASS, invalid_url, route_id};
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, QUIET_ACTION_CLASS, ResourceOption,
        ResourceSelect, parse_input_object,
    },
    navigation::{workflow_details_path, workflow_run_details_path},
    pages::resource_options,
};
use crono_api::{ExecutionTarget, StartWorkflowRequest, WorkflowResource};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::{NavigateOptions, components::A, hooks::use_navigate};
use uuid::Uuid;

/// Resolve the authorized Workflow before allowing a launch.
#[component]
pub fn RunWorkflowPage() -> impl IntoView {
    let id = route_id("workflow_id");
    let workflow = LocalResource::new(move || {
        let id = id.get();
        async move {
            match id {
                Some(id) => api::get_workflow(id).await,
                None => Err(invalid_url()),
            }
        }
    });
    view! { {move || workflow.map(|result| match result {
        Ok(workflow) => view! { <LaunchForm workflow=workflow.clone() /> }.into_any(),
        Err(error) => view! { <ApiFailure message=error.message.clone() on_retry=Callback::new(move |()| workflow.refetch()) /> }.into_any(),
    }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Workflow…"</p> }.into_any())} }
}

/// Reuse Job invocation selectors; this form never selects per-node destinations.
#[component]
fn LaunchForm(workflow: WorkflowResource) -> impl IntoView {
    let namespace = RwSignal::new(Some(workflow.namespace_id));
    let targets = resource_options::targets(namespace);
    let sets = resource_options::target_sets(namespace);
    let options = destination_options(targets, sets);
    let destination = RwSignal::new(None::<Uuid>);
    let inputs = RwSignal::new("{}".to_string());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let prior = RwSignal::new(None::<StartWorkflowRequest>);
    let input_error = Signal::derive(move || parse_input_object(&inputs.get()).err());
    let id = workflow.id;
    let navigate = use_navigate();
    let cancel = navigate.clone();
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if busy.get_untracked() {
            return;
        }
        error.set(None);
        let Some(target_id) = destination.get_untracked() else {
            error.set(Some("Select a Target or Target Set.".to_string()));
            return;
        };
        let Ok(values) = parse_input_object(&inputs.get_untracked()) else {
            return;
        };
        let target = if sets
            .options
            .get_untracked()
            .iter()
            .any(|option| option.id == target_id)
        {
            ExecutionTarget::TargetSet { id: target_id }
        } else {
            ExecutionTarget::Target { id: target_id }
        };
        let request = retry_request(prior.get_untracked().as_ref(), target, values);
        prior.set(Some(request.clone()));
        busy.set(true);
        let navigate = navigate.clone();
        spawn_local(async move {
            let result = api::create_workflow_run(id, &request).await;
            if busy.is_disposed() {
                return;
            }
            match result {
                Ok(run) => navigate(
                    &workflow_run_details_path(run.id),
                    NavigateOptions::default(),
                ),
                Err(failure) => {
                    error.set(Some(failure.message));
                    busy.set(false);
                }
            }
        });
    };
    view! { <div class="space-y-6">
        <PageHeader title="Run Workflow" description="Every Job inherits the same Target or Target Set and invocation inputs."><A href=workflow_details_path(id) attr:class=QUIET_ACTION_CLASS>"Workflow details"</A></PageHeader>
        <section class=PANEL_CLASS><form class="space-y-5" on:submit=submit novalidate aria-busy=move || busy.get().to_string()>
            <fieldset class="space-y-5" disabled=move || busy.get()>
                <ResourceSelect id="workflow-run-destination" label="Target or Target Set" placeholder="Choose a destination…" options selected=destination loading=Signal::derive(move || targets.loading.get() || sets.loading.get()) load_error=Signal::derive(move || targets.load_error.get().or_else(|| sets.load_error.get())) />
                <JsonObjectInput id="workflow-run-inputs" label="Invocation inputs" value=inputs error=input_error />
                <p class="text-xs text-crono-muted">"Inputs are configuration, not a secret store. Target Set fan-out uses normal Crono Runs for each target."</p>
            </fieldset>
            <div class="space-y-1 rounded-lg bg-zinc-50 p-4 text-sm"><p class="break-words font-semibold">{format!("Workflow: {}", workflow.name)}</p><p>{format!("Jobs: {}", workflow.nodes.len())}</p><p class="break-words">{move || options.get().iter().find(|option| Some(option.id) == destination.get()).map_or_else(|| "Destination: not selected".to_string(), |option| option.label.clone())}</p></div>
            <Show when=move || error.get().is_some()><p role="alert" class="break-words text-sm text-crono-failed">{move || error.get().unwrap_or_default()}</p></Show>
            <FormActions submit_label="Run Workflow" disabled=Signal::derive(move || busy.get() || input_error.get().is_some()) on_cancel=Callback::new(move |()| { if !busy.get_untracked() { cancel(&workflow_details_path(id), NavigateOptions::default()); } }) />
        </form></section>
    </div> }
}

/// Match the existing manual Job form's single selector and explicit kind labels.
fn destination_options(
    targets: resource_options::ResourceOptions,
    sets: resource_options::ResourceOptions,
) -> Signal<Vec<ResourceOption>> {
    Signal::derive(move || {
        targets
            .options
            .get()
            .into_iter()
            .map(|option| ResourceOption {
                id: option.id,
                label: format!("Target · {}", option.label),
            })
            .chain(sets.options.get().into_iter().map(|option| ResourceOption {
                id: option.id,
                label: format!("Target Set · {}", option.label),
            }))
            .collect()
    })
}

/// Preserve idempotency only for an unchanged invocation payload.
fn retry_request(
    prior: Option<&StartWorkflowRequest>,
    target: ExecutionTarget,
    inputs: serde_json::Value,
) -> StartWorkflowRequest {
    let request_id = prior
        .filter(|prior| prior.target == target && prior.inputs == inputs)
        .map_or_else(Uuid::now_v7, |prior| prior.request_id);
    StartWorkflowRequest {
        request_id,
        target,
        inputs,
    }
}

#[cfg(test)]
mod tests {
    use super::retry_request;
    use crono_api::ExecutionTarget;
    use uuid::Uuid;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn ambiguous_launch_retry_preserves_identity_only_for_same_payload() {
        let target = ExecutionTarget::Target { id: Uuid::now_v7() };
        let first = retry_request(None, target, serde_json::json!({"version":1}));
        assert_eq!(
            retry_request(Some(&first), target, first.inputs.clone()),
            first
        );
        assert_ne!(
            retry_request(Some(&first), target, serde_json::json!({"version":2})).request_id,
            first.request_id
        );
        assert_ne!(
            retry_request(
                Some(&first),
                ExecutionTarget::TargetSet { id: Uuid::now_v7() },
                first.inputs.clone()
            )
            .request_id,
            first.request_id
        );
    }
}
