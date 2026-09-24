//! Namespace-scoped Target authoring.
//!
//! Targets contribute destination-specific argv suffixes and JSON inputs while
//! retaining UUID identity across edits. They never select an executor; that
//! remains the Job's responsibility.

use super::resource_options;
use crate::{
    api,
    components::{
        ArgumentListInput, FormActions, JsonObjectInput, PageHeader, ResourceNameInput,
        ResourceSelect, name_validation_message, parse_input_object, visible_name_validation,
    },
};
use crono_api::{CreateTargetRequest, UpdateTargetRequest};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;

/// Create and edit Targets without exposing relationship UUIDs.
#[component]
pub fn TargetsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let editing_id = RwSignal::new(None);
    let name = RwSignal::new(String::new());
    let arguments = RwSignal::new(Vec::<String>::new());
    let inputs = RwSignal::new("{}".to_string());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let server_field = RwSignal::new(None::<(String, String)>);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
    let targets = LocalResource::new(move || async move {
        match namespace_id.get() {
            Some(id) => api::list_targets(id).await,
            None => Ok(crono_api::Page {
                items: Vec::new(),
                next_cursor: None,
            }),
        }
    });
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
    let input_error = Signal::derive(move || {
        server_error("inputs")
            .get()
            .or_else(|| parse_input_object(&inputs.get()).err())
    });
    let argument_error = Signal::derive(move || {
        server_error("arguments").get().or_else(|| {
            crono_execution::validate_argument_templates(&arguments.get())
                .err()
                .map(|error| error.to_string())
        })
    });
    let reset = Callback::new(move |()| {
        editing_id.set(None);
        name.set(String::new());
        arguments.set(Vec::new());
        inputs.set("{}".to_string());
        attempted.set(false);
        server_field.set(None);
    });
    let disabled = Signal::derive(move || {
        submitting.get()
            || namespace_id.get().is_none()
            || name_validation_message(&name.get(), true).is_some()
            || input_error.get().is_some()
            || argument_error.get().is_some()
    });
    let submit = target_submit(
        &TargetSubmitState {
            namespace_id,
            editing_id,
            name,
            arguments,
            inputs,
            attempted,
            submitting,
            server_field,
            feedback,
            targets,
            argument_error,
        },
        reset,
    );
    target_view(&TargetViewState {
        namespace_id,
        editing_id,
        name,
        arguments,
        inputs,
        feedback,
        namespace_choices,
        targets,
        namespace_error,
        name_error,
        input_error,
        argument_error,
        disabled,
        reset,
        submit,
    })
}

#[derive(Clone, Copy)]
struct TargetViewState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    targets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetResource>>>,
    namespace_error: Signal<Option<String>>,
    name_error: Signal<Option<String>>,
    input_error: Signal<Option<String>>,
    argument_error: Signal<Option<String>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn target_view(state: &TargetViewState) -> impl IntoView + use<> {
    let state = *state;
    view! {
        <div class="space-y-8">
            <PageHeader title="Targets" description="Targets supply destination-specific arguments and variables to a Job." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">{move || if state.editing_id.get().is_some() { "Edit Target" } else { "Create Target" }}</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect id="target-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_error />
                    <Show when=move || !state.namespace_choices.loading.get() && state.namespace_choices.options.get().is_empty() && state.namespace_choices.load_error.get().is_none()><p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No namespaces exist yet. "<A href="/namespaces" attr:class="font-medium text-crono-primary">"Create one before creating a Target."</A></p></Show>
                    <ResourceNameInput id="target-name" label="Name" value=state.name error=state.name_error />
                    <ArgumentListInput id="target-arguments" label="Additional arguments" values=state.arguments error=state.argument_error />
                    <JsonObjectInput id="target-inputs" label="Target inputs" value=state.inputs error=state.input_error />
                    <FormActions submit_label="Save Target" disabled=state.disabled on_cancel=state.reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface"><header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Targets in Namespace"</h2></header>{move || state.targets.map(|result| match result {
                Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Target."</p> }.into_any(),
                Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().cloned().map(|target| { let edit_target = target.clone(); view! { <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6"><div><p class="font-medium text-crono-text">{target.qualified_name}</p><p class="text-sm text-crono-muted">{format!("{} additional argv items", target.arguments.len())}</p></div><button type="button" class="text-sm font-medium text-crono-primary" on:click=move |_| { state.editing_id.set(Some(edit_target.id)); state.namespace_id.set(Some(edit_target.namespace_id)); state.name.set(edit_target.name.clone()); state.arguments.set(edit_target.arguments.clone()); state.inputs.set(pretty_json(&edit_target.inputs)); }>"Edit"</button></li> } }).collect_view()}</ul> }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Targets…"</p> }.into_any())}</section>
        </div>
    }
}

#[derive(Clone, Copy)]
struct TargetSubmitState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    server_field: RwSignal<Option<(String, String)>>,
    feedback: RwSignal<Option<String>>,
    targets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetResource>>>,
    argument_error: Signal<Option<String>>,
}

fn target_submit(
    state: &TargetSubmitState,
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
        if name_validation_message(&state.name.get_untracked(), true).is_some()
            || state.argument_error.get_untracked().is_some()
        {
            return;
        }
        let create = CreateTargetRequest {
            name: state.name.get_untracked(),
            arguments: state.arguments.get_untracked(),
            inputs,
        };
        let current_edit = state.editing_id.get_untracked();
        state.submitting.set(true);
        spawn_local(async move {
            let result = match current_edit {
                Some(id) => {
                    api::update_target(
                        id,
                        &UpdateTargetRequest {
                            name: create.name,
                            arguments: create.arguments,
                            inputs: create.inputs,
                        },
                    )
                    .await
                }
                None => api::create_target(namespace, &create).await,
            };
            match result {
                Ok(target) => {
                    reset.run(());
                    state
                        .feedback
                        .set(Some(format!("Saved {}.", target.qualified_name)));
                    state.targets.refetch();
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
