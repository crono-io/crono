//! Schedule creation and enablement controls.
//!
//! Schedules bind one Job to either a Target or Target Set by UUID. Cron and
//! one-shot timing share the same input overlay and fan-out behavior used by
//! manual Runs.

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, ResourceNameInput, ResourceOption,
        ResourceSelect, name_validation_message, parse_input_object, visible_name_validation,
    },
};
use crono_api::{
    CatchupPolicy, CreateScheduleRequest, ExecutionTarget, ExecutionTargetResource, MisfirePolicy,
    UpdateScheduleRequest,
};
use leptos::{prelude::*, task::spawn_local};

const FIELD_CLASS: &str = "mt-1.5 w-full rounded-md border border-crono-border bg-white px-3 py-2.5 text-sm text-crono-text shadow-sm outline-none focus:border-crono-primary focus:ring-2 focus:ring-crono-primary-soft";

/// Create schedules and toggle existing schedule activity.
#[component]
pub fn SchedulesPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let name = RwSignal::new(String::new());
    let job_id = RwSignal::new(None);
    let destination_id = RwSignal::new(None);
    let timing = RwSignal::new("cron".to_string());
    let cron_expression = RwSignal::new("0 * * * *".to_string());
    let execute_at = RwSignal::new(String::new());
    let timezone = RwSignal::new("UTC".to_string());
    let inputs = RwSignal::new("{}".to_string());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let feedback = RwSignal::new(None::<String>);
    let namespace_choices = resource_options::namespaces();
    let job_choices = resource_options::jobs(namespace_id);
    let target_choices = resource_options::targets(namespace_id);
    let set_choices = resource_options::target_sets(namespace_id);
    let schedules = schedule_resource(namespace_id);
    clear_schedule_selections(namespace_id, job_id, destination_id);
    let destinations = schedule_destinations(target_choices, set_choices);
    let name_error = Signal::derive(move || visible_name_validation(&name.get(), attempted.get()));
    let input_error = Signal::derive(move || parse_input_object(&inputs.get()).err());
    let namespace_error = Signal::derive(move || {
        (attempted.get() && namespace_id.get().is_none()).then(|| "Select a Namespace.".to_string())
    });
    let job_error = Signal::derive(move || {
        (attempted.get() && job_id.get().is_none()).then(|| "Select a Job.".to_string())
    });
    let destination_error = Signal::derive(move || {
        (attempted.get() && destination_id.get().is_none())
            .then(|| "Select a Target or Target Set.".to_string())
    });
    let reset = Callback::new(move |()| {
        name.set(String::new());
        job_id.set(None);
        destination_id.set(None);
        inputs.set("{}".to_string());
        attempted.set(false);
    });
    let disabled = Signal::derive(move || {
        submitting.get()
            || namespace_id.get().is_none()
            || job_id.get().is_none()
            || destination_id.get().is_none()
            || name_validation_message(&name.get(), true).is_some()
            || input_error.get().is_some()
            || (timing.get() == "cron" && cron_expression.get().trim().is_empty())
            || (timing.get() == "once" && execute_at.get().trim().is_empty())
    });
    let submit = schedule_submit(
        &ScheduleSubmitState {
            namespace_id,
            name,
            job_id,
            destination_id,
            timing,
            cron_expression,
            execute_at,
            timezone,
            inputs,
            attempted,
            submitting,
            feedback,
            set_choices,
            schedules,
        },
        reset,
    );

    schedules_view(&ScheduleViewState {
        namespace_id,
        name,
        job_id,
        destination_id,
        timing,
        cron_expression,
        execute_at,
        timezone,
        inputs,
        feedback,
        namespace_choices,
        job_choices,
        target_choices,
        set_choices,
        schedules,
        destinations,
        name_error,
        input_error,
        namespace_error,
        job_error,
        destination_error,
        disabled,
        reset,
        submit,
    })
}

