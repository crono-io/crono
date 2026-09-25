//! Namespace-scoped Job form and execution-template preview.
//!
//! Jobs own the executor, executable, base argv templates, retry policy, and
//! least-specific input layer. Existing Namespaces and Queues remain UUID-
//! backed selections; preview destinations are transient and never persisted.

use super::super::resource_options;
use super::preview::{
    PreviewFields, job_preview_options, load_target_sets, load_targets, pretty_json, preview_text,
};
use crate::{
    api,
    components::{
        ArgumentListInput, FormActions, JsonObjectInput, PageHeader, ResourceNameInput,
        ResourceSelect, name_validation_message, parse_input_object, visible_name_validation,
    },
};
use crono_api::{CreateJobRequest, ExecutorKind, JobResource, UpdateJobRequest};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::{NavigateOptions, components::A, hooks::use_navigate};

const FIELD_CLASS: &str = "mt-1.5 w-full rounded-md border border-crono-border bg-white px-3 py-2.5 text-sm text-crono-text shadow-sm outline-none focus:border-crono-primary focus:ring-2 focus:ring-crono-primary-soft";

/// Render the shared create/edit form without owning browse-page state.
#[component]
pub(super) fn JobForm(#[prop(optional)] initial_job: Option<JobResource>) -> impl IntoView {
    let state = JobState::new(initial_job.as_ref());
    let (edit_id, namespace_name) =
        initial_job.map_or((None, None), |job| (Some(job.id), Some(job.namespace)));
    let is_edit = edit_id.is_some();
    let fields = state.fields();
    let errors = job_errors(&state);
    let JobErrors {
        name: name_error,
        inputs: input_error,
        arguments: argument_error,
        namespace: namespace_error,
        queue: queue_error,
        executable: executable_error,
        shell_command: shell_command_error,
    } = errors;
    let JobState {
        namespace_id,
        name,
        queue_id,
        executor,
        executable,
        shell_command,
        arguments,
        inputs,
        idempotent,
        dry_run,
        max_attempts,
        retry_initial,
        retry_max,
        retry_multiplier,
        retry_jitter,
        preview_target,
        feedback,
        namespace_choices,
        queue_choices,
        targets,
        target_sets,
        ..
    } = state;
    let disabled = job_form_disabled(&state, fields, errors);
    let submit = job_submit(&state, fields, edit_id);
    let navigate = use_navigate();
    let cancel = Callback::new(move |()| navigate("/jobs", NavigateOptions::default()));
    let preview_options = job_preview_options(targets, target_sets);
    let preview = state.preview(fields);

    view! {
        <div class="space-y-8">
            <PageHeader
                title=if is_edit { "Edit Job" } else { "Create Job" }
                description="Define the executable, argv templates, defaults, and retry policy copied into every durable Run."
            />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">{if is_edit { "Job settings" } else { "New Job" }}</h2>
                <form class="mt-5 space-y-5" on:submit=move |event| submit.run(event) novalidate>
                    {match namespace_name {
                        Some(namespace) => view! {
                            <div><p class="text-sm font-medium text-crono-text">"Namespace"</p><p class="mt-1.5 rounded-md border border-crono-border bg-zinc-50 px-3 py-2.5 text-sm text-crono-muted">{namespace}</p></div>
                        }.into_any(),
                        None => view! {
                            <div>
                                <ResourceSelect id="job-namespace" label="Namespace" placeholder="Search/select namespace…" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error field_error=namespace_error />
                                <Show when=move || !namespace_choices.loading.get() && namespace_choices.options.get().is_empty() && namespace_choices.load_error.get().is_none()>
                                    <p class="rounded-md bg-zinc-50 p-3 text-sm text-crono-muted">"No namespaces exist yet. "<A href="/namespaces" attr:class="font-medium text-crono-primary">"Create one before creating a Job."</A></p>
                                </Show>
                            </div>
                        }.into_any(),
                    }}
                    <div class="grid gap-4 md:grid-cols-2">
                        <ResourceNameInput id="job-name" label="Name" value=name error=name_error />
                        <ResourceSelect id="job-queue" label="Worker Queue" placeholder="Search/select Queue…" options=queue_choices.options selected=queue_id loading=queue_choices.loading load_error=queue_choices.load_error field_error=queue_error />
                    </div>
                    <div class="grid gap-4 md:grid-cols-2">
                        <label class="block text-sm font-medium text-crono-text">"Executor"<select class=FIELD_CLASS prop:value=move || match executor.get() { ExecutorKind::Noop => "noop", ExecutorKind::Process => "process", ExecutorKind::Shell => "shell" } on:change=move |event| { let selected = match event_target_value(&event).as_str() { "process" => ExecutorKind::Process, "shell" => ExecutorKind::Shell, _ => ExecutorKind::Noop }; if selected == ExecutorKind::Shell && executor.get_untracked() != ExecutorKind::Shell { executable.set("/bin/sh".to_string()); } executor.set(selected); }><option value="noop">"No-op"</option><option value="process">"Process · direct executable"</option><option value="shell">"Shell · script"</option></select></label>
                        <label class="block text-sm font-medium text-crono-text">{move || if executor.get() == ExecutorKind::Shell { "Shell interpreter" } else { "Executable" }}<input class=FIELD_CLASS type="text" placeholder=move || if executor.get() == ExecutorKind::Shell { "/bin/sh" } else { "/usr/bin/echo" } disabled=move || executor.get() == ExecutorKind::Noop prop:value=move || executable.get() on:input=move |event| executable.set(event_target_value(&event))/><p class="mt-1 text-sm text-crono-failed">{move || executable_error.get().unwrap_or_default()}</p></label>
                    </div>
                    <Show when=move || executor.get() == ExecutorKind::Shell>
                        <label class="block text-sm font-medium text-crono-text">"Shell script"<textarea class=FIELD_CLASS rows="4" placeholder="printf 'started\n'; /usr/bin/sleep 10; printf 'Hello, %s\n' \"$1\"" prop:value=move || shell_command.get() on:input=move |event| shell_command.set(event_target_value(&event))></textarea><p class="mt-1 text-xs text-crono-muted">"The script is literal. Put {{ name }} in Arguments and read it as $1; Crono will not inject input into shell source."</p><p class="mt-1 text-sm text-crono-failed">{move || shell_command_error.get().unwrap_or_default()}</p></label>
                    </Show>
                    <label class="flex items-start gap-3 rounded-lg border border-crono-border bg-zinc-50 p-4 text-sm text-crono-text"><input class="mt-0.5" type="checkbox" prop:checked=move || dry_run.get() on:change=move |event| dry_run.set(event_target_checked(&event))/><span><span class="font-medium">"Dry run"</span><span class="mt-1 block text-xs text-crono-muted">"Print the resolved command in Run output without executing it. Future Runs will be marked skipped, even when the worker was not started with --dry-run."</span></span></label>
                    <ArgumentListInput id="job-arguments" label="Arguments" values=arguments error=argument_error />
                    <Show when=move || executor.get() == ExecutorKind::Shell>
                        <p class="-mt-3 text-xs text-crono-muted">"Shell example: interpreter "<code>"/bin/sh"</code>", script "<code>"printf 'started\\n'; /usr/bin/sleep 10; printf 'Hello, %s\\n' \"$1\""</code>", one argument "<code>"{{ name }}"</code>", and default inputs "<code>r#"{"name":"world"}"#</code>". The first line appears before the sleep when you refresh output."</p>
                    </Show>
                    <Show when=move || executor.get() != ExecutorKind::Shell>
                        <p class="-mt-3 text-xs text-crono-muted">"Process example: executable "<code>"/usr/bin/echo"</code>", one argument "<code>"Hello, {{ name }}"</code>", and default inputs "<code>r#"{"name":"world"}"#</code>". Process does not parse shell syntax such as &&."</p>
                    </Show>
                    <JsonObjectInput id="job-inputs" label="Default inputs" value=inputs error=input_error />
                    <details class="rounded-lg border border-crono-border p-4"><summary class="cursor-pointer text-sm font-medium text-crono-text">"Retry and safety policy"</summary><div class="mt-4 grid gap-4 sm:grid-cols-2 lg:grid-cols-5"><NumberField label="Max attempts" value=max_attempts /><NumberField label="Initial seconds" value=retry_initial /><NumberField label="Maximum seconds" value=retry_max /><NumberField label="Multiplier" value=retry_multiplier /><NumberField label="Jitter" value=retry_jitter /></div><label class="mt-4 flex items-center gap-2 text-sm text-crono-text"><input type="checkbox" prop:checked=move || idempotent.get() on:change=move |event| idempotent.set(event_target_checked(&event))/><span>"Safe to retry after an ambiguous worker failure"</span></label></details>
                    <div class="rounded-lg border border-crono-border bg-zinc-50 p-4">
                        <h3 class="text-sm font-semibold text-crono-text">"Command and inputs preview"</h3>
                        <p class="mt-1 text-xs text-crono-muted">"Select a Target or Target Set to see the command and merged inputs prepared for each Target. This does not create or execute a Run. Inputs supplied later by a Schedule or manual Run are not included."</p>
                        <div class="mt-3"><ResourceSelect id="job-preview-target" label="Preview destination" placeholder="Select a Target or Target Set…" options=preview_options selected=preview_target loading=Signal::derive(move || namespace_id.get().is_some() && (targets.get().is_none() || target_sets.get().is_none())) load_error=Signal::derive(|| None) optional=true /></div>
                        <pre class="mt-3 overflow-auto whitespace-pre-wrap rounded-md bg-zinc-900 p-3 text-xs text-zinc-100">{move || preview.get()}</pre>
                    </div>
                    <FormActions submit_label="Save Job" disabled=disabled on_cancel=cancel />
                </form>
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || feedback.get().unwrap_or_default()}</p>
            </section>
        </div>
    }
}

