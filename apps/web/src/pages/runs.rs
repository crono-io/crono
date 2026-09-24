//! Live Run creation through UUID-backed Job and Target selection.

use super::resource_options;
use crate::{
    api,
    components::{FormActions, PageHeader, ResourceSelect},
};
use leptos::{prelude::*, task::spawn_local};

/// Select existing resources and inspect durable dispatch state.
#[component]
pub fn RunsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let job_id = RwSignal::new(None);
    let target_id = RwSignal::new(None);
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
    let job_choices = resource_options::jobs(namespace_id);
    let target_choices = resource_options::targets(namespace_id);
    let namespace_field_error = Signal::derive(move || {
        (attempted.get() && namespace_id.get().is_none()).then(|| "Select a Namespace.".to_string())
    });
    let job_field_error = Signal::derive(move || {
        (attempted.get() && job_id.get().is_none()).then(|| "Select a Job.".to_string())
    });
    let target_field_error = Signal::derive(move || {
        (attempted.get() && target_id.get().is_none()).then(|| "Select a Target.".to_string())
    });
    let previous_namespace = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous_namespace.get_untracked() != current {
            job_id.set(None);
            target_id.set(None);
            previous_namespace.set(current);
        }
    });
    let runs = LocalResource::new(api::list_runs);
    let disabled = Signal::derive(move || {
        submitting.get() || job_id.get().is_none() || target_id.get().is_none()
    });
    let reset = Callback::new(move |()| {
        job_id.set(None);
        target_id.set(None);
        attempted.set(false);
        feedback.set(None);
    });
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        attempted.set(true);
        feedback.set(None);
        let (Some(job), Some(target)) = (job_id.get_untracked(), target_id.get_untracked()) else {
            return;
        };
        submitting.set(true);
        spawn_local(async move {
            match api::create_run(job, target).await {
                Ok(run) => {
                    feedback.set(Some(format!("Created Run {}.", run.id)));
                    runs.refetch();
                }
                Err(error) => feedback.set(Some(error.message)),
            }
            submitting.set(false);
        });
    };

    view! {
        <div class="space-y-8">
            <PageHeader title="Runs" description="Create a durable Run, then observe its transition from pending dispatch to JetStream acknowledged." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Run a Job"</h2>
                <form class="mt-5 space-y-4" on:submit=submit novalidate>
                    <ResourceSelect id="run-namespace" label="Namespace" placeholder="Search/select namespace…" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error field_error=namespace_field_error />
                    <div class="grid gap-4 md:grid-cols-2">
                        <ResourceSelect id="run-job" label="Job" placeholder="Search/select job…" options=job_choices.options selected=job_id loading=job_choices.loading load_error=job_choices.load_error field_error=job_field_error />
                        <ResourceSelect id="run-target" label="Target" placeholder="Search/select target…" options=target_choices.options selected=target_id loading=target_choices.loading load_error=target_choices.load_error field_error=target_field_error />
                    </div>
                    <Show when=move || namespace_id.get().is_some() && !job_choices.loading.get() && job_choices.options.get().is_empty() && job_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No Jobs exist in this Namespace. Create a Job before starting a Run."</p>
                    </Show>
                    <Show when=move || namespace_id.get().is_some() && !target_choices.loading.get() && target_choices.options.get().is_empty() && target_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No Targets exist in this Namespace. Create a Target before starting a Run."</p>
                    </Show>
                    <FormActions submit_label="Run Job" disabled=disabled on_cancel=reset />
                </form>
                <div class="mt-3 flex flex-wrap items-center gap-3">
                    <p class="text-sm text-crono-muted" role="status">{move || feedback.get().unwrap_or_default()}</p>
                    <button class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover" type="button" on:click=move |_| runs.refetch()>"Refresh status"</button>
                </div>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Recent Runs"</h2></header>
                {move || runs.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Runs yet."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|run| {
                        let status = run_status(run.status);
                        view! {
                            <li class="grid gap-2 px-5 py-4 sm:grid-cols-[1fr_auto] sm:px-6">
                                <div><p class="font-medium text-crono-text">{format!("{} → {}", run.job, run.target)}</p><code class="text-xs text-crono-muted">{run.id.to_string()}</code></div>
                                <span class="self-center rounded-full bg-crono-primary-soft px-2.5 py-1 text-xs font-medium text-crono-primary">{status}</span>
                            </li>
                        }
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Runs…"</p> }.into_any())}
            </section>
        </div>
    }
}

pub(super) const fn run_status(status: crono_api::RunStatus) -> &'static str {
    match status {
        crono_api::RunStatus::PendingDispatch => "pending dispatch",
        crono_api::RunStatus::Queued => "queued",
        crono_api::RunStatus::Running => "running",
        crono_api::RunStatus::RetryWait => "retry wait",
        crono_api::RunStatus::Succeeded => "succeeded",
        crono_api::RunStatus::Failed => "failed",
        crono_api::RunStatus::Dead => "dead",
        crono_api::RunStatus::Skipped => "skipped",
        crono_api::RunStatus::Cancelled => "cancelled",
        crono_api::RunStatus::Unknown => "unknown",
    }
}
