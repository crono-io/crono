//! Namespace-scoped Job authoring and execution-template preview.
//!
//! Jobs own the executor, executable, base argv templates, retry policy, and
//! least-specific input layer. Existing Namespaces and Queues remain UUID-
//! backed selections; preview destinations are transient and never persisted.

use super::resource_options;
use crate::{
    api,
    components::{
        ArgumentListInput, FormActions, JsonObjectInput, PageHeader, ResourceNameInput,
        ResourceOption, ResourceSelect, name_validation_message, parse_input_object,
        visible_name_validation,
    },
};
use crono_api::{CreateJobRequest, ExecutorKind, UpdateJobRequest};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;

const FIELD_CLASS: &str = "mt-1.5 w-full rounded-md border border-crono-border bg-white px-3 py-2.5 text-sm text-crono-text shadow-sm outline-none focus:border-crono-primary focus:ring-2 focus:ring-crono-primary-soft";

/// Create and edit process or no-op Jobs with a deterministic argv preview.
#[component]
pub fn JobsPage() -> impl IntoView {
    let state = JobState::new();
    let fields = state.fields();
    let errors = job_errors(&state);
    let reset = job_reset(&state);
    let JobErrors {
        name: name_error,
        inputs: input_error,
        arguments: argument_error,
        namespace: namespace_error,
        queue: queue_error,
        executable: executable_error,
    } = errors;
    let JobState {
        namespace_id,
        editing_id,
        name,
        queue_id,
        executor,
        executable,
        arguments,
        inputs,
        idempotent,
        max_attempts,
        retry_initial,
        retry_max,
        retry_multiplier,
        retry_jitter,
        preview_target,
        submitting,
        feedback,
        namespace_choices,
        queue_choices,
        jobs,
        targets,
        target_sets,
        ..
    } = state;
    let disabled = Signal::derive(move || {
        submitting.get()
            || namespace_id.get().is_none()
            || name_validation_message(&name.get(), true).is_some()
            || input_error.get().is_some()
            || argument_error.get().is_some()
            || executable_error.get().is_some()
            || job_request(fields).is_err()
    });
    let submit = job_submit(&state, fields, reset);
    let preview_options = job_preview_options(targets, target_sets);
    let preview = Signal::derive(move || {
        preview_text(
            fields,
            preview_target.get(),
            targets.get(),
            target_sets.get(),
        )
    });

    view! {
        <div class="space-y-8">
            <PageHeader title="Jobs" description="Jobs define the executable, argv templates, defaults, and retry policy copied into every durable Run." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">{move || if editing_id.get().is_some() { "Edit Job" } else { "Create Job" }}</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| submit.run(event) novalidate>
                    <ResourceSelect id="job-namespace" label="Namespace" placeholder="Search/select namespace…" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error field_error=namespace_error />
                    <Show when=move || !namespace_choices.loading.get() && namespace_choices.options.get().is_empty() && namespace_choices.load_error.get().is_none()>
                        <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No namespaces exist yet. "<A href="/namespaces" attr:class="font-medium text-crono-primary">"Create one before creating a Job."</A></p>
                    </Show>
                    <div class="grid gap-4 md:grid-cols-2">
                        <ResourceNameInput id="job-name" label="Name" value=name error=name_error />
                        <ResourceSelect id="job-queue" label="Worker Queue" placeholder="Search/select Queue…" options=queue_choices.options selected=queue_id loading=queue_choices.loading load_error=queue_choices.load_error field_error=queue_error />
                    </div>
                    <div class="grid gap-4 md:grid-cols-2">
                        <label class="block text-sm font-medium text-crono-text">"Executor"<select class=FIELD_CLASS prop:value=move || match executor.get() { ExecutorKind::Noop => "noop", ExecutorKind::Process => "process" } on:change=move |event| executor.set(if event_target_value(&event) == "process" { ExecutorKind::Process } else { ExecutorKind::Noop })><option value="noop">"No-op"</option><option value="process">"Process"</option></select></label>
                        <label class="block text-sm font-medium text-crono-text">"Executable"<input class=FIELD_CLASS type="text" placeholder="/usr/bin/curl" disabled=move || executor.get() == ExecutorKind::Noop prop:value=move || executable.get() on:input=move |event| executable.set(event_target_value(&event))/><p class="mt-1 text-sm text-crono-failed">{move || executable_error.get().unwrap_or_default()}</p></label>
                    </div>
                    <ArgumentListInput id="job-arguments" label="Arguments" values=arguments error=argument_error />
                    <JsonObjectInput id="job-inputs" label="Default inputs" value=inputs error=input_error />
                    <details class="rounded-lg border border-crono-border p-4"><summary class="cursor-pointer text-sm font-medium text-crono-text">"Retry and safety policy"</summary><div class="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-5"><NumberField label="Max attempts" value=max_attempts /><NumberField label="Initial seconds" value=retry_initial /><NumberField label="Maximum seconds" value=retry_max /><NumberField label="Multiplier" value=retry_multiplier /><NumberField label="Jitter" value=retry_jitter /></div><label class="mt-4 flex items-center gap-2 text-sm text-crono-text"><input type="checkbox" prop:checked=move || idempotent.get() on:change=move |event| idempotent.set(event_target_checked(&event))/><span>"Safe to retry after an ambiguous worker failure"</span></label></details>
                    <div class="rounded-lg border border-crono-border bg-zinc-50 p-4"><h3 class="text-sm font-semibold text-crono-text">"Template preview"</h3><div class="mt-3"><ResourceSelect id="job-preview-target" label="Preview destination" placeholder="Select a Target or Target Set…" options=preview_options selected=preview_target loading=Signal::derive(move || namespace_id.get().is_some() && (targets.get().is_none() || target_sets.get().is_none())) load_error=Signal::derive(|| None) optional=true /></div><pre class="mt-3 overflow-auto whitespace-pre-wrap rounded-md bg-zinc-900 p-3 text-xs text-zinc-100">{move || preview.get()}</pre></div>
                    <FormActions submit_label="Save Job" disabled=disabled on_cancel=reset />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || feedback.get().unwrap_or_default()}</p>
            </section>
            <ResourceList namespace_id=namespace_id jobs=jobs fields=fields editing_id=editing_id />
        </div>
    }
}

