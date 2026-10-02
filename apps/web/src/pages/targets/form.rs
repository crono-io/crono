//! Target creation and editing share one validated form.
//!
//! The Job chooses the executor. This form stores the Target's argv suffix and
//! JSON inputs. An edit starts with an authorized Target loaded by its route,
//! keeps its Namespace and ID fixed, and preserves values after API failures.
//! Completed saves show a modal result. API failures retain field-level guidance
//! and entered values; successful creation clears the form for another Target.

use super::super::resource_options;
use crate::{
    api,
    components::{
        ArgumentListInput, FormActions, JsonObjectInput, PageHeader, ResourceFeedback,
        ResourceFeedbackModal, ResourceNameInput, ResourceSelect, name_validation_message,
        parse_input_object, visible_name_validation,
    },
};
use crono_api::{CreateTargetRequest, TargetResource, UpdateTargetRequest};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::{NavigateOptions, components::A, hooks::use_navigate};
use uuid::Uuid;

/// Render create or edit after the route has supplied the authorized Target.
#[component]
pub(super) fn TargetForm(
    #[prop(optional)] initial_target: Option<TargetResource>,
) -> impl IntoView {
    let edit_id = initial_target.as_ref().map(|target| target.id);
    let editing_default = initial_target
        .as_ref()
        .is_some_and(|target| target.namespace == "default" && target.name == "default");
    let namespace_id = RwSignal::new(initial_target.as_ref().map(|target| target.namespace_id));
    let name = RwSignal::new(
        initial_target
            .as_ref()
            .map_or_else(String::new, |target| target.name.clone()),
    );
    let arguments = RwSignal::new(
        initial_target
            .as_ref()
            .map_or_else(Vec::new, |target| target.arguments.clone()),
    );
    let inputs = RwSignal::new(
        initial_target
            .as_ref()
            .map_or_else(|| "{}".to_string(), |target| pretty_json(&target.inputs)),
    );
    let namespace_name = RwSignal::new(initial_target.map(|target| target.namespace));
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let server_field = RwSignal::new(None::<(String, String)>);
    let feedback = RwSignal::new(None::<ResourceFeedback>);
    let namespace_choices = resource_options::namespaces();
    let TargetErrors {
        namespace: namespace_error,
        name: name_error,
        inputs: input_error,
        arguments: argument_error,
    } = target_errors(
        namespace_id,
        name,
        arguments,
        inputs,
        attempted,
        server_field,
    );
    let clear_after_create = Callback::new(move |()| {
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
        TargetSubmitState {
            edit_id,
            namespace_id,
            name,
            arguments,
            inputs,
            attempted,
            submitting,
            server_field,
            feedback,
            argument_error,
        },
        clear_after_create,
    );
    let navigate = use_navigate();
    let cancel = Callback::new(move |()| navigate("/targets", NavigateOptions::default()));
    target_view(&TargetViewState {
        is_edit: edit_id.is_some(),
        namespace_name,
        namespace_id,
        editing_default,
        name,
        arguments,
        inputs,
        feedback,
        namespace_choices,
        namespace_error,
        name_error,
        input_error,
        argument_error,
        disabled,
        cancel,
        submit,
    })
}

#[derive(Clone, Copy)]
struct TargetErrors {
    namespace: Signal<Option<String>>,
    name: Signal<Option<String>>,
    inputs: Signal<Option<String>>,
    arguments: Signal<Option<String>>,
}

/// Combine local validation and field-specific API errors without discarding input.
fn target_errors(
    namespace_id: RwSignal<Option<Uuid>>,
    name: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    attempted: RwSignal<bool>,
    server_field: RwSignal<Option<(String, String)>>,
) -> TargetErrors {
    let server_error = move |field: &'static str| {
        Signal::derive(move || {
            server_field
                .get()
                .filter(|(name, _)| name == field)
                .map(|(_, message)| message)
        })
    };
    TargetErrors {
        namespace: Signal::derive(move || {
            server_error("namespace_id").get().or_else(|| {
                (attempted.get() && namespace_id.get().is_none())
                    .then(|| "Select a Namespace.".to_string())
            })
        }),
        name: Signal::derive(move || {
            server_error("name")
                .get()
                .or_else(|| visible_name_validation(&name.get(), attempted.get()))
        }),
        inputs: Signal::derive(move || {
            server_error("inputs")
                .get()
                .or_else(|| parse_input_object(&inputs.get()).err())
        }),
        arguments: Signal::derive(move || {
            server_error("arguments").get().or_else(|| {
                crono_execution::validate_argument_templates(&arguments.get())
                    .err()
                    .map(|error| error.to_string())
            })
        }),
    }
}

