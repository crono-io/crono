//! Namespace-scoped Job creation using UUID-backed resource selection.

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

/// Create direct no-op Jobs without requiring users to type Namespace keys.
#[component]
pub fn JobsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let name = RwSignal::new(String::new());
    let queue = RwSignal::new("default".to_string());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let name_server_error = RwSignal::new(None::<String>);
    let namespace_server_error = RwSignal::new(None::<String>);
    let queue_server_error = RwSignal::new(None::<String>);
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
    let queue_error = Signal::derive(move || {
        queue_server_error
            .get()
            .or_else(|| visible_name_validation(&queue.get(), attempted.get()))
    });
    let jobs = LocalResource::new(move || {
        let selected = namespace_id.get();
        async move {
            match selected {
                Some(id) => api::list_jobs(id).await,
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
            || name_validation_message(&queue.get(), true).is_some()
    });
    let reset = Callback::new(move |()| {
        name.set(String::new());
        queue.set("default".to_string());
        attempted.set(false);
        name_server_error.set(None);
        namespace_server_error.set(None);
        queue_server_error.set(None);
        feedback.set(None);
    });
    let submit = job_submit(JobSubmission {
        namespace_id,
        name,
        queue,
        attempted,
        submitting,
        name_server_error,
        namespace_server_error,
        queue_server_error,
        feedback,
        jobs,
    });
    job_page(JobPageState {
        namespace_id,
        name,
        queue,
        feedback,
        namespace_choices,
        namespace_field_error,
        name_error,
        queue_error,
        jobs,
        disabled,
        reset,
        submit,
    })
}

#[derive(Clone, Copy)]
struct JobSubmission {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    queue: RwSignal<String>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    name_server_error: RwSignal<Option<String>>,
    namespace_server_error: RwSignal<Option<String>>,
    queue_server_error: RwSignal<Option<String>>,
    feedback: RwSignal<Option<String>>,
    jobs: LocalResource<api::ApiResult<crono_api::Page<crono_api::JobResource>>>,
}

fn job_submit(state: JobSubmission) -> Callback<leptos::ev::SubmitEvent> {
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        state.attempted.set(true);
        state.name_server_error.set(None);
        state.namespace_server_error.set(None);
        state.queue_server_error.set(None);
        state.feedback.set(None);
        let Some(selected) = state.namespace_id.get_untracked() else {
            return;
        };
        let job_name = state.name.get_untracked();
        let queue_name = state.queue.get_untracked();
        if name_validation_message(&job_name, true).is_some()
            || name_validation_message(&queue_name, true).is_some()
        {
            return;
        }
        state.submitting.set(true);
        spawn_local(async move {
            match api::create_job(selected, job_name, queue_name).await {
                Ok(job) => {
                    state.name.set(String::new());
                    state.attempted.set(false);
                    state
                        .feedback
                        .set(Some(format!("Created {}.", job.qualified_name)));
                    state.jobs.refetch();
                }
                Err(error) => match error.field.as_deref() {
                    Some("namespace_id") => {
                        state.namespace_server_error.set(Some(error.message));
                    }
                    Some("name") => state.name_server_error.set(Some(error.message)),
                    Some("queue") => state.queue_server_error.set(Some(error.message)),
                    _ => state.feedback.set(Some(error.message)),
                },
            }
            state.submitting.set(false);
        });
    })
}

#[derive(Clone, Copy)]
struct JobPageState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    queue: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    namespace_field_error: Signal<Option<String>>,
    name_error: Signal<Option<String>>,
    queue_error: Signal<Option<String>>,
    jobs: LocalResource<api::ApiResult<crono_api::Page<crono_api::JobResource>>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn job_page(state: JobPageState) -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader title="Jobs" description="Jobs define what Crono snapshots into each durable Run." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Job"</h2>
                <form class="mt-5 space-y-4" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect
                        id="job-namespace"
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
                            <A href="/namespaces" attr:class="font-medium text-crono-primary hover:text-crono-primary-hover">"Create a namespace before creating a job."</A>
                        </p>
                    </Show>
                    <ResourceNameInput id="job-name" label="Name" value=state.name error=state.name_error />
                    <div>
                        <label for="job-queue" class="block text-sm font-medium text-crono-text">"Queue"<span class="ml-1 text-crono-failed">"*"</span></label>
                        <input id="job-queue" class="mt-1.5 w-full rounded-md border border-crono-border px-3 py-2.5 text-sm" aria-invalid=move || state.queue_error.get().is_some().then_some("true") prop:value=move || state.queue.get() on:input=move |event| state.queue.set(event_target_value(&event)) />
                        <p class="mt-1.5 text-xs text-crono-muted">"Queue tokens use lowercase letters, numbers and hyphens."</p>
                        <p class="mt-1 text-sm text-crono-failed" role="alert">{move || state.queue_error.get().unwrap_or_default()}</p>
                    </div>
                    <FormActions submit_label="Create Job" disabled=state.disabled on_cancel=state.reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Jobs in Namespace"</h2></header>
                {move || state.jobs.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Job."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|job| view! {
                        <li class="grid gap-1 px-5 py-4 sm:grid-cols-[1fr_auto] sm:px-6">
                            <span class="font-medium text-crono-text">{job.qualified_name.clone()}</span>
                            <span class="text-sm text-crono-muted">{format!("{:?} · {}", job.executor, job.queue)}</span>
                        </li>
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Jobs…"</p> }.into_any())}
            </section>
        </div>
    }
}
