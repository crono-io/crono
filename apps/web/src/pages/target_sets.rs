//! Persisted Target Set creation with searchable UUID-backed membership.

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, PageHeader, ResourceMultiSelect, ResourceNameInput, ResourceSelect,
        name_validation_message, visible_name_validation,
    },
};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;

/// Create and list named, explicit Target selections in one Namespace.
#[component]
pub fn TargetSetsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let name = RwSignal::new(String::new());
    let target_ids = RwSignal::new(Vec::new());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let name_server_error = RwSignal::new(None::<String>);
    let namespace_server_error = RwSignal::new(None::<String>);
    let targets_server_error = RwSignal::new(None::<String>);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
    let target_choices = resource_options::targets(namespace_id);
    let target_sets = LocalResource::new(move || {
        let selected = namespace_id.get();
        async move {
            match selected {
                Some(id) => api::list_target_sets(id).await,
                None => Ok(crono_api::Page {
                    items: Vec::new(),
                    next_cursor: None,
                }),
            }
        }
    });
    let previous_namespace = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous_namespace.get_untracked() != current {
            target_ids.set(Vec::new());
            previous_namespace.set(current);
        }
    });
    let namespace_field_error = Signal::derive(move || {
        namespace_server_error.get().or_else(|| {
            (attempted.get() && namespace_id.get().is_none())
                .then(|| "Select a Namespace.".to_string())
        })
    });
    let name_error = Signal::derive(move || {
        name_server_error
            .get()
            .or_else(|| visible_name_validation(&name.get(), attempted.get()))
    });
    let member_error = Signal::derive(move || {
        targets_server_error.get().or_else(|| {
            (attempted.get() && target_ids.get().is_empty())
                .then(|| "Select at least one Target.".to_string())
        })
    });
    let disabled = Signal::derive(move || {
        submitting.get()
            || namespace_id.get().is_none()
            || target_ids.get().is_empty()
            || name_validation_message(&name.get(), true).is_some()
    });
    let reset = Callback::new(move |()| {
        name.set(String::new());
        target_ids.set(Vec::new());
        attempted.set(false);
        name_server_error.set(None);
        namespace_server_error.set(None);
        targets_server_error.set(None);
        feedback.set(None);
    });
    let submit = target_set_submit(TargetSetSubmission {
        namespace_id,
        name,
        target_ids,
        attempted,
        submitting,
        name_server_error,
        namespace_server_error,
        targets_server_error,
        feedback,
        target_sets,
    });
    target_sets_page(TargetSetsPageState {
        namespace_id,
        name,
        target_ids,
        feedback,
        namespace_choices,
        target_choices,
        namespace_field_error,
        name_error,
        member_error,
        target_sets,
        disabled,
        reset,
        submit,
    })
}

#[derive(Clone, Copy)]
struct TargetSetSubmission {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    target_ids: RwSignal<Vec<uuid::Uuid>>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    name_server_error: RwSignal<Option<String>>,
    namespace_server_error: RwSignal<Option<String>>,
    targets_server_error: RwSignal<Option<String>>,
    feedback: RwSignal<Option<String>>,
    target_sets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetSetResource>>>,
}

fn target_set_submit(state: TargetSetSubmission) -> Callback<leptos::ev::SubmitEvent> {
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        state.attempted.set(true);
        state.name_server_error.set(None);
        state.namespace_server_error.set(None);
        state.targets_server_error.set(None);
        state.feedback.set(None);
        let Some(selected_namespace) = state.namespace_id.get_untracked() else {
            return;
        };
        let set_name = state.name.get_untracked();
        let members = state.target_ids.get_untracked();
        if name_validation_message(&set_name, true).is_some() || members.is_empty() {
            return;
        }
        state.submitting.set(true);
        spawn_local(async move {
            match api::create_target_set(selected_namespace, set_name, members).await {
                Ok(target_set) => {
                    state.name.set(String::new());
                    state.target_ids.set(Vec::new());
                    state.attempted.set(false);
                    state
                        .feedback
                        .set(Some(format!("Created {}.", target_set.qualified_name)));
                    state.target_sets.refetch();
                }
                Err(error) => match error.field.as_deref() {
                    Some("namespace_id") => {
                        state.namespace_server_error.set(Some(error.message));
                    }
                    Some("name") => state.name_server_error.set(Some(error.message)),
                    Some("target_ids") => state.targets_server_error.set(Some(error.message)),
                    _ => state.feedback.set(Some(error.message)),
                },
            }
            state.submitting.set(false);
        });
    })
}

#[derive(Clone, Copy)]
struct TargetSetsPageState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    target_ids: RwSignal<Vec<uuid::Uuid>>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    target_choices: resource_options::ResourceOptions,
    namespace_field_error: Signal<Option<String>>,
    name_error: Signal<Option<String>>,
    member_error: Signal<Option<String>>,
    target_sets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetSetResource>>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn target_sets_page(state: TargetSetsPageState) -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader title="Target Sets" description="Target Sets organize explicit selections of Targets in one Namespace." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Target Set"</h2>
                <form class="mt-5 space-y-4" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect
                        id="target-set-namespace"
                        label="Namespace"
                        placeholder="Search/select namespace…"
                        options=state.namespace_choices.options
                        selected=state.namespace_id
                        loading=state.namespace_choices.loading
                        load_error=state.namespace_choices.load_error
                        field_error=state.namespace_field_error
                    />
                    <Show when=move || !state.namespace_choices.loading.get() && state.namespace_choices.options.get().is_empty() && state.namespace_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">
                            "No namespaces exist yet. "
                            <A href="/namespaces" attr:class="font-medium text-crono-primary">"Create one first."</A>
                        </p>
                    </Show>
                    <ResourceNameInput id="target-set-name" label="Name" value=state.name error=state.name_error />
                    <ResourceMultiSelect
                        id="target-set-targets"
                        label="Targets"
                        options=state.target_choices.options
                        selected=state.target_ids
                        loading=state.target_choices.loading
                        load_error=state.target_choices.load_error
                        field_error=state.member_error
                    />
                    <Show when=move || state.namespace_id.get().is_some() && !state.target_choices.loading.get() && state.target_choices.options.get().is_empty() && state.target_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">
                            "No Targets exist in this Namespace. "
                            <A href="/targets" attr:class="font-medium text-crono-primary">"Create a Target first."</A>
                        </p>
                    </Show>
                    <FormActions submit_label="Create Target Set" disabled=state.disabled on_cancel=state.reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Target Sets in Namespace"</h2></header>
                {move || state.target_sets.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Target Set."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|target_set| view! {
                        <li class="px-5 py-4 sm:px-6">
                            <p class="font-medium text-crono-text">{target_set.qualified_name.clone()}</p>
                            <p class="mt-1 text-sm text-crono-muted">{target_set.targets.iter().map(|target| target.name.clone()).collect::<Vec<_>>().join(", ")}</p>
                        </li>
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Target Sets…"</p> }.into_any())}
            </section>
        </div>
    }
}