#[derive(Clone, Copy)]
struct TargetViewState {
    is_edit: bool,
    namespace_name: RwSignal<Option<String>>,
    namespace_id: RwSignal<Option<Uuid>>,
    editing_default: bool,
    name: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    feedback: RwSignal<Option<ResourceFeedback>>,
    namespace_choices: resource_options::ResourceOptions,
    namespace_error: Signal<Option<String>>,
    name_error: Signal<Option<String>>,
    input_error: Signal<Option<String>>,
    argument_error: Signal<Option<String>>,
    disabled: Signal<bool>,
    cancel: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

/// Keep create and edit presentation aligned while fixing edit-only fields.
fn target_view(state: &TargetViewState) -> impl IntoView + use<> {
    let state = *state;
    let navigate = use_navigate();
    let view_targets = Callback::new(move |()| navigate("/targets", NavigateOptions::default()));
    view! {
        <div class="space-y-8">
            <PageHeader title=if state.is_edit { "Edit Target" } else { "Create Target" } description="Targets supply destination-specific arguments and variables to a Job." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">{if state.is_edit { "Target settings" } else { "New Target" }}</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| state.submit.run(event) novalidate>
                    {match state.namespace_name.get_untracked() {
                        Some(namespace) => view! {
                            <div><p class="text-sm font-medium text-crono-text">"Namespace"</p><p class="mt-1.5 rounded-md border border-crono-border bg-zinc-50 px-3 py-2.5 text-sm text-crono-muted">{namespace}</p></div>
                        }.into_any(),
                        None => view! {
                            <div>
                                <ResourceSelect id="target-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_error select_single=true />
                                <Show when=move || !state.namespace_choices.loading.get() && state.namespace_choices.options.get().is_empty() && state.namespace_choices.load_error.get().is_none()><p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No namespaces exist yet. "<A href="/namespaces" attr:class="font-medium text-crono-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary">"Create one before creating a Target."</A></p></Show>
                            </div>
                        }.into_any(),
                    }}
                    <ResourceNameInput id="target-name" label="Name" value=state.name error=state.name_error read_only=Signal::derive(move || state.editing_default) />
                    {state.editing_default.then(|| view! { <p class="text-xs text-crono-muted">"The default Target name is fixed; its arguments and inputs can be edited."</p> })}
                    <div class="space-y-1">
                        <ArgumentListInput id="target-arguments" label="Additional arguments" values=state.arguments error=state.argument_error />
                        <p class="text-xs text-crono-muted">
                            "For an ansible-playbook Job, add "<code class="whitespace-nowrap font-mono">"--limit"</code>" in one row and "<code class="whitespace-nowrap font-mono">"{{ host }}"</code>" in the next. Target arguments follow Job arguments."
                        </p>
                    </div>
                    <div class="space-y-1">
                        <JsonObjectInput id="target-inputs" label="Target inputs" value=state.inputs error=state.input_error />
                        <p class="text-xs text-crono-muted">
                            "Example JSON: "<code class="whitespace-nowrap font-mono">"{\"host\":\"web01.example.com\"}"</code>". The argument above limits the playbook to this inventory host."
                        </p>
                    </div>
                    <div class="rounded-md border border-crono-border bg-zinc-50 px-3 py-2.5">
                        <p class="text-xs font-medium text-crono-muted">"Example worker command (Job + Target)"</p>
                        <code class="mt-1 block break-words font-mono text-xs text-crono-text">
                            "/usr/bin/ansible-playbook -i /etc/ansible/hosts /srv/playbooks/deploy.yml --limit web01.example.com"
                        </code>
                    </div>
                    <div class="rounded-md border border-crono-border bg-zinc-50 px-3 py-2.5">
                        <p class="text-xs font-medium text-crono-muted">"Simple echo demo (Job + Target)"</p>
                        <p class="mt-1 text-xs text-crono-muted">
                            "Job: "<code class="font-mono">"/bin/echo"</code>" with argument "<code class="font-mono">"Hello"</code>". Target: argument "<code class="whitespace-nowrap font-mono">"{{ name }}"</code>" and inputs "<code class="whitespace-nowrap font-mono">"{\"name\":\"world\"}"</code>"."
                        </p>
                        <code class="mt-1 block break-words font-mono text-xs text-crono-text">"/bin/echo Hello world"</code>
                    </div>
                    <FormActions submit_label="Save Target" disabled=state.disabled on_cancel=state.cancel />
                </form>
                <ResourceFeedbackModal id="target-save-result" resource="Target" plural="Targets" feedback=state.feedback on_view=view_targets />
            </section>
        </div>
    }
}

#[derive(Clone, Copy)]
struct TargetSubmitState {
    edit_id: Option<Uuid>,
    namespace_id: RwSignal<Option<Uuid>>,
    name: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    server_field: RwSignal<Option<(String, String)>>,
    feedback: RwSignal<Option<ResourceFeedback>>,
    argument_error: Signal<Option<String>>,
}

/// Validate before sending and keep an edit bound to its original Target ID.
fn target_submit(
    state: TargetSubmitState,
    clear_after_create: Callback<()>,
) -> Callback<leptos::ev::SubmitEvent> {
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if state.submitting.get_untracked() {
            return;
        }
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
        state.submitting.set(true);
        spawn_local(async move {
            let result = match state.edit_id {
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
                    if state.edit_id.is_some() {
                        state.name.set(target.name.clone());
                        state.arguments.set(target.arguments.clone());
                        state.inputs.set(pretty_json(&target.inputs));
                    } else {
                        clear_after_create.run(());
                    }
                    state.feedback.set(Some(ResourceFeedback::saved(
                        format!("Saved Target {}.", target.qualified_name),
                        if state.edit_id.is_some() {
                            "Continue editing"
                        } else {
                            "Create another Target"
                        },
                    )));
                }
                Err(error) => {
                    if let Some(field) = error.field {
                        state.server_field.set(Some((field, error.message.clone())));
                    }
                    state
                        .feedback
                        .set(Some(ResourceFeedback::failed(error.message)));
                }
            }
            state.submitting.set(false);
        });
    })
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}
