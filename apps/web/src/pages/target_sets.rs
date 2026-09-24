//! Target Set membership and shared-variable authoring.
//!
//! A Target Set is an explicit, UUID-backed fan-out selection plus one JSON
//! input layer shared by all members. Editing replaces metadata and membership
//! without changing the Target Set's identity.

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, ResourceMultiSelect, ResourceNameInput,
        ResourceSelect, name_validation_message, parse_input_object, visible_name_validation,
    },
};
use crono_api::{CreateTargetSetRequest, UpdateTargetSetRequest};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;

/// Create and edit explicit Target fan-out groups.
#[component]
pub fn TargetSetsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let editing_id = RwSignal::new(None);
    let name = RwSignal::new(String::new());
    let target_ids = RwSignal::new(Vec::new());
    let inputs = RwSignal::new("{}".to_string());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let server_field = RwSignal::new(None::<(String, String)>);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
    let target_choices = resource_options::targets(namespace_id);
    let target_sets = LocalResource::new(move || async move {
        match namespace_id.get() {
            Some(id) => api::list_target_sets(id).await,
            None => Ok(crono_api::Page {
                items: Vec::new(),
                next_cursor: None,
            }),
        }
    });
    clear_target_set_selection(namespace_id, target_ids, editing_id);
    let server_error = move |field: &'static str| {
        Signal::derive(move || {
            server_field
                .get()
                .filter(|(name, _)| name == field)
                .map(|(_, message)| message)
        })
    };
    let namespace_error = Signal::derive(move || {
        server_error("namespace_id").get().or_else(|| {
            (attempted.get() && namespace_id.get().is_none())
                .then(|| "Select a Namespace.".to_string())
        })
    });
    let name_error = Signal::derive(move || {
        server_error("name")
            .get()
            .or_else(|| visible_name_validation(&name.get(), attempted.get()))
    });
    let member_error = Signal::derive(move || {
        server_error("target_ids").get().or_else(|| {
            (attempted.get() && target_ids.get().is_empty())
                .then(|| "Select at least one Target.".to_string())
        })
    });
    let input_error = Signal::derive(move || {
        server_error("inputs")
            .get()
            .or_else(|| parse_input_object(&inputs.get()).err())
    });
    let reset = Callback::new(move |()| {
        editing_id.set(None);
        name.set(String::new());
        target_ids.set(Vec::new());
        inputs.set("{}".to_string());
        attempted.set(false);
        server_field.set(None);
    });
    let disabled = Signal::derive(move || {
        submitting.get()
            || namespace_id.get().is_none()
            || target_ids.get().is_empty()
            || name_validation_message(&name.get(), true).is_some()
            || input_error.get().is_some()
    });
    let submit = target_set_submit(
        &TargetSetSubmitState {
            namespace_id,
            editing_id,
            name,
            target_ids,
            inputs,
            attempted,
            submitting,
            server_field,
            feedback,
            target_sets,
        },
        reset,
    );
    target_sets_view(&TargetSetViewState {
        namespace_id,
        editing_id,
        name,
        target_ids,
        inputs,
        feedback,
        namespace_choices,
        target_choices,
        target_sets,
        namespace_error,
        name_error,
        member_error,
        input_error,
        disabled,
        reset,
        submit,
    })
}

fn clear_target_set_selection(
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    target_ids: RwSignal<Vec<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
) {
    let previous = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous.get_untracked() != current {
            target_ids.set(Vec::new());
            editing_id.set(None);
            previous.set(current);
        }
    });
}

