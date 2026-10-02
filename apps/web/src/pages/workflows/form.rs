//! Structured Workflow authoring with a read-only live dependency preview.
//!
//! Jobs are selected by Namespace and referenced by UUID. Row identities keep
//! dependencies stable while names change. Save sends the current revision and
//! leaves the complete draft intact on server errors; history is never edited.
//! Removal confirmations explain incident dependencies before changing the draft.

use super::{
    ApiFailure, PANEL_CLASS, condition_label,
    editor::{EdgeRow, Editor, JobRow},
    graph::WorkflowGraph,
    invalid_url, route_id,
};
use crate::{
    api,
    components::{
        FormActions, Icon, Modal, PageHeader, QUIET_ACTION_CLASS, ResourceFeedback,
        ResourceFeedbackModal, ResourceNameInput, ResourceOption, ResourceSelect, focus_heading,
        visible_name_validation,
    },
    components::{
        forms::{FIELD_CLASS, READ_ONLY_FIELD_CLASS},
        resource_dialogs::DELETE_ACTION_CLASS,
    },
    navigation::{MaterialSymbol, workflow_details_path},
    pages::resource_options,
};
use crono_api::{DependencyCondition, UpdateWorkflowRequest, WorkflowResource};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::{NavigateOptions, components::A, hooks::use_navigate};

/// Author a new definition without creating or copying the referenced Jobs.
#[component]
pub fn CreateWorkflowPage() -> impl IntoView {
    view! { <WorkflowForm /> }
}

