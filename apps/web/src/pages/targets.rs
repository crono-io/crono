//! Namespace-scoped Target creation using UUID-backed resource selection.

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, PageHeader, ResourceNameInput, ResourceSelect, name_validation_message,
        visible_name_validation,
    },
};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;

/// Create identity-only Targets without requiring users to type Namespace keys.
#[component]
pub fn TargetsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let name = RwSignal::new(String::new());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let name_server_error = RwSignal::new(None::<String>);
    let namespace_server_error = RwSignal::new(None::<String>);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
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
    let targets = LocalResource::new(move || {
        let selected = namespace_id.get();
        async move {
            match selected {
                Some(id) => api::list_targets(id).await,
                None => Ok(crono_api::Page {
                    items: Vec::new(),
                    next_cursor: None,
                }),
            }
        }
    });
    let disabled = Signal::derive(move || {
        submitting.get()
            || namespace_id.get().is_none()
            || name_validation_message(&name.get(), true).is_some()
    });
    let reset = Callback::new(move |()| {
        name.set(String::new());
        attempted.set(false);
        name_server_error.set(None);
        namespace_server_error.set(None);
        feedback.set(None);
    });
    let submit = Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        attempted.set(true);
        name_server_error.set(None);
        namespace_server_error.set(None);
        feedback.set(None);
        let Some(selected) = namespace_id.get_untracked() else {
            return;
        };
        let target_name = name.get_untracked();
        if name_validation_message(&target_name, true).is_some() {
            return;
        }
        submitting.set(true);
        spawn_local(async move {
            match api::create_target(selected, target_name).await {
                Ok(target) => {
                    name.set(String::new());
                    attempted.set(false);
                    feedback.set(Some(format!("Created {}.", target.qualified_name)));
                    targets.refetch();
                }
                Err(error) => match error.field.as_deref() {
                    Some("namespace_id") => namespace_server_error.set(Some(error.message)),
                    Some("name") => name_server_error.set(Some(error.message)),
                    _ => feedback.set(Some(error.message)),
                },
            }
            submitting.set(false);
        });
    });
    target_page(TargetPageState {
        namespace_id,
        name,
        feedback,
        namespace_choices,
        namespace_field_error,
        name_error,
        targets,
        disabled,
        reset,
        submit,
    })
}

#[derive(Clone, Copy)]
struct TargetPageState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    namespace_field_error: Signal<Option<String>>,
    name_error: Signal<Option<String>>,
    targets: LocalResource<api::ApiResult<crono_api::Page<crono_api::TargetResource>>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn target_page(state: TargetPageState) -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader title="Targets" description="Targets identify where or against what a Job executes, without coupling that identity to an executor." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Target"</h2>
                <form class="mt-5 space-y-4" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect
                        id="target-namespace"
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
                            <A href="/namespaces" attr:class="font-medium text-crono-primary hover:text-crono-primary-hover">"Create a namespace before creating a target."</A>
                        </p>
                    </Show>
                    <ResourceNameInput id="target-name" label="Name" value=state.name error=state.name_error />
                    <FormActions submit_label="Create Target" disabled=state.disabled on_cancel=state.reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Targets in Namespace"</h2></header>
                {move || state.targets.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Target."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|target| view! {
                        <li class="px-5 py-4 font-medium text-crono-text sm:px-6">{target.qualified_name.clone()}</li>
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Targets…"</p> }.into_any())}
            </section>
        </div>
    }
}