#[derive(Clone, Copy)]
struct TargetSetViewState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    target_ids: RwSignal<Vec<uuid::Uuid>>,
    inputs: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    target_choices: resource_options::ResourceOptions,
    target_sets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetSetResource>>>,
    namespace_error: Signal<Option<String>>,
    name_error: Signal<Option<String>>,
    member_error: Signal<Option<String>>,
    input_error: Signal<Option<String>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn target_sets_view(state: &TargetSetViewState) -> impl IntoView + use<> {
    let state = *state;
    view! {
        <div class="space-y-8">
            <PageHeader title="Target Sets" description="Target Sets fan a Job out to explicit Targets and add shared variables." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">{move || if state.editing_id.get().is_some() { "Edit Target Set" } else { "Create Target Set" }}</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect id="target-set-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_error />
                    <Show when=move || !state.namespace_choices.loading.get() && state.namespace_choices.options.get().is_empty() && state.namespace_choices.load_error.get().is_none()><p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No namespaces exist yet. "<A href="/namespaces" attr:class="font-medium text-crono-primary">"Create one first."</A></p></Show>
                    <ResourceNameInput id="target-set-name" label="Name" value=state.name error=state.name_error />
                    <ResourceMultiSelect id="target-set-targets" label="Targets" options=state.target_choices.options selected=state.target_ids loading=state.target_choices.loading load_error=state.target_choices.load_error field_error=state.member_error />
                    <JsonObjectInput id="target-set-inputs" label="Shared inputs" value=state.inputs error=state.input_error />
                    <FormActions submit_label="Save Target Set" disabled=state.disabled on_cancel=state.reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface"><header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Target Sets in Namespace"</h2></header>{move || state.target_sets.map(|result| match result {
                Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Target Set."</p> }.into_any(),
                Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().cloned().map(|set| { let edit_set = set.clone(); view! { <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6"><div><p class="font-medium text-crono-text">{set.qualified_name}</p><p class="mt-1 text-sm text-crono-muted">{set.targets.iter().map(|target| target.name.clone()).collect::<Vec<_>>().join(", ")}</p></div><button type="button" class="text-sm font-medium text-crono-primary" on:click=move |_| { state.editing_id.set(Some(edit_set.id)); state.namespace_id.set(Some(edit_set.namespace_id)); state.name.set(edit_set.name.clone()); state.target_ids.set(edit_set.targets.iter().map(|target| target.id).collect()); state.inputs.set(pretty_json(&edit_set.inputs)); }>"Edit"</button></li> } }).collect_view()}</ul> }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Target Sets…"</p> }.into_any())}</section>
        </div>
    }
}

#[derive(Clone, Copy)]
struct TargetSetSubmitState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    target_ids: RwSignal<Vec<uuid::Uuid>>,
    inputs: RwSignal<String>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    server_field: RwSignal<Option<(String, String)>>,
    feedback: RwSignal<Option<String>>,
    target_sets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetSetResource>>>,
}

fn target_set_submit(
    state: &TargetSetSubmitState,
    reset: Callback<()>,
) -> Callback<leptos::ev::SubmitEvent> {
    let state = *state;
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        state.attempted.set(true);
        state.server_field.set(None);
        state.feedback.set(None);
        let Some(namespace) = state.namespace_id.get_untracked() else {
            return;
        };
        let Ok(inputs) = parse_input_object(&state.inputs.get_untracked()) else {
            return;
        };
        let request = CreateTargetSetRequest {
            name: state.name.get_untracked(),
            target_ids: state.target_ids.get_untracked(),
            inputs,
        };
        if name_validation_message(&request.name, true).is_some() || request.target_ids.is_empty() {
            return;
        }
        let current_edit = state.editing_id.get_untracked();
        state.submitting.set(true);
        spawn_local(async move {
            let result = match current_edit {
                Some(id) => {
                    api::update_target_set(
                        id,
                        &UpdateTargetSetRequest {
                            name: request.name,
                            target_ids: request.target_ids,
                            inputs: request.inputs,
                        },
                    )
                    .await
                }
                None => api::create_target_set(namespace, &request).await,
            };
            match result {
                Ok(set) => {
                    reset.run(());
                    state
                        .feedback
                        .set(Some(format!("Saved {}.", set.qualified_name)));
                    state.target_sets.refetch();
                }
                Err(error) => match error.field {
                    Some(field) => state.server_field.set(Some((field, error.message))),
                    None => state.feedback.set(Some(error.message)),
                },
            }
            state.submitting.set(false);
        });
    })
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}