/// Restore the current graph and revision from an authorized deep link.
#[component]
pub fn EditWorkflowPage() -> impl IntoView {
    let id = route_id("workflow_id");
    let workflow = LocalResource::new(move || {
        let id = id.get();
        async move {
            match id {
                Some(id) => api::get_workflow(id).await,
                None => Err(invalid_url()),
            }
        }
    });
    view! { {move || workflow.map(|result| match result {
        Ok(workflow) => view! { <WorkflowForm initial=workflow.clone() /> }.into_any(),
        Err(error) => view! { <ApiFailure message=error.message.clone() on_retry=Callback::new(move |()| workflow.refetch()) /> }.into_any(),
    }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Workflow…"</p> }.into_any())} }
}

/// Keep creation/editing consistent, with server validation adjacent to the editor.
#[component]
fn WorkflowForm(#[prop(optional)] initial: Option<WorkflowResource>) -> impl IntoView {
    let editor = Editor::new(initial.as_ref());
    let namespaces = resource_options::namespaces();
    let jobs = job_options(editor);
    let previous_namespace = RwSignal::new(editor.namespace.get_untracked());
    Effect::new(move |_| {
        let current = editor.namespace.get();
        if previous_namespace.get_untracked() != current {
            editor.clear_jobs();
            previous_namespace.set(current);
        }
    });
    let errors = Signal::derive(move || {
        editor.error.get().or_else(|| {
            editor
                .attempted
                .get()
                .then(|| editor.request().err())
                .flatten()
        })
    });
    let dirty = Signal::derive(move || {
        editor.edit_id.is_some() && editor.request().ok() != editor.saved.get()
    });
    let navigate = use_navigate();
    let cancel_nav = navigate.clone();
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if editor.busy.get_untracked() {
            return;
        }
        editor.attempted.set(true);
        editor.error.set(None);
        let Ok(request) = untrack(move || editor.request()) else {
            return;
        };
        let Some(namespace) = editor.namespace.get_untracked() else {
            return;
        };
        editor.busy.set(true);
        spawn_local(save(editor, namespace, request));
    };
    view! {
        <div class="space-y-6">
            <PageHeader title=if editor.edit_id.is_some() { "Edit Workflow" } else { "Create Workflow" } description="Connect existing Jobs with dependencies: on success, on failure, or always.">
                <A href="/workflows" attr:class=QUIET_ACTION_CLASS>"All Workflows"</A>
                {editor.edit_id.map(|_| view! { <ReloadLatest editor on_jobs_reload=jobs.reload /> })}
            </PageHeader>
            <Show when=move || dirty.get()><p class="text-sm text-crono-muted" role="status">"Unsaved changes"</p></Show>
            <form class="space-y-6" on:submit=submit novalidate aria-busy=move || editor.busy.get().to_string()>
                <fieldset disabled=move || editor.busy.get() class="space-y-6">
                    <Metadata editor namespaces />
                    <JobEditor editor jobs />
                    <DependenciesEditor editor />
                </fieldset>
                <Show when=move || errors.get().is_some()><p class="rounded-lg bg-red-50 p-4 text-sm text-crono-failed" role="alert">{move || errors.get().unwrap_or_default()}</p></Show>
                <section class=PANEL_CLASS><h2 class="font-semibold text-crono-text">"Workflow preview"</h2><WorkflowGraph nodes=Signal::derive(move || editor.graph_nodes(&jobs.options.get())) edges=Signal::derive(move || editor.graph_edges()) /></section>
                <p class="text-sm text-crono-muted">"Changes affect future workflow runs only. Existing workflow run history is unchanged."</p>
                <FormActions submit_label="Save Workflow" disabled=Signal::derive(move || editor.busy.get()) on_cancel=Callback::new(move |()| { if !editor.busy.get_untracked() { cancel_nav("/workflows", NavigateOptions::default()); } }) />
            </form>
            <ResourceFeedbackModal id="workflow-save-feedback" resource="Workflow" plural="Workflow" feedback=editor.feedback on_view=Callback::new(move |()| { if let Some(id) = editor.saved_id.get_untracked() { navigate(&workflow_details_path(id), NavigateOptions::default()); } }) />
        </div>
    }
}

/// Retain loaded references when the Job catalog is stale or separately denied.
/// UUID fallback labels expose only references already present in the authorized draft.
fn job_options(editor: Editor) -> resource_options::ResourceOptions {
    let jobs = resource_options::jobs(editor.namespace);
    resource_options::ResourceOptions {
        options: Signal::derive(move || {
            let mut options = jobs.options.get();
            for row in editor.nodes.get() {
                if let Some(id) = row.job.get()
                    && !options.iter().any(|option| option.id == id)
                {
                    options.push(ResourceOption {
                        id,
                        label: format!("Job {id}"),
                    });
                }
            }
            options
        }),
        ..jobs
    }
}

/// Save only the frozen draft; a conflict never updates its optimistic revision.
async fn save(editor: Editor, namespace: uuid::Uuid, request: crono_api::CreateWorkflowRequest) {
    if editor.busy.is_disposed() {
        return;
    }
    let result = match editor.edit_id {
        Some(id) => {
            api::update_workflow(
                id,
                &UpdateWorkflowRequest {
                    revision: editor.revision.get_untracked(),
                    name: request.name.clone(),
                    description: request.description.clone(),
                    nodes: request.nodes.clone(),
                    edges: request.edges.clone(),
                },
            )
            .await
        }
        None => api::create_workflow(namespace, &request).await,
    };
    if editor.busy.is_disposed() {
        return;
    }
    match result {
        Ok(workflow) => {
            editor.saved_id.set(Some(workflow.id));
            editor.revision.set(workflow.revision);
            editor.saved.set(Some(request));
            editor.feedback.set(Some(ResourceFeedback::saved(
                format!("Saved Workflow {}.", workflow.name),
                if editor.edit_id.is_some() {
                    "Keep editing"
                } else {
                    "Create another"
                },
            )));
            if editor.edit_id.is_none() {
                editor.name.set(String::new());
                editor.description.set(String::new());
                editor.nodes.set(vec![JobRow::empty()]);
                editor.edges.set(Vec::new());
                editor.attempted.set(false);
            }
        }
        Err(error) => {
            let message = if editor.edit_id.is_some() && error.code == "already_exists" {
                format!(
                    "{} The name may be in use or the Workflow may have changed. Choose another name, or use Reload latest to review the current revision.",
                    error.message
                )
            } else {
                error.message
            };
            editor.error.set(Some(message.clone()));
            editor.feedback.set(Some(ResourceFeedback::failed(message)));
        }
    }
    editor.busy.set(false);
}

/// Namespace identity stays fixed for updates, matching the API contract.
#[component]
fn Metadata(editor: Editor, namespaces: resource_options::ResourceOptions) -> impl IntoView {
    view! {
        <section class=PANEL_CLASS>
            <div class="grid gap-4 md:grid-cols-2">
                <ResourceNameInput id="workflow-name" label="Name" value=editor.name error=Signal::derive(move || visible_name_validation(&editor.name.get(), editor.attempted.get())) />
                {if editor.edit_id.is_some() { view! { <div><p class="text-sm font-medium text-crono-text">"Namespace"</p><p class=READ_ONLY_FIELD_CLASS>{editor.namespace_label.get_value()}</p></div> }.into_any() } else { view! { <ResourceSelect id="workflow-namespace" label="Namespace" placeholder="Select a Namespace…" selected=editor.namespace options=namespaces.options loading=namespaces.loading load_error=namespaces.load_error select_single=true /> }.into_any() }}
            </div>
            <div><label for="workflow-description" class="block text-sm font-medium text-crono-text">"Description"</label><textarea id="workflow-description" maxlength="500" class=FIELD_CLASS prop:value=move || editor.description.get() on:input=move |event| editor.description.set(event_target_value(&event)) /></div>
        </section>
    }
}

/// Each row remains keyed by identity, so typing does not recreate the control or lose focus.
#[component]
fn JobEditor(editor: Editor, jobs: resource_options::ResourceOptions) -> impl IntoView {
    let heading = NodeRef::<leptos::html::H2>::new();
    view! {
        <section class=PANEL_CLASS>
            <h2 node_ref=heading tabindex="-1" class="font-semibold text-crono-text">"Jobs"</h2>
            <p class="text-sm text-crono-muted">"Each node references an existing Job in this Namespace. Names identify Jobs in the dependencies below."</p>
            <For each=move || editor.nodes.get() key=|row| row.key children=move |row| view! { <JobEditorRow row editor jobs on_removed=Callback::new(move |()| focus_heading(heading)) /> } />
            <button type="button" class=QUIET_ACTION_CLASS disabled=move || { editor.nodes.get().len() >= 64 } on:click=move |_| editor.nodes.update(|rows| rows.push(JobRow::empty()))>"+ Add Job"</button>
        </section>
    }
}

/// Derive an initially empty node name once a Job is explicitly chosen.
#[component]
fn JobEditorRow(
    row: JobRow,
    editor: Editor,
    jobs: resource_options::ResourceOptions,
    on_removed: Callback<()>,
) -> impl IntoView {
    let previous = RwSignal::new(row.job.get_untracked());
    Effect::new(move |_| {
        let current = row.job.get();
        if previous.get_untracked() != current {
            if row.name.get_untracked().is_empty()
                && let Some(job) = jobs
                    .options
                    .get_untracked()
                    .iter()
                    .find(|job| Some(job.id) == current)
            {
                row.name.set(unique_node_name(editor, &job.label));
            }
            previous.set(current);
        }
    });
    let open = RwSignal::new(false);
    let incident = Signal::derive(move || {
        editor
            .edges
            .get()
            .iter()
            .filter(|edge| edge.from.get() == Some(row.key) || edge.to.get() == Some(row.key))
            .count()
    });
    let remove = move || {
        open.set(false);
        editor.remove_node(row.key);
        on_removed.run(());
    };
    view! {
        <div class="grid items-start gap-3 rounded-lg border border-crono-border p-3 md:grid-cols-[minmax(0,1fr)_minmax(0,2fr)_auto]">
            <ResourceNameInput id=format!("workflow-node-name-{}", row.key) label="Node name" value=row.name error=Signal::derive(move || visible_name_validation(&row.name.get(), editor.attempted.get())) />
            <ResourceSelect id=format!("workflow-node-job-{}", row.key) label="Job" placeholder="Choose an existing Job…" options=jobs.options selected=row.job loading=jobs.loading load_error=jobs.load_error />
            <button type="button" class=format!("min-h-11 md:mt-[1.625rem] {DELETE_ACTION_CLASS}") aria-label=move || format!("Remove Job {}", row.name.get()) on:click=move |_| { if incident.get_untracked() == 0 { remove(); } else { open.set(true); } }><Icon symbol=MaterialSymbol::Delete class="text-lg" />"Remove"</button>
            <Modal id=format!("workflow-remove-node-{}", row.key) open title=Signal::derive(|| "Remove Job from Workflow?".to_string()) busy=Signal::derive(move || editor.busy.get())>
                <p class="text-sm text-crono-muted">{move || format!("Removing {} also removes {} connected dependencies from this draft. Nothing is saved until you save the Workflow.", row.name.get(), incident.get())}</p>
                <div class="flex justify-end gap-3"><button type="button" class=QUIET_ACTION_CLASS autofocus on:click=move |_| open.set(false)>"Cancel"</button><button type="button" class=DELETE_ACTION_CLASS on:click=move |_| remove()>"Remove Job"</button></div>
            </Modal>
        </div>
    }
}

/// Derived names stay canonical and unique without changing names already entered.
fn unique_node_name(editor: Editor, base: &str) -> String {
    let existing: Vec<String> = editor
        .nodes
        .get_untracked()
        .iter()
        .map(|row| row.name.get_untracked())
        .collect();
    if !existing.iter().any(|name| name == base) {
        return base.to_string();
    }
    for suffix in 2..=65 {
        let suffix = format!("-{suffix}");
        let prefix = base
            .get(..63_usize.saturating_sub(suffix.len()))
            .unwrap_or(base)
            .trim_end_matches('-');
        let candidate = format!("{prefix}{suffix}");
        if !existing.contains(&candidate) {
            return candidate;
        }
    }
    String::new()
}

/// Dependencies use typed endpoints, never arbitrary condition expressions.
#[component]
fn DependenciesEditor(editor: Editor) -> impl IntoView {
    let choices = Signal::derive(move || {
        editor
            .nodes
            .get()
            .iter()
            .map(|row| ResourceOption {
                id: row.key,
                label: row.name.get(),
            })
            .collect::<Vec<_>>()
    });
    view! {
        <section class=PANEL_CLASS><h2 class="font-semibold text-crono-text">"Dependencies"</h2>
            <p class="text-sm text-crono-muted">"Choose which predecessor result allows the next Job to start. All incoming dependencies must match. Jobs with no incoming dependencies start together."</p>
            <For each=move || editor.edges.get() key=|row| row.key children=move |row| view! { <DependencyRow row editor choices /> } />
            <button type="button" class=QUIET_ACTION_CLASS disabled=move || { editor.nodes.get().len() < 2 || editor.edges.get().len() >= 256 } on:click=move |_| editor.edges.update(|rows| rows.push(EdgeRow::empty()))>"+ Add Dependency"</button>
        </section>
    }
}

/// The To selector excludes the predecessor; submission also rejects duplicate pairs.
#[component]
fn DependencyRow(
    row: EdgeRow,
    editor: Editor,
    choices: Signal<Vec<ResourceOption>>,
) -> impl IntoView {
    let destinations = Signal::derive(move || {
        choices
            .get()
            .into_iter()
            .filter(|choice| Some(choice.id) != row.from.get())
            .collect::<Vec<_>>()
    });
    view! {
        <div class="grid items-start gap-3 rounded-lg border border-crono-border p-3 md:grid-cols-[minmax(0,1fr)_9rem_minmax(0,1fr)_auto]">
            <ResourceSelect id=format!("workflow-edge-from-{}", row.key) label="From Job" placeholder="Choose predecessor…" options=choices selected=row.from loading=Signal::derive(|| false) load_error=Signal::derive(|| None) />
            <div><label for=format!("workflow-edge-condition-{}", row.key) class="block text-sm font-medium text-crono-text">"Condition"</label><select id=format!("workflow-edge-condition-{}", row.key) class=FIELD_CLASS prop:value=move || match row.condition.get() { DependencyCondition::Success => "success", DependencyCondition::Failure => "failure", DependencyCondition::Always => "always" } on:change=move |event| { match event_target_value(&event).as_str() { "success" => row.condition.set(DependencyCondition::Success), "failure" => row.condition.set(DependencyCondition::Failure), "always" => row.condition.set(DependencyCondition::Always), _ => {} } }>
                <option value="success">{condition_label(DependencyCondition::Success)}</option><option value="failure">{condition_label(DependencyCondition::Failure)}</option><option value="always">{condition_label(DependencyCondition::Always)}</option>
            </select></div>
            <ResourceSelect id=format!("workflow-edge-to-{}", row.key) label="To Job" placeholder="Choose dependent Job…" options=destinations selected=row.to loading=Signal::derive(|| false) load_error=Signal::derive(|| None) />
            <button type="button" class=format!("min-h-11 md:mt-[1.625rem] {DELETE_ACTION_CLASS}") aria-label="Remove dependency" on:click=move |_| editor.edges.update(|rows| rows.retain(|edge| edge.key != row.key))><Icon symbol=MaterialSymbol::Delete class="text-lg" />"Remove"</button>
        </div>
    }
}

/// Loading a newer revision requires explicit draft disposal, never silent merging.
#[component]
fn ReloadLatest(editor: Editor, on_jobs_reload: Callback<()>) -> impl IntoView {
    let open = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let owner = Owner::current();
    let reload = move |_| {
        if editor.busy.get_untracked() {
            return;
        }
        let Some(id) = editor.edit_id else {
            return;
        };
        let Some(owner) = owner.clone() else {
            error.set(Some(
                "Reload is unavailable. Your draft is unchanged.".to_string(),
            ));
            return;
        };
        editor.busy.set(true);
        error.set(None);
        on_jobs_reload.run(());
        spawn_local(async move {
            let result = api::get_workflow(id).await;
            if editor.busy.is_disposed() {
                return;
            }
            match result {
                Ok(workflow) => {
                    owner.with(|| editor.replace(&workflow));
                    open.set(false);
                }
                Err(failure) => error.set(Some(format!(
                    "{} Your draft is unchanged; retry or keep editing.",
                    failure.message
                ))),
            }
            editor.busy.set(false);
        });
    };
    view! {
        <button type="button" class=QUIET_ACTION_CLASS disabled=move || editor.busy.get() on:click=move |_| { error.set(None); open.set(true); }>"Reload latest"</button>
        <Modal id="workflow-reload-latest" open busy=Signal::derive(move || editor.busy.get()) title=Signal::derive(|| "Reload latest Workflow?".to_string())>
            <p class="text-sm text-crono-muted">"After a successful load, this replaces your draft with the current server definition and revision. Unsaved changes will be discarded. Keep a copy below before reloading if you need to reconcile changes."</p>
            <Show when=move || error.get().is_some()><p role="alert" class="text-sm text-crono-failed">{move || error.get().unwrap_or_default()}</p></Show>
            <details><summary class="cursor-pointer text-sm text-crono-primary">"Copy current draft"</summary><pre class="mt-2 max-h-64 overflow-auto rounded bg-zinc-50 p-3 text-xs">{move || editor.request().ok().and_then(|request| serde_json::to_string_pretty(&request).ok()).unwrap_or_else(|| "Complete the required fields to export the draft.".to_string())}</pre></details>
            <div class="flex flex-wrap justify-end gap-3"><button type="button" class=QUIET_ACTION_CLASS autofocus disabled=move || editor.busy.get() on:click=move |_| open.set(false)>"Keep draft"</button><button type="button" class=QUIET_ACTION_CLASS disabled=move || editor.busy.get() on:click=reload>"Discard draft and reload"</button></div>
        </Modal>
    }
}
