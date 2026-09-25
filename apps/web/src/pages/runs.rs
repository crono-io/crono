//! Live Run creation through UUID-backed Job and Target selection.
//!
//! Recent Runs expose their authorized Attempt output on demand, including
//! commands recorded by dry-run workers. Output stays behind the server's
//! `RunRead` decision rather than being included in the list response.

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, ResourceOption, ResourceSelect,
        parse_input_object,
    },
};
use leptos::{prelude::*, task::spawn_local};

/// Select existing resources and inspect durable dispatch state.
#[component]
pub fn RunsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let job_id = RwSignal::new(None);
    let target_id = RwSignal::new(None);
    let inputs = RwSignal::new("{}".to_string());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
    let job_choices = resource_options::jobs(namespace_id);
    let target_choices = resource_options::targets(namespace_id);
    let set_choices = resource_options::target_sets(namespace_id);
    let destination_choices = destination_options(target_choices, set_choices);
    let namespace_field_error = Signal::derive(move || {
        (attempted.get() && namespace_id.get().is_none()).then(|| "Select a Namespace.".to_string())
    });
    let job_field_error = Signal::derive(move || {
        (attempted.get() && job_id.get().is_none()).then(|| "Select a Job.".to_string())
    });
    let target_field_error = Signal::derive(move || {
        (attempted.get() && target_id.get().is_none())
            .then(|| "Select a Target or Target Set.".to_string())
    });
    let input_error = Signal::derive(move || parse_input_object(&inputs.get()).err());
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
        submitting.get()
            || job_id.get().is_none()
            || target_id.get().is_none()
            || input_error.get().is_some()
    });
    let reset = Callback::new(move |()| {
        job_id.set(None);
        target_id.set(None);
        inputs.set("{}".to_string());
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
        let Ok(invocation_inputs) = parse_input_object(&inputs.get_untracked()) else {
            return;
        };
        let destination = if set_choices
            .options
            .get_untracked()
            .iter()
            .any(|option| option.id == target)
        {
            crono_api::ExecutionTarget::TargetSet { id: target }
        } else {
            crono_api::ExecutionTarget::Target { id: target }
        };
        submitting.set(true);
        spawn_local(async move {
            match api::create_run(job, destination, invocation_inputs).await {
                Ok(batch) => {
                    feedback.set(Some(format!("Created {} Run(s).", batch.runs.len())));
                    runs.refetch();
                }
                Err(error) => feedback.set(Some(error.message)),
            }
            submitting.set(false);
        });
    };

    runs_view(&RunsViewState {
        namespace_id,
        job_id,
        target_id,
        inputs,
        feedback,
        namespace_choices,
        job_choices,
        target_choices,
        set_choices,
        destination_choices,
        namespace_field_error,
        job_field_error,
        target_field_error,
        input_error,
        runs,
        disabled,
        reset,
        submit: Callback::new(submit),
    })
}