fn schedule_resource(
    namespace_id: RwSignal<Option<uuid::Uuid>>,
) -> LocalResource<api::ApiResult<crono_api::Page<crono_api::ScheduleResource>>> {
    LocalResource::new(move || async move {
        match namespace_id.get() {
            Some(id) => api::list_schedules(id).await,
            None => Ok(crono_api::Page {
                items: Vec::new(),
                next_cursor: None,
            }),
        }
    })
}

fn clear_schedule_selections(
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    job_id: RwSignal<Option<uuid::Uuid>>,
    destination_id: RwSignal<Option<uuid::Uuid>>,
) {
    let previous = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous.get_untracked() != current {
            job_id.set(None);
            destination_id.set(None);
            previous.set(current);
        }
    });
}

fn schedule_destinations(
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

#[derive(Clone, Copy)]
struct ScheduleSubmitState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    job_id: RwSignal<Option<uuid::Uuid>>,
    destination_id: RwSignal<Option<uuid::Uuid>>,
    timing: RwSignal<String>,
    cron_expression: RwSignal<String>,
    execute_at: RwSignal<String>,
    timezone: RwSignal<String>,
    inputs: RwSignal<String>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    feedback: RwSignal<Option<String>>,
    set_choices: resource_options::ResourceOptions,
    schedules: LocalResource<api::ApiResult<crono_api::Page<crono_api::ScheduleResource>>>,
}

fn schedule_submit(
    state: &ScheduleSubmitState,
    reset: Callback<()>,
) -> Callback<leptos::ev::SubmitEvent> {
    let state = *state;
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        state.attempted.set(true);
        state.feedback.set(None);
        let selection = (
            state.namespace_id.get_untracked(),
            state.job_id.get_untracked(),
            state.destination_id.get_untracked(),
        );
        let (Some(namespace), Some(job), Some(destination)) = selection else {
            return;
        };
        let Ok(inputs) = parse_input_object(&state.inputs.get_untracked()) else {
            return;
        };
        let target = if state
            .set_choices
            .options
            .get_untracked()
            .iter()
            .any(|option| option.id == destination)
        {
            ExecutionTarget::TargetSet { id: destination }
        } else {
            ExecutionTarget::Target { id: destination }
        };
        let is_cron = state.timing.get_untracked() == "cron";
        let request = CreateScheduleRequest {
            name: state.name.get_untracked(),
            job_id: job,
            target,
            inputs,
            cron_expression: is_cron.then(|| state.cron_expression.get_untracked()),
            execute_at: (!is_cron).then(|| state.execute_at.get_untracked()),
            timezone: state.timezone.get_untracked(),
            misfire_policy: MisfirePolicy::RunLate,
            misfire_grace_seconds: None,
            catchup_policy: CatchupPolicy::RunOnce,
            max_catchup_runs: 100,
            max_catchup_age_seconds: 86_400,
        };
        if name_validation_message(&request.name, true).is_some() {
            return;
        }
        state.submitting.set(true);
        spawn_local(async move {
            match api::create_schedule(namespace, &request).await {
                Ok(schedule) => {
                    reset.run(());
                    state.feedback.set(Some(format!(
                        "Created {}/{}.",
                        schedule.namespace, schedule.name
                    )));
                    state.schedules.refetch();
                }
                Err(error) => state.feedback.set(Some(error.message)),
            }
            state.submitting.set(false);
        });
    })
}

#[derive(Clone, Copy)]
struct ScheduleViewState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    job_id: RwSignal<Option<uuid::Uuid>>,
    destination_id: RwSignal<Option<uuid::Uuid>>,
    timing: RwSignal<String>,
    cron_expression: RwSignal<String>,
    execute_at: RwSignal<String>,
    timezone: RwSignal<String>,
    inputs: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    job_choices: resource_options::ResourceOptions,
    target_choices: resource_options::ResourceOptions,
    set_choices: resource_options::ResourceOptions,
    schedules: LocalResource<api::ApiResult<crono_api::Page<crono_api::ScheduleResource>>>,
    destinations: Signal<Vec<ResourceOption>>,
    name_error: Signal<Option<String>>,
    input_error: Signal<Option<String>>,
    namespace_error: Signal<Option<String>>,
    job_error: Signal<Option<String>>,
    destination_error: Signal<Option<String>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
}