/// Keep submit enablement aligned with the existing inline validation rules.
fn job_form_disabled(state: &JobState, fields: JobFields, errors: JobErrors) -> Signal<bool> {
    let state = *state;
    Signal::derive(move || {
        state.submitting.get()
            || state.namespace_id.get().is_none()
            || name_validation_message(&state.name.get(), true).is_some()
            || errors.inputs.get().is_some()
            || errors.arguments.get().is_some()
            || errors.executable.get().is_some()
            || errors.shell_command.get().is_some()
            || job_request(fields).is_err()
    })
}

#[derive(Clone, Copy)]
struct JobState {
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    name: RwSignal<String>,
    queue_id: RwSignal<Option<uuid::Uuid>>,
    executor: RwSignal<ExecutorKind>,
    executable: RwSignal<String>,
    shell_command: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    idempotent: RwSignal<bool>,
    dry_run: RwSignal<bool>,
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
    targets: LocalResource<api::ApiResult<Vec<crono_api::TargetResource>>>,
    target_sets: LocalResource<api::ApiResult<Vec<crono_api::TargetSetResource>>>,
}

impl JobState {
    /// Keep transient preview computation outside the form layout.
    fn preview(&self, fields: JobFields) -> Signal<String> {
        let preview_target = self.preview_target;
        let targets = self.targets;
        let target_sets = self.target_sets;
        Signal::derive(move || {
            preview_text(
                fields.preview_fields(),
                preview_target.get(),
                targets.get(),
                target_sets.get(),
            )
        })
    }

