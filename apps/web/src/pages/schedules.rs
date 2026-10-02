//! Schedule creation and enablement controls.
//!
//! Schedules bind one Job to either a Target or Target Set by UUID. Cron and
//! one-shot timing share the same input overlay and fan-out behavior used by
//! manual Runs. Shared modals report creation and enablement outcomes; failed
//! saves retain field guidance and all entered values. Revisioned updates are
//! bounded to one pending request from the list.

use super::resource_options;
use crate::{
    api,
    components::{
        FormActions, JsonObjectInput, PageHeader, QUIET_ACTION_CLASS, ResourceFeedback,
        ResourceFeedbackModal, ResourceNameInput, ResourceOption, ResourceSelect, TimezoneSelect,
        focus_heading, forms::FIELD_CLASS, name_validation_message, parse_input_object,
        visible_name_validation,
    },
};
use crono_api::{
    CatchupPolicy, CreateScheduleRequest, ExecutionTarget, ExecutionTargetResource, MisfirePolicy,
    UpdateScheduleRequest,
};
use leptos::{prelude::*, task::spawn_local};

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
    let feedback = RwSignal::new(None::<ResourceFeedback>);
    let server_field = RwSignal::new(None::<(String, String)>);
    let namespace_choices = resource_options::namespaces();
    let job_choices = resource_options::jobs(namespace_id);
    let target_choices = resource_options::targets(namespace_id);
    let set_choices = resource_options::target_sets(namespace_id);
    let schedules = schedule_resource(namespace_id);
    clear_schedule_selections(namespace_id, job_id, destination_id);
    let destinations = schedule_destinations(target_choices, set_choices);
    let name_error = Signal::derive(move || {
        schedule_server_error(server_field, "name")
            .or_else(|| visible_name_validation(&name.get(), attempted.get()))
    });
    let input_error = Signal::derive(move || {
        schedule_server_error(server_field, "inputs")
            .or_else(|| parse_input_object(&inputs.get()).err())
    });
    let namespace_error = choice_error(server_field, namespace_id, attempted, "namespace_id");
    let job_error = choice_error(server_field, job_id, attempted, "job_id");
    let destination_error = choice_error(server_field, destination_id, attempted, "target");
    let reset = Callback::new(move |()| {
        name.set(String::new());
        job_id.set(None);
        destination_id.set(None);
        inputs.set("{}".to_string());
        attempted.set(false);
        server_field.set(None);
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
            server_field,
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
        server_field,
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
    feedback: RwSignal<Option<ResourceFeedback>>,
    server_field: RwSignal<Option<(String, String)>>,
    set_choices: resource_options::ResourceOptions,
    schedules: LocalResource<api::ApiResult<crono_api::Page<crono_api::ScheduleResource>>>,
}

/// Submit validated fields once; API failures retain inputs and also open the result modal.
fn schedule_submit(
    state: &ScheduleSubmitState,
    reset: Callback<()>,
) -> Callback<leptos::ev::SubmitEvent> {
    let state = *state;
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if state.submitting.get_untracked() {
            return;
        }
        state.attempted.set(true);
        state.feedback.set(None);
        state.server_field.set(None);
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
                    state.feedback.set(Some(ResourceFeedback::saved(
                        format!("Created {}/{}.", schedule.namespace, schedule.name),
                        "Create another Schedule",
                    )));
                    state.schedules.refetch();
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
    feedback: RwSignal<Option<ResourceFeedback>>,
    server_field: RwSignal<Option<(String, String)>>,
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
    let heading = NodeRef::<leptos::html::H2>::new();
    let updating = RwSignal::new(false);
    view! {
        <div class="space-y-8">
            <PageHeader title="Schedules" description="Run a Job once or on a cron expression against a Target or Target Set." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Schedule"</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceSelect id="schedule-namespace" label="Namespace" placeholder="Search/select namespace…" options=state.namespace_choices.options selected=state.namespace_id loading=state.namespace_choices.loading load_error=state.namespace_choices.load_error field_error=state.namespace_error select_single=true />
                    <ResourceNameInput id="schedule-name" label="Name" value=state.name error=state.name_error />
                    <div class="grid gap-4 md:grid-cols-2"><ResourceSelect id="schedule-job" label="Job" placeholder="Search/select Job…" options=state.job_choices.options selected=state.job_id loading=state.job_choices.loading load_error=state.job_choices.load_error field_error=state.job_error /><ResourceSelect id="schedule-destination" label="Destination" placeholder="Search/select Target or Target Set…" options=state.destinations selected=state.destination_id loading=Signal::derive(move || state.target_choices.loading.get() || state.set_choices.loading.get()) load_error=Signal::derive(move || state.target_choices.load_error.get().or_else(|| state.set_choices.load_error.get())) field_error=state.destination_error select_single=true /></div>
                    <div class="grid gap-4 md:grid-cols-3"><label class="block text-sm font-medium text-crono-text">"Timing"<select class=FIELD_CLASS prop:value=move || state.timing.get() on:change=move |event| state.timing.set(event_target_value(&event))><option value="cron">"Cron"</option><option value="once">"One shot"</option></select></label><Show when=move || state.timing.get() == "cron" fallback=move || view! { <label class="block text-sm font-medium text-crono-text md:col-span-2">"Execute at (RFC 3339)"<input class=FIELD_CLASS type="text" placeholder="2026-09-25T12:00:00Z" prop:value=move || state.execute_at.get() on:input=move |event| state.execute_at.set(event_target_value(&event))/><p class="mt-1 text-sm text-crono-failed" role="alert">{move || schedule_server_error(state.server_field, "execute_at").unwrap_or_default()}</p></label> }><label class="block text-sm font-medium text-crono-text">"Cron expression"<input class=FIELD_CLASS type="text" prop:value=move || state.cron_expression.get() on:input=move |event| state.cron_expression.set(event_target_value(&event))/><p class="mt-1 text-sm text-crono-failed" role="alert">{move || schedule_server_error(state.server_field, "cron_expression").unwrap_or_default()}</p></label><div><TimezoneSelect selected=state.timezone /><p class="mt-1 text-sm text-crono-failed" role="alert">{move || schedule_server_error(state.server_field, "timezone").unwrap_or_default()}</p></div></Show></div>
                    <JsonObjectInput id="schedule-inputs" label="Invocation inputs" value=state.inputs error=state.input_error />
                    <FormActions submit_label="Create Schedule" disabled=state.disabled on_cancel=state.reset />
                </form>

            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface"><header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 node_ref=heading tabindex="-1" class="font-semibold text-crono-text">"Schedules in Namespace"</h2></header>{move || state.schedules.map(|result| match result {
                Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Schedule."</p> }.into_any(),
                Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().cloned().map(|schedule| { let toggle = schedule.clone(); let destination = match &schedule.target { ExecutionTargetResource::Target { name, .. } => format!("Target · {name}"), ExecutionTargetResource::TargetSet { name, .. } => format!("Target Set · {name}") }; view! { <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6"><div><p class="font-medium text-crono-text">{format!("{}/{}", schedule.namespace, schedule.name)}</p><p class="text-sm text-crono-muted">{format!("{} → {} · next {}", schedule.job, destination, schedule.next_run_at.unwrap_or_else(|| "not scheduled".to_string()))}</p></div><button type="button" class=QUIET_ACTION_CLASS disabled=move || updating.get() on:click=move |_| { if updating.get_untracked() { return; } updating.set(true); state.feedback.set(None); spawn_local(async move { match api::update_schedule(toggle.id, &UpdateScheduleRequest { revision: toggle.revision, enabled: !toggle.enabled }).await { Ok(_) => { state.feedback.set(Some(ResourceFeedback::saved("Schedule updated.".to_string(), "Done"))); state.schedules.refetch(); }, Err(error) => { let mut feedback = ResourceFeedback::failed(error.message); feedback.continue_label = "Close"; state.feedback.set(Some(feedback)); }, } updating.set(false); }); }>{if schedule.enabled { "Disable" } else { "Enable" }}</button></li> } }).collect_view()}</ul> }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Schedules…"</p> }.into_any())}</section>
            <ResourceFeedbackModal id="schedule-save-result" resource="Schedule" plural="Schedules" feedback=state.feedback on_view=Callback::new(move |()| focus_heading(heading)) />
        </div>
    }
}

/// Keep server field guidance available after dismissing the result modal.
fn schedule_server_error(
    server_field: RwSignal<Option<(String, String)>>,
    field: &str,
) -> Option<String> {
    server_field
        .get()
        .filter(|(name, _)| name == field)
        .map(|(_, message)| message)
}

/// Combine authoritative server guidance with missing-selector validation.
fn choice_error(
    server_field: RwSignal<Option<(String, String)>>,
    selected: RwSignal<Option<uuid::Uuid>>,
    attempted: RwSignal<bool>,
    field: &'static str,
) -> Signal<Option<String>> {
    Signal::derive(move || {
        schedule_server_error(server_field, field).or_else(|| {
            (attempted.get() && selected.get().is_none()).then(|| {
                match field {
                    "namespace_id" => "Select a Namespace.",
                    "job_id" => "Select a Job.",
                    _ => "Select a Target or Target Set.",
                }
                .to_string()
            })
        })
    })
}