#[derive(Clone, Copy)]
struct JobState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    queue_id: RwSignal<Option<uuid::Uuid>>,
    executor: RwSignal<ExecutorKind>,
    executable: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    idempotent: RwSignal<bool>,
    max_attempts: RwSignal<String>,
    retry_initial: RwSignal<String>,
    retry_max: RwSignal<String>,
    retry_multiplier: RwSignal<String>,
    retry_jitter: RwSignal<String>,
    preview_target: RwSignal<Option<uuid::Uuid>>,
    attempted: RwSignal<bool>,
    submitting: RwSignal<bool>,
    server_field: RwSignal<Option<(String, String)>>,
    feedback: RwSignal<Option<String>>,
    namespace_choices: resource_options::ResourceOptions,
    queue_choices: resource_options::ResourceOptions,
    jobs: LocalResource<api::ApiResult<crono_api::Page<crono_api::JobResource>>>,
    targets: LocalResource<api::ApiResult<Vec<crono_api::TargetResource>>>,
    target_sets: LocalResource<api::ApiResult<Vec<crono_api::TargetSetResource>>>,
}

impl JobState {
    fn new() -> Self {
        let namespace_id = RwSignal::new(None);
        let queue_id = RwSignal::new(None);
        let editing_id = RwSignal::new(None);
        let preview_target = RwSignal::new(None);
        let queue_choices = resource_options::queues();
        let state = Self {
            namespace_id,
            editing_id,
            name: RwSignal::new(String::new()),
            queue_id,
            executor: RwSignal::new(ExecutorKind::Noop),
            executable: RwSignal::new(String::new()),
            arguments: RwSignal::new(Vec::new()),
            inputs: RwSignal::new("{}".to_string()),
            idempotent: RwSignal::new(false),
            max_attempts: RwSignal::new("1".to_string()),
            retry_initial: RwSignal::new("1".to_string()),
            retry_max: RwSignal::new("60".to_string()),
            retry_multiplier: RwSignal::new("2.0".to_string()),
            retry_jitter: RwSignal::new("0.2".to_string()),
            preview_target,
            attempted: RwSignal::new(false),
            submitting: RwSignal::new(false),
            server_field: RwSignal::new(None),
            feedback: RwSignal::new(None),
            namespace_choices: resource_options::namespaces(),
            queue_choices,
            jobs: LocalResource::new(move || load_jobs(namespace_id.get())),
            targets: LocalResource::new(move || load_targets(namespace_id.get())),
            target_sets: LocalResource::new(move || load_target_sets(namespace_id.get())),
        };
        initialize_default_queue(queue_id, queue_choices);
        clear_job_selections_on_namespace_change(namespace_id, editing_id, preview_target);
        state
    }

