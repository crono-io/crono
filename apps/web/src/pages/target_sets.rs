//! Target Set membership and shared-variable authoring.
//!
//! A Target Set is an explicit, UUID-backed fan-out selection plus one JSON
//! input layer shared by all members. Editing replaces metadata and membership
//! without changing the Target Set's identity. Shared modals report every API
//! outcome while failures retain field guidance and entered membership choices.
//! Read-only examples explain per-member Runs and input precedence without
//! adding sample resources or altering the user's draft.

mod guide;

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, QUIET_ACTION_CLASS, ResourceFeedback,
        ResourceFeedbackModal, ResourceMultiSelect, ResourceNameInput, ResourceSelect,
        focus_heading, name_validation_message, parse_input_object, visible_name_validation,
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
    let feedback = RwSignal::new(None::<ResourceFeedback>);
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

/// Clear membership and edit identity when switching Namespace collections.
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
    feedback: RwSignal<Option<ResourceFeedback>>,
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

/// Pair the live editor with illustrative examples and the selected Namespace's list.
fn target_sets_view(state: &TargetSetViewState) -> impl IntoView + use<> {
    let state = *state;
    let heading = NodeRef::<leptos::html::H2>::new();
    view! {
        <div class="space-y-8">
            <PageHeader title="Target Sets" description="Group Targets to run the same Job once for each member, with inputs shared across the group." />
            <guide::TargetSetSteps />
            <div class="grid items-start gap-6 xl:grid-cols-2">
                {target_set_form(&state)}
                <guide::TargetSetGuide />
            </div>
            {target_sets_list(&state, heading)}
            <ResourceFeedbackModal id="target-set-save-result" resource="Target Set" plural="Target Sets" feedback=state.feedback on_view=Callback::new(move |()| focus_heading(heading)) />
        </div>
    }
}

/// Explain each draft field while preserving the existing save and reset behavior.
fn target_set_form(state: &TargetSetViewState) -> impl IntoView + use<> {
    let state = *state;
    view! {
        <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
            <h2 class="text-base font-semibold text-crono-text">{move || if state.editing_id.get().is_some() { "Edit Target Set" } else { "Create Target Set" }}</h2>
            <form class="mt-5 space-y-5" on:submit=move |event| state.submit.run(event) novalidate>
                <div class="space-y-1">
                    <ResourceSelect id="target-set-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_error select_single=true />
                    <p class="text-xs text-crono-muted">"The Target Set and all its members belong to this Namespace."</p>
                </div>
                <Show when=move || !state.namespace_choices.loading.get() && state.namespace_choices.options.get().is_empty() && state.namespace_choices.load_error.get().is_none()><p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No namespaces exist yet. "<A href="/namespaces" attr:class="font-medium text-crono-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary">"Create one first."</A></p></Show>
                <div class="space-y-1">
                    <ResourceNameInput id="target-set-name" label="Name" value=state.name error=state.name_error />
                    <p class="text-xs text-crono-muted">"Use a name that describes the group, such as "<code class="font-mono">"web-fleet"</code>" or "<code class="font-mono">"greeting-team"</code>"."</p>
                </div>
                <div class="space-y-2">
                    <ResourceMultiSelect id="target-set-targets" label="Member Targets" options=state.target_choices.options selected=state.target_ids loading=state.target_choices.loading load_error=state.target_choices.load_error field_error=state.member_error />
                    <p class="text-xs text-crono-muted">"Choose existing Targets explicitly. New Targets are added to this group only when you select them."</p>
                    <Show when=move || state.namespace_id.get().is_some() && !state.target_choices.loading.get() && state.target_choices.options.get().is_empty() && state.target_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"This Namespace has no Targets yet. "<A href="/targets/new" attr:class=QUIET_ACTION_CLASS>"Create a Target"</A>" before adding members."</p>
                    </Show>
                    <Show when=move || !state.target_ids.get().is_empty()>
                        <p role="status" class="rounded-md bg-crono-primary-soft px-3 py-2 text-sm text-crono-text">
                            {move || {
                                let count = state.target_ids.get().len();
                                if count == 1 {
                                    "Running a Job on this set creates 1 Run for its selected Target.".to_string()
                                } else {
                                    format!("Running a Job on this set creates {count} Runs, one per selected Target.")
                                }
                            }}
                        </p>
                    </Show>
                </div>
                <div class="space-y-1">
                    <JsonObjectInput id="target-set-inputs" label="Shared inputs (optional)" value=state.inputs error=state.input_error />
                    <p class="text-xs text-crono-muted">"Common values for every member, for example "<code class="break-all font-mono">"{\"greeting\":\"Hello\"}"</code>". Keep "<code class="font-mono">"{}"</code>" if none are needed. Target inputs override matching shared values."</p>
                </div>
                <FormActions submit_label="Save Target Set" disabled=state.disabled on_cancel=state.reset />
            </form>
            <p class="mt-4 text-xs text-crono-muted">"Saving defines the group. To execute it, select this Target Set when running a Job or creating a Schedule."</p>
        </section>
    }
}