    /// Seed edit fields from the authorized API resource; create starts blank.
    fn new(initial: Option<&JobResource>) -> Self {
        let namespace_id = RwSignal::new(initial.map(|job| job.namespace_id));
        let queue_id = RwSignal::new(initial.map(|job| job.queue_id));
        let preview_target = RwSignal::new(None);
        let queue_choices = resource_options::queues(initial.map(|job| job.queue_id));
        let state = Self {
            namespace_id,
            name: RwSignal::new(initial.map_or_else(String::new, |job| job.name.clone())),
            queue_id,
            executor: RwSignal::new(initial.map_or(ExecutorKind::Noop, |job| job.executor)),
            executable: RwSignal::new(
                initial
                    .and_then(|job| job.executable.clone())
                    .unwrap_or_default(),
            ),
            shell_command: RwSignal::new(
                initial
                    .and_then(|job| job.shell_command.clone())
                    .unwrap_or_default(),
            ),
            arguments: RwSignal::new(initial.map_or_else(Vec::new, |job| job.arguments.clone())),
            inputs: RwSignal::new(
                initial.map_or_else(|| "{}".to_string(), |job| pretty_json(&job.inputs)),
            ),
            idempotent: RwSignal::new(initial.is_some_and(|job| job.idempotent)),
            dry_run: RwSignal::new(initial.is_some_and(|job| job.dry_run)),
            max_attempts: RwSignal::new(
                initial.map_or_else(|| "1".to_string(), |job| job.max_attempts.to_string()),
            ),
            retry_initial: RwSignal::new(initial.map_or_else(
                || "1".to_string(),
                |job| job.retry_initial_seconds.to_string(),
            )),
            retry_max: RwSignal::new(
                initial.map_or_else(|| "60".to_string(), |job| job.retry_max_seconds.to_string()),
            ),
            retry_multiplier: RwSignal::new(
                initial.map_or_else(|| "2.0".to_string(), |job| job.retry_multiplier.to_string()),
            ),
            retry_jitter: RwSignal::new(
                initial.map_or_else(|| "0.2".to_string(), |job| job.retry_jitter.to_string()),
            ),
            preview_target,
            attempted: RwSignal::new(false),
            submitting: RwSignal::new(false),
            server_field: RwSignal::new(None),
            feedback: RwSignal::new(None),
            namespace_choices: resource_options::namespaces(),
            queue_choices,
            targets: LocalResource::new(move || load_targets(namespace_id.get())),
            target_sets: LocalResource::new(move || load_target_sets(namespace_id.get())),
        };
        if initial.is_none() {
            initialize_default_queue(queue_id, queue_choices);
        }
        clear_preview_on_namespace_change(namespace_id, preview_target);
        state
    }