    const fn fields(self) -> JobFields {
        JobFields {
            name: self.name,
            queue_id: self.queue_id,
            executor: self.executor,
            executable: self.executable,
            arguments: self.arguments,
            inputs: self.inputs,
            idempotent: self.idempotent,
            max_attempts: self.max_attempts,
            retry_initial: self.retry_initial,
            retry_max: self.retry_max,
            retry_multiplier: self.retry_multiplier,
            retry_jitter: self.retry_jitter,
        }
    }
}

fn initialize_default_queue(
    queue_id: RwSignal<Option<uuid::Uuid>>,
    choices: resource_options::ResourceOptions,
) {
    let initialized = RwSignal::new(false);
    Effect::new(move |_| {
        if !initialized.get() && !choices.loading.get() {
            initialized.set(true);
            let default = choices
                .options
                .get()
                .into_iter()
                .find(|item| item.label == "default");
            if let Some(queue) = default {
                queue_id.set(Some(queue.id));
            }
        }
    });
}

fn clear_job_selections_on_namespace_change(
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    editing_id: RwSignal<Option<uuid::Uuid>>,
    preview_target: RwSignal<Option<uuid::Uuid>>,
) {
    let previous = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous.get_untracked() != current {
            editing_id.set(None);
            preview_target.set(None);
            previous.set(current);
        }
    });
}

#[derive(Clone, Copy)]
struct JobErrors {
    name: Signal<Option<String>>,
    inputs: Signal<Option<String>>,
    arguments: Signal<Option<String>>,
    namespace: Signal<Option<String>>,
    queue: Signal<Option<String>>,
    executable: Signal<Option<String>>,
}

fn job_errors(state: &JobState) -> JobErrors {
    let state = *state;
    let server_error = move |field: &'static str| {
        Signal::derive(move || {
            state
                .server_field
                .get()
                .filter(|(name, _)| name == field)
                .map(|(_, message)| message)
        })
    };
    JobErrors {
        name: Signal::derive(move || {
            server_error("name")
                .get()
                .or_else(|| visible_name_validation(&state.name.get(), state.attempted.get()))
        }),
        inputs: Signal::derive(move || {
            server_error("inputs")
                .get()
                .or_else(|| parse_input_object(&state.inputs.get()).err())
        }),
        arguments: Signal::derive(move || {
            server_error("arguments").get().or_else(|| {
                crono_execution::validate_argument_templates(&state.arguments.get())
                    .err()
                    .map(|error| error.to_string())
            })
        }),
        namespace: Signal::derive(move || {
            server_error("namespace_id").get().or_else(|| {
                (state.attempted.get() && state.namespace_id.get().is_none())
                    .then(|| "Select a Namespace.".to_string())
            })
        }),
        queue: Signal::derive(move || {
            server_error("queue_id").get().or_else(|| {
                (state.attempted.get() && state.queue_id.get().is_none())
                    .then(|| "Select a Queue.".to_string())
            })
        }),
        executable: Signal::derive(move || {
            server_error("executable").get().or_else(|| {
                (state.executor.get() == ExecutorKind::Process
                    && state.executable.get().trim().is_empty())
                .then(|| "An absolute executable path is required for process Jobs.".to_string())
            })
        }),
    }
}

fn job_reset(state: &JobState) -> Callback<()> {
    let state = *state;
    Callback::new(move |()| {
        state.editing_id.set(None);
        state.name.set(String::new());
        state.executor.set(ExecutorKind::Noop);
        state.executable.set(String::new());
        state.arguments.set(Vec::new());
        state.inputs.set("{}".to_string());
        state.idempotent.set(false);
        state.max_attempts.set("1".to_string());
        state.retry_initial.set("1".to_string());
        state.retry_max.set("60".to_string());
        state.retry_multiplier.set("2.0".to_string());
        state.retry_jitter.set("0.2".to_string());
        state.attempted.set(false);
        state.server_field.set(None);
    })
}