fn schedules_view(state: &ScheduleViewState) -> impl IntoView + use<> {
    let state = *state;
    view! {
        <div class="space-y-8">
            <PageHeader title="Schedules" description="Run a Job once or on a cron expression against a Target or Target Set." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Schedule"</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect id="schedule-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_error />
                    <ResourceNameInput id="schedule-name" label="Name" value=state.name error=state.name_error />
                    <div class="grid gap-4 md:grid-cols-2"><ResourceSelect id="schedule-job" label="Job" placeholder="Search/select Job…" options=state.job_choices.options selected=state.job_id loading=state.job_choices.loading load_error=state.job_choices.load_error field_error=state.job_error /><ResourceSelect id="schedule-destination" label="Destination" placeholder="Search/select Target or Target Set…" options=state.destinations selected=state.destination_id loading=Signal::derive(move || state.target_choices.loading.get() || state.set_choices.loading.get()) load_error=Signal::derive(move || state.target_choices.load_error.get().or_else(|| state.set_choices.load_error.get())) field_error=state.destination_error /></div>
                    <div class="grid gap-4 md:grid-cols-3"><label class="text-sm font-medium text-crono-text">"Timing"<select class=FIELD_CLASS prop:value=move || state.timing.get() on:change=move |event| state.timing.set(event_target_value(&event))><option value="cron">"Cron"</option><option value="once">"One shot"</option></select></label><Show when=move || state.timing.get() == "cron" fallback=move || view! { <label class="text-sm font-medium text-crono-text md:col-span-2">"Execute at (RFC 3339)"<input class=FIELD_CLASS type="text" placeholder="2026-09-25T12:00:00Z" prop:value=move || state.execute_at.get() on:input=move |event| state.execute_at.set(event_target_value(&event))/></label> }><label class="text-sm font-medium text-crono-text">"Cron expression"<input class=FIELD_CLASS type="text" prop:value=move || state.cron_expression.get() on:input=move |event| state.cron_expression.set(event_target_value(&event))/></label><label class="text-sm font-medium text-crono-text">"Timezone"<input class=FIELD_CLASS type="text" prop:value=move || state.timezone.get() on:input=move |event| state.timezone.set(event_target_value(&event))/></label></Show></div>
                    <JsonObjectInput id="schedule-inputs" label="Invocation inputs" value=state.inputs error=state.input_error />
                    <FormActions submit_label="Create Schedule" disabled=state.disabled on_cancel=state.reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface"><header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Schedules in Namespace"</h2></header>{move || state.schedules.map(|result| match result {
                Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Schedule."</p> }.into_any(),
                Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().cloned().map(|schedule| { let toggle = schedule.clone(); let destination = match &schedule.target { ExecutionTargetResource::Target { name, .. } => format!("Target · {name}"), ExecutionTargetResource::TargetSet { name, .. } => format!("Target Set · {name}") }; view! { <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6"><div><p class="font-medium text-crono-text">{format!("{}/{}", schedule.namespace, schedule.name)}</p><p class="text-sm text-crono-muted">{format!("{} → {} · next {}", schedule.job, destination, schedule.next_run_at.unwrap_or_else(|| "not scheduled".to_string()))}</p></div><button type="button" class="text-sm font-medium text-crono-primary" on:click=move |_| { spawn_local(async move { match api::update_schedule(toggle.id, &UpdateScheduleRequest { revision: toggle.revision, enabled: !toggle.enabled }).await { Ok(_) => { state.feedback.set(Some("Schedule updated.".to_string())); state.schedules.refetch(); }, Err(error) => state.feedback.set(Some(error.message)), } }); }>{if schedule.enabled { "Disable" } else { "Enable" }}</button></li> } }).collect_view()}</ul> }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Schedules…"</p> }.into_any())}</section>
        </div>
    }
}