#[derive(Clone, Copy)]
struct RunsViewState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    job_id: RwSignal<Option<uuid::Uuid>>,
    target_id: RwSignal<Option<uuid::Uuid>>,
    inputs: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    job_choices: resource_options::ResourceOptions,
    target_choices: resource_options::ResourceOptions,
    set_choices: resource_options::ResourceOptions,
    destination_choices: Signal<Vec<ResourceOption>>,
    namespace_field_error: Signal<Option<String>>,
    job_field_error: Signal<Option<String>>,
    target_field_error: Signal<Option<String>>,
    input_error: Signal<Option<String>>,
    runs: LocalResource<api::ApiResult<crono_api::Page<crono_api::RunResource>>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn runs_view(state: &RunsViewState) -> impl IntoView + use<> {
    let state = *state;
    view! {
        <div class="space-y-8">
            <PageHeader title="Runs" description="Create a durable Run, then observe its transition from pending dispatch to JetStream acknowledged." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Run a Job"</h2>
                <form class="mt-5 space-y-4" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect id="run-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_field_error />
                    <div class="grid gap-4 md:grid-cols-2">
                        <ResourceSelect id="run-job" label="Job" placeholder="Search/select job…" options=state.job_choices.options selected=state.job_id loading=state.job_choices.loading load_error=state.job_choices.load_error field_error=state.job_field_error />
                        <ResourceSelect id="run-target" label="Destination" placeholder="Search/select Target or Target Set…" options=state.destination_choices selected=state.target_id loading=Signal::derive(move || state.target_choices.loading.get() || state.set_choices.loading.get()) load_error=Signal::derive(move || state.target_choices.load_error.get().or_else(|| state.set_choices.load_error.get())) field_error=state.target_field_error />
                    </div>
                    <JsonObjectInput id="run-inputs" label="Invocation inputs" value=state.inputs error=state.input_error />
                    <Show when=move || state.namespace_id.get().is_some() && !state.job_choices.loading.get() && state.job_choices.options.get().is_empty() && state.job_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No Jobs exist in this Namespace. Create a Job before starting a Run."</p>
                    </Show>
                    <Show when=move || state.namespace_id.get().is_some() && !state.target_choices.loading.get() && state.target_choices.options.get().is_empty() && state.target_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No Targets exist in this Namespace. Create a Target before starting a Run."</p>
                    </Show>
                    <FormActions submit_label="Run Job" disabled=state.disabled on_cancel=state.reset />
                </form>
                <div class="mt-3 flex flex-wrap items-center gap-3">
                    <p class="text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
                    <button class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover" type="button" on:click=move |_| state.runs.refetch()>"Refresh status"</button>
                </div>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Recent Runs"</h2></header>
                {move || state.runs.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Runs yet."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|run| {
                        let status = run_status(run.status);
                        let run_id = run.id;
                        let output_open = RwSignal::new(false);
                        view! {
                            <li class="grid gap-2 px-5 py-4 sm:grid-cols-[1fr_auto] sm:px-6">
                                <div><p class="font-medium text-crono-text">{format!("{} → {}", run.job, run.target)}</p><code class="text-xs text-crono-muted">{run.id.to_string()}</code></div>
                                <span class="self-center rounded-full bg-crono-primary-soft px-2.5 py-1 text-xs font-medium text-crono-primary">{status}</span>
                                <div class="sm:col-span-2">
                                    <button
                                        type="button"
                                        class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover"
                                        aria-expanded=move || output_open.get().to_string()
                                        on:click=move |_| output_open.update(|open| *open = !*open)
                                    >{move || if output_open.get() { "Hide output" } else { "View output" }}</button>
                                    <Show when=move || output_open.get()>
                                        <RunAttemptOutput run_id=run_id />
                                    </Show>
                                </div>
                            </li>
                        }
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Runs…"</p> }.into_any())}
            </section>
        </div>
    }
}

/// Fetch Attempt output only while a Run's disclosure is open.
#[component]
fn RunAttemptOutput(run_id: uuid::Uuid) -> impl IntoView {
    let attempts = LocalResource::new(move || api::list_run_attempts(run_id));
    view! {
        <div class="mt-3 rounded-md border border-crono-border bg-zinc-50 p-3">
            <button type="button" class="text-xs font-medium text-crono-primary" on:click=move |_| attempts.refetch()>"Refresh output"</button>
            {move || attempts.map(|result| match result {
                Ok(items) if items.is_empty() => view! { <p class="mt-2 text-sm text-crono-muted">"No attempts yet."</p> }.into_any(),
                Ok(items) => view! { <div class="mt-2 space-y-3">{items.iter().cloned().map(|attempt| {
                    let status = attempt_status(attempt.status);
                    let stdout = attempt.stdout_tail.filter(|value| !value.is_empty());
                    let stderr = attempt.stderr_tail.filter(|value| !value.is_empty());
                    let error = attempt.error;
                    let no_output = stdout.is_none() && stderr.is_none() && error.is_none();
                    view! {
                        <section class="rounded-md border border-crono-border bg-white p-3">
                            <h3 class="text-sm font-medium text-crono-text">{format!("Attempt {} · {}", attempt.attempt, status)}</h3>
                            <p class="text-xs text-crono-muted">{format!("Started: {} · Completed: {}", attempt.started_at.as_deref().unwrap_or("—"), attempt.completed_at.as_deref().unwrap_or("—"))}</p>
                            {attempt.exit_code.map(|code| view! { <p class="text-xs text-crono-muted">{format!("Exit code: {code}")}</p> })}
                            {stdout.map(|value| view! { <div class="mt-2"><p class="text-xs font-medium text-crono-muted">"stdout"</p><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-crono-text">{value}</pre></div> })}
                            {stderr.map(|value| view! { <div class="mt-2"><p class="text-xs font-medium text-crono-muted">"stderr"</p><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-crono-text">{value}</pre></div> })}
                            {error.map(|value| view! { <p class="mt-2 text-xs text-crono-failed">{value}</p> })}
                            {no_output.then(|| view! { <p class="mt-2 text-xs text-crono-muted">"No output captured."</p> })}
                        </section>
                    }
                }).collect_view()}</div> }.into_any(),
                Err(error) => view! { <p class="mt-2 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="mt-2 text-sm text-crono-muted">"Loading output…"</p> }.into_any())}
        </div>
    }
}

const fn attempt_status(status: crono_api::AttemptStatus) -> &'static str {
    match status {
        crono_api::AttemptStatus::PendingDispatch => "pending dispatch",
        crono_api::AttemptStatus::Queued => "queued",
        crono_api::AttemptStatus::Running => "running",
        crono_api::AttemptStatus::Succeeded => "succeeded",
        crono_api::AttemptStatus::Failed => "failed",
        crono_api::AttemptStatus::Dead => "dead",
        crono_api::AttemptStatus::Unknown => "unknown",
    }
}

fn destination_options(
    targets: resource_options::ResourceOptions,
    sets: resource_options::ResourceOptions,
) -> Signal<Vec<ResourceOption>> {
    Signal::derive(move || {
        let mut options = targets
            .options
            .get()
            .into_iter()
            .map(|option| ResourceOption {
                id: option.id,
                label: format!("Target · {}", option.label),
            })
            .collect::<Vec<_>>();
        options.extend(sets.options.get().into_iter().map(|option| ResourceOption {
            id: option.id,
            label: format!("Target Set · {}", option.label),
        }));
        options
    })
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