fn job_submit(
    state: &JobState,
    fields: JobFields,
    reset: Callback<()>,
) -> Callback<leptos::ev::SubmitEvent> {
    let state = *state;
    Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        state.attempted.set(true);
        state.server_field.set(None);
        state.feedback.set(None);
        let Some(namespace) = state.namespace_id.get_untracked() else {
            return;
        };
        let Ok(request) = job_request(fields) else {
            return;
        };
        let current_edit = state.editing_id.get_untracked();
        state.submitting.set(true);
        spawn_local(async move {
            let result = match current_edit {
                Some(id) => api::update_job(id, &update_job_request(request)).await,
                None => api::create_job(namespace, &request).await,
            };
            match result {
                Ok(job) => {
                    reset.run(());
                    state
                        .feedback
                        .set(Some(format!("Saved {}.", job.qualified_name)));
                    state.jobs.refetch();
                }
                Err(error) => match error.field {
                    Some(field) => state.server_field.set(Some((field, error.message))),
                    None => state.feedback.set(Some(error.message)),
                },
            }
            state.submitting.set(false);
        });
    })
}

fn job_preview_options(
    targets: LocalResource<api::ApiResult<Vec<crono_api::TargetResource>>>,
    sets: LocalResource<api::ApiResult<Vec<crono_api::TargetSetResource>>>,
) -> Signal<Vec<ResourceOption>> {
    Signal::derive(move || {
        let mut options = targets
            .get()
            .and_then(Result::ok)
            .unwrap_or_default()
            .into_iter()
            .map(|target| ResourceOption {
                id: target.id,
                label: format!("Target · {}", target.name),
            })
            .collect::<Vec<_>>();
        options.extend(
            sets.get()
                .and_then(Result::ok)
                .unwrap_or_default()
                .into_iter()
                .map(|set| ResourceOption {
                    id: set.id,
                    label: format!("Target Set · {}", set.name),
                }),
        );
        options
    })
}

#[derive(Clone, Copy)]
struct JobFields {
    name: RwSignal<String>,
    queue_id: RwSignal<Option<uuid::Uuid>>,
    executor: RwSignal<ExecutorKind>,
    executable: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    idempotent: RwSignal<bool>,
    max_attempts: RwSignal<String>,
    retry_initial: RwSignal<String>,
    retry_max: RwSignal<String>,
    retry_multiplier: RwSignal<String>,
    retry_jitter: RwSignal<String>,
}

fn job_request(fields: JobFields) -> Result<CreateJobRequest, ()> {
    Ok(CreateJobRequest {
        name: fields.name.get(),
        queue_id: fields.queue_id.get().ok_or(())?,
        executor: fields.executor.get(),
        executable: (fields.executor.get() == ExecutorKind::Process)
            .then(|| fields.executable.get()),
        arguments: fields.arguments.get(),
        inputs: parse_input_object(&fields.inputs.get()).map_err(|_| ())?,
        idempotent: fields.idempotent.get(),
        max_attempts: fields.max_attempts.get().parse().map_err(|_| ())?,
        retry_initial_seconds: fields.retry_initial.get().parse().map_err(|_| ())?,
        retry_max_seconds: fields.retry_max.get().parse().map_err(|_| ())?,
        retry_multiplier: fields.retry_multiplier.get().parse().map_err(|_| ())?,
        retry_jitter: fields.retry_jitter.get().parse().map_err(|_| ())?,
    })
}

fn update_job_request(value: CreateJobRequest) -> UpdateJobRequest {
    UpdateJobRequest {
        name: value.name,
        queue_id: value.queue_id,
        executor: value.executor,
        executable: value.executable,
        arguments: value.arguments,
        inputs: value.inputs,
        idempotent: value.idempotent,
        max_attempts: value.max_attempts,
        retry_initial_seconds: value.retry_initial_seconds,
        retry_max_seconds: value.retry_max_seconds,
        retry_multiplier: value.retry_multiplier,
        retry_jitter: value.retry_jitter,
    }
}

#[component]
fn NumberField(label: &'static str, value: RwSignal<String>) -> impl IntoView {
    view! { <label class="block text-xs font-medium text-crono-muted">{label}<input class=FIELD_CLASS type="number" min="0" step="any" prop:value=move || value.get() on:input=move |event| value.set(event_target_value(&event))/></label> }
}