/// Show member names and counts, with guidance specific to the collection state.
fn target_sets_list(
    state: &TargetSetViewState,
    heading: NodeRef<leptos::html::H2>,
) -> impl IntoView + use<> {
    let state = *state;
    view! {
        <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
            <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                <h2 node_ref=heading tabindex="-1" class="font-semibold text-crono-text">"Target Sets in Namespace"</h2>
            </header>
            {move || state.target_sets.map(|result| match result {
                Ok(page) if page.items.is_empty() => view! {
                    <p class="px-6 py-10 text-center text-sm text-crono-muted">
                        {move || if state.namespace_id.get().is_some() { "No Target Sets in this Namespace yet. Name a group and choose its members above." } else { "Select a Namespace above to browse its Target Sets." }}
                    </p>
                }.into_any(),
                Ok(page) => view! {
                    <ul class="divide-y divide-crono-border">
                        {page.items.iter().cloned().map(|set| {
                            let edit_set = set.clone();
                            let count = set.targets.len();
                            let members = set.targets.iter().map(|target| target.name.clone()).collect::<Vec<_>>().join(", ");
                            view! {
                                <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6">
                                    <div class="min-w-0">
                                        <p class="break-words font-medium text-crono-text">{set.qualified_name}</p>
                                        <p class="mt-1 break-words text-sm text-crono-muted">{format!("{count} {} · {members}", if count == 1 { "Target" } else { "Targets" })}</p>
                                    </div>
                                    <button type="button" class=QUIET_ACTION_CLASS on:click=move |_| {
                                        state.editing_id.set(Some(edit_set.id));
                                        state.namespace_id.set(Some(edit_set.namespace_id));
                                        state.name.set(edit_set.name.clone());
                                        state.target_ids.set(edit_set.targets.iter().map(|target| target.id).collect());
                                        state.inputs.set(pretty_json(&edit_set.inputs));
                                    }>"Edit"</button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Target Sets…"</p> }.into_any())}
        </section>
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
    feedback: RwSignal<Option<ResourceFeedback>>,
    target_sets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetSetResource>>>,
}

/// Submit validated fields once; API failures retain inputs and also open the result modal.
fn target_set_submit(
    state: &TargetSetSubmitState,
    reset: Callback<()>,
) -> Callback<leptos::ev::SubmitEvent> {
    let state = *state;
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
        let request = CreateTargetSetRequest {
            name: state.name.get_untracked().trim().to_owned(),
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
                    state.feedback.set(Some(ResourceFeedback::saved(
                        format!("Saved {}.", set.qualified_name),
                        if current_edit.is_some() {
                            "Done"
                        } else {
                            "Create another Target Set"
                        },
                    )));
                    state.target_sets.refetch();
                }
                Err(error) => {
                    if let Some(field) = error.field {
                        state.server_field.set(Some((field, error.message.clone())));
                    } else if error.code == "already_exists" {
                        state
                            .server_field
                            .set(Some(("name".to_string(), error.message.clone())));
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
