//! Explicit manual Run creation using the existing Job, Target, and input selectors.
//!
//! The server owns authorization and snapshot creation. This form submits only
//! the selected identities and invocation inputs; it does not resolve commands.

use super::ACTION_CLASS;
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, ResourceOption, ResourceSelect,
        parse_input_object,
    },
    navigation::AppRoute,
    pages::resource_options,
};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;

/// Start a manual Run only after explicit selection and input validation.
#[component]
pub fn RunJobPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let job_id = RwSignal::new(None);
    let target_id = RwSignal::new(None);
    let inputs = RwSignal::new("{}".to_string());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let feedback = RwSignal::new(None::<String>);
    let created_id = RwSignal::new(None::<uuid::Uuid>);
    let namespace_choices = resource_options::namespaces();
    let job_choices = resource_options::jobs(namespace_id);
    let target_choices = resource_options::targets(namespace_id);
    let set_choices = resource_options::target_sets(namespace_id);
    let destination_choices = destination_options(target_choices, set_choices);
    let namespace_error = required_error(attempted, namespace_id, "Select a Namespace.");
    let job_error = required_error(attempted, job_id, "Select a Job.");
    let target_error = required_error(attempted, target_id, "Select a Target or Target Set.");
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
    let disabled = Signal::derive(move || {
        submitting.get()
            || job_id.get().is_none()
            || target_id.get().is_none()
            || input_error.get().is_some()
    });
    let reset = reset_form(job_id, target_id, inputs, attempted, feedback, created_id);
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        attempted.set(true);
        feedback.set(None);
        created_id.set(None);
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
                    created_id.set(batch.runs.first().map(|run| run.id));
                }
                Err(error) => feedback.set(Some(error.message)),
            }
            submitting.set(false);
        });
    };
    view! {
        <div class="space-y-8">
            <PageHeader title="Run a Job" description="Start a manual execution from a Job and a Target or Target Set.">
                <A href=AppRoute::Runs.path() attr:class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover">"← All Runs"</A>
            </PageHeader>
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <form class="space-y-4" on:submit=submit novalidate>
                    <ResourceSelect id="run-namespace" label="Namespace" placeholder="Search/select namespace…" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error field_error=namespace_error />
                    <div class="grid gap-4 md:grid-cols-2">
                        <ResourceSelect id="run-job" label="Job" placeholder="Search/select job…" options=job_choices.options selected=job_id loading=job_choices.loading load_error=job_choices.load_error field_error=job_error />
                        <ResourceSelect id="run-target" label="Destination" placeholder="Search/select Target or Target Set…" options=destination_choices selected=target_id loading=Signal::derive(move || target_choices.loading.get() || set_choices.loading.get()) load_error=Signal::derive(move || target_choices.load_error.get().or_else(|| set_choices.load_error.get())) field_error=target_error />
                    </div>
                    <JsonObjectInput id="run-inputs" label="Invocation inputs" value=inputs error=input_error />
                    <Show when=move || namespace_id.get().is_some() && !job_choices.loading.get() && job_choices.options.get().is_empty() && job_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No Jobs exist in this Namespace. Create a Job before starting a Run."</p>
                    </Show>
                    <Show when=move || namespace_id.get().is_some() && !target_choices.loading.get() && target_choices.options.get().is_empty() && target_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No Targets exist in this Namespace. Create a Target before starting a Run."</p>
                    </Show>
                    <FormActions submit_label="Run Job" disabled=disabled on_cancel=reset />
                </form>
                <div class="mt-4 flex items-center gap-3">
                    <p class="text-sm text-crono-muted" role="status">{move || feedback.get().unwrap_or_default()}</p>
                    {move || created_id.get().map(|id| view! { <A href=crate::navigation::run_details_path(id) attr:class=ACTION_CLASS>"View Run"</A> })}
                </div>
            </section>
        </div>
    }
}

fn reset_form(
    job_id: RwSignal<Option<uuid::Uuid>>,
    target_id: RwSignal<Option<uuid::Uuid>>,
    inputs: RwSignal<String>,
    attempted: RwSignal<bool>,
    feedback: RwSignal<Option<String>>,
    created_id: RwSignal<Option<uuid::Uuid>>,
) -> Callback<()> {
    Callback::new(move |()| {
        job_id.set(None);
        target_id.set(None);
        inputs.set("{}".to_string());
        attempted.set(false);
        feedback.set(None);
        created_id.set(None);
    })
}

fn required_error(
    attempted: RwSignal<bool>,
    value: RwSignal<Option<uuid::Uuid>>,
    message: &'static str,
) -> Signal<Option<String>> {
    Signal::derive(move || (attempted.get() && value.get().is_none()).then(|| message.to_string()))
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