#[component]
fn ResourceList(
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    jobs: LocalResource<api::ApiResult<crono_api::Page<crono_api::JobResource>>>,
    fields: JobFields,
    editing_id: RwSignal<Option<uuid::Uuid>>,
) -> impl IntoView {
    view! {
        <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
            <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Jobs in Namespace"</h2></header>
            {move || jobs.map(|result| match result {
                Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Select a Namespace or create its first Job."</p> }.into_any(),
                Ok(page) => view! {
                    <ul class="divide-y divide-crono-border">
                        {page.items.iter().cloned().map(|job| {
                            let edit_job = job.clone();
                            view! {
                                <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6">
                                    <div>
                                        <p class="font-medium text-crono-text">{job.qualified_name}</p>
                                        <p class="text-sm text-crono-muted">{format!("{:?} · {} · {} argv items", job.executor, job.queue, job.arguments.len())}</p>
                                    </div>
                                    <button
                                        type="button"
                                        class="text-sm font-medium text-crono-primary"
                                        on:click=move |_| {
                                            namespace_id.set(Some(edit_job.namespace_id));
                                            editing_id.set(Some(edit_job.id));
                                            fields.name.set(edit_job.name.clone());
                                            fields.queue_id.set(Some(edit_job.queue_id));
                                            fields.executor.set(edit_job.executor);
                                            fields.executable.set(edit_job.executable.clone().unwrap_or_default());
                                            fields.arguments.set(edit_job.arguments.clone());
                                            fields.inputs.set(pretty_json(&edit_job.inputs));
                                            fields.idempotent.set(edit_job.idempotent);
                                            fields.max_attempts.set(edit_job.max_attempts.to_string());
                                            fields.retry_initial.set(edit_job.retry_initial_seconds.to_string());
                                            fields.retry_max.set(edit_job.retry_max_seconds.to_string());
                                            fields.retry_multiplier.set(edit_job.retry_multiplier.to_string());
                                            fields.retry_jitter.set(edit_job.retry_jitter.to_string());
                                        }
                                    >
                                        "Edit"
                                    </button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Jobs…"</p> }.into_any())}
        </section>
    }
}

fn preview_text(
    fields: JobFields,
    selected: Option<uuid::Uuid>,
    targets: Option<api::ApiResult<Vec<crono_api::TargetResource>>>,
    sets: Option<api::ApiResult<Vec<crono_api::TargetSetResource>>>,
) -> String {
    let Some(selected) = selected else {
        return "Select a preview destination.".to_string();
    };
    let Ok(job_inputs) = parse_input_object(&fields.inputs.get()) else {
        return "Fix the Job inputs to preview.".to_string();
    };
    let targets = targets.and_then(Result::ok).unwrap_or_default();
    let sets = sets.and_then(Result::ok).unwrap_or_default();
    let executable = if fields.executor.get() == ExecutorKind::Noop {
        "noop".to_string()
    } else {
        fields.executable.get()
    };
    let selected_targets = if let Some(target) = targets.iter().find(|target| target.id == selected)
    {
        vec![(target, None)]
    } else if let Some(set) = sets.iter().find(|set| set.id == selected) {
        set.targets
            .iter()
            .filter_map(|member| {
                targets
                    .iter()
                    .find(|target| target.id == member.id)
                    .map(|target| (target, Some(&set.inputs)))
            })
            .collect()
    } else {
        return "The selected preview destination is no longer available.".to_string();
    };
    selected_targets
        .into_iter()
        .map(|(target, set_inputs)| {
            let empty = serde_json::json!({});
            let merged = crono_execution::merge_inputs(&[
                &job_inputs,
                set_inputs.unwrap_or(&empty),
                &target.inputs,
            ])
            .and_then(|inputs| {
                let mut argv = fields.arguments.get();
                argv.extend(target.arguments.clone());
                crono_execution::render_arguments(&argv, &inputs).map(|argv| (inputs, argv))
            });
            match merged {
                Ok((merged_inputs, argv)) => format!(
                    "{}\n$ {} {}\ninputs: {}",
                    target.name,
                    executable,
                    argv.iter()
                        .map(|item| format!("{item:?}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    pretty_json(&merged_inputs)
                ),
                Err(error) => format!("{}\nPreview error: {error}", target.name),
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string())
}

async fn load_jobs(
    namespace: Option<uuid::Uuid>,
) -> api::ApiResult<crono_api::Page<crono_api::JobResource>> {
    match namespace {
        Some(id) => api::list_jobs(id).await,
        None => Ok(crono_api::Page {
            items: Vec::new(),
            next_cursor: None,
        }),
    }
}

async fn load_targets(
    namespace: Option<uuid::Uuid>,
) -> api::ApiResult<Vec<crono_api::TargetResource>> {
    match namespace {
        Some(id) => api::all_targets(id).await,
        None => Ok(Vec::new()),
    }
}

async fn load_target_sets(
    namespace: Option<uuid::Uuid>,
) -> api::ApiResult<Vec<crono_api::TargetSetResource>> {
    match namespace {
        Some(id) => api::all_target_sets(id).await,
        None => Ok(Vec::new()),
    }
}