    const fn fields(self) -> JobFields {
        JobFields {
            name: self.name,
            queue_id: self.queue_id,
            executor: self.executor,
            executable: self.executable,
            shell_command: self.shell_command,
            arguments: self.arguments,
            inputs: self.inputs,
            idempotent: self.idempotent,
            dry_run: self.dry_run,
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

fn clear_preview_on_namespace_change(
    namespace_id: RwSignal<Option<uuid::Uuid>>,
    preview_target: RwSignal<Option<uuid::Uuid>>,
) {
    let previous = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous.get_untracked() != current {
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
    shell_command: Signal<Option<String>>,
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
                (state.executor.get() != ExecutorKind::Noop
                    && state.executable.get().trim().is_empty())
                .then(|| "An absolute executable or interpreter path is required.".to_string())
            })
        }),
        shell_command: Signal::derive(move || {
            server_error("shell_command").get().or_else(|| {
                (state.executor.get() == ExecutorKind::Shell
                    && (state.shell_command.get().trim().is_empty()
                        || state.shell_command.get().contains("{{")))
                .then(|| {
                    "Enter a literal script; put input templates in Arguments and use $1, $2, etc."
                        .to_string()
                })
            })
        }),
    }
}

fn job_reset(state: &JobState) -> Callback<()> {
    let state = *state;
    Callback::new(move |()| {
        state.name.set(String::new());
        state.executor.set(ExecutorKind::Noop);
        state.executable.set(String::new());
        state.shell_command.set(String::new());
        state.arguments.set(Vec::new());
        state.inputs.set("{}".to_string());
        state.idempotent.set(false);
        state.dry_run.set(false);
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
    edit_id: Option<uuid::Uuid>,
) -> Callback<leptos::ev::SubmitEvent> {
    let state = *state;
    let reset = job_reset(&state);
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
        state.submitting.set(true);
        spawn_local(async move {
            let result = match edit_id {
                Some(id) => api::update_job(id, &update_job_request(request)).await,
                None => api::create_job(namespace, &request).await,
            };
            match result {
                Ok(job) => {
                    if edit_id.is_none() {
                        reset.run(());
                    }
                    state
                        .feedback
                        .set(Some(format!("Saved {}.", job.qualified_name)));
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

#[derive(Clone, Copy)]
struct JobFields {
    name: RwSignal<String>,
    queue_id: RwSignal<Option<uuid::Uuid>>,
    executor: RwSignal<ExecutorKind>,
    executable: RwSignal<String>,
    shell_command: RwSignal<String>,
    arguments: RwSignal<Vec<String>>,
    inputs: RwSignal<String>,
    idempotent: RwSignal<bool>,
    dry_run: RwSignal<bool>,
    max_attempts: RwSignal<String>,
    retry_initial: RwSignal<String>,
    retry_max: RwSignal<String>,
    retry_multiplier: RwSignal<String>,
    retry_jitter: RwSignal<String>,
}

impl JobFields {
    /// Restrict the transient preview to execution-relevant form values.
    const fn preview_fields(self) -> PreviewFields {
        PreviewFields {
            executor: self.executor,
            executable: self.executable,
            shell_command: self.shell_command,
            arguments: self.arguments,
            inputs: self.inputs,
        }
    }
}

fn job_request(fields: JobFields) -> Result<CreateJobRequest, ()> {
    Ok(CreateJobRequest {
        name: fields.name.get(),
        queue_id: fields.queue_id.get().ok_or(())?,
        executor: fields.executor.get(),
        executable: (fields.executor.get() != ExecutorKind::Noop).then(|| fields.executable.get()),
        shell_command: (fields.executor.get() == ExecutorKind::Shell)
            .then(|| fields.shell_command.get()),
        arguments: fields.arguments.get(),
        inputs: parse_input_object(&fields.inputs.get()).map_err(|_| ())?,
        idempotent: fields.idempotent.get(),
        dry_run: fields.dry_run.get(),
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
        shell_command: value.shell_command,
        arguments: value.arguments,
        inputs: value.inputs,
        idempotent: value.idempotent,
        dry_run: value.dry_run,
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
