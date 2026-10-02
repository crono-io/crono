//! Definition inspection and cursor-paged invocation history.
//!
//! The authorized definition is the source for the graph and structured lists.
//! Optional Job labels improve readability without making Job listing a condition
//! of Workflow read access. History stays bounded and uses ordinary Run links.

use super::{
    ACTION_CLASS, ApiFailure, PANEL_CLASS, StateBadge, condition_label, duration,
    graph::{WorkflowGraph, definition_edges, definition_nodes},
    invalid_url,
    list::DeleteWorkflow,
    route_id, workflow_state,
};
use crate::{
    api,
    components::{Icon, QUIET_ACTION_CLASS, ResourceOption},
    navigation::{
        MaterialSymbol, job_edit_path, workflow_edit_path, workflow_launch_path,
        workflow_run_details_path,
    },
    pages::resource_options,
};
use crono_api::{WorkflowResource, WorkflowRunResource};
use leptos::prelude::*;
use leptos_router::{NavigateOptions, components::A, hooks::use_navigate};
use uuid::Uuid;

/// Inspect the current definition, with explicit retries for failed deep links.
#[component]
pub fn WorkflowDetailsPage() -> impl IntoView {
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
    view! { <div class="space-y-6">
        <div class="flex justify-between gap-3"><A href="/workflows" attr:class=QUIET_ACTION_CLASS>"All Workflows"</A><button type="button" class=QUIET_ACTION_CLASS on:click=move |_| workflow.refetch()><Icon symbol=MaterialSymbol::Refresh class="text-lg" />"Refresh"</button></div>
        {move || workflow.map(|result| match result {
            Ok(workflow) => view! { <Definition workflow=workflow.clone() /> }.into_any(),
            Err(error) => view! { <ApiFailure message=error.message.clone() on_retry=Callback::new(move |()| workflow.refetch()) /> }.into_any(),
        }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Workflow…"</p> }.into_any())}
    </div> }
}

/// Keep destructive actions under the server's history guard.
#[component]
fn Definition(workflow: WorkflowResource) -> impl IntoView {
    let namespace = RwSignal::new(Some(workflow.namespace_id));
    let jobs = resource_options::jobs(namespace);
    let stored = StoredValue::new(workflow.clone());
    let id = workflow.id;
    let navigate = use_navigate();
    view! {
        <header class="flex flex-wrap items-start justify-between gap-4">
            <div class="min-w-0"><h1 class="break-words text-3xl font-bold text-crono-text">{workflow.name.clone()}</h1><p class="mt-2 text-sm text-crono-muted">{format!("{} Jobs · {} dependencies · Namespace {} · revision {}", workflow.nodes.len(), workflow.edges.len(), workflow.namespace, workflow.revision)}</p>{workflow.description.map(|description| view! { <p class="mt-2 break-words text-sm text-crono-muted">{description}</p> })}</div>
            <div class="flex flex-wrap gap-2"><A href=workflow_launch_path(id) attr:class=ACTION_CLASS>"Run Workflow"</A><A href=workflow_edit_path(id) attr:class=QUIET_ACTION_CLASS>"Edit"</A><DeleteWorkflow id name=workflow.name on_deleted=Callback::new(move |_: String| navigate("/workflows", NavigateOptions::default())) /></div>
        </header>
        <section class=PANEL_CLASS><h2 class="font-semibold">"Jobs and dependencies"</h2><WorkflowGraph nodes=Signal::derive(move || definition_nodes(&stored.get_value(), &jobs.options.get(), &[])) edges=Signal::derive(move || definition_edges(&stored.get_value())) /></section>
        <DefinitionTables workflow=stored.get_value() jobs=jobs.options />
        <WorkflowHistory id />
    }
}

/// The structured representation remains available independently of SVG layout.
#[component]
pub(super) fn DefinitionTables(
    workflow: WorkflowResource,
    jobs: Signal<Vec<ResourceOption>>,
) -> impl IntoView {
    view! { <div class="grid min-w-0 gap-6 lg:grid-cols-2">
        <section class=PANEL_CLASS><h2 class="font-semibold">"Jobs"</h2><ul class="divide-y divide-crono-border">{workflow.nodes.into_iter().map(|node| {
            let id = node.job_id;
            view! { <li class="flex flex-wrap justify-between gap-2 py-3"><span class="break-words font-medium">{node.name}</span><A href=job_edit_path(id) attr:class="break-all rounded text-sm text-crono-primary hover:underline focus-visible:ring-2 focus-visible:ring-crono-primary">{move || jobs.get().iter().find(|job| job.id == id).map_or_else(|| id.to_string(), |job| job.label.clone())}</A></li> }
        }).collect_view()}</ul></section>
        <section class=PANEL_CLASS><h2 class="font-semibold">"Dependencies"</h2><p class="text-xs text-crono-muted">"All incoming dependencies must match before a Job can start."</p><ul class="divide-y divide-crono-border">{workflow.edges.into_iter().map(|edge| view! { <li class="flex flex-wrap items-center gap-2 py-3 text-sm"><span class="font-medium">{edge.from}</span><span class="text-crono-muted">{condition_label(edge.condition)}" → "</span><span class="font-medium">{edge.to}</span></li> }).collect_view()}</ul></section>
    </div> }
}

/// Fetch only one history page; opaque cursors are retained for Previous.
#[component]
fn WorkflowHistory(id: Uuid) -> impl IntoView {
    let before = RwSignal::new(None);
    let previous = RwSignal::new(Vec::<Option<Uuid>>::new());
    let runs = LocalResource::new(move || {
        let before = before.get();
        async move { api::list_workflow_runs(id, before).await }
    });
    view! { <section class=PANEL_CLASS>
        <div class="flex items-center justify-between"><h2 class="font-semibold">"Recent Workflow runs"</h2><button type="button" class=QUIET_ACTION_CLASS on:click=move |_| runs.refetch()>"Refresh history"</button></div>
        {move || runs.map(|result| match result {
            Ok(page) if page.items.is_empty() => view! { <p class="text-sm text-crono-muted">"No Workflow runs yet. Run this Workflow to see progress here."</p> }.into_any(),
            Ok(page) => view! { <div class="overflow-x-auto"><table class="w-full text-left text-sm"><thead class="border-b border-crono-border text-xs text-crono-muted"><tr><th class="p-3">"Started (UTC)"</th><th class="p-3">"Status"</th><th class="p-3">"Duration"</th><th class="p-3">"Actions"</th></tr></thead><tbody class="divide-y divide-crono-border">{page.items.iter().cloned().map(|run| view! { <HistoryRow run /> }).collect_view()}</tbody></table></div> }.into_any(),
            Err(error) => view! { <ApiFailure message=error.message.clone() on_retry=Callback::new(move |()| runs.refetch()) /> }.into_any(),
        }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Workflow runs…"</p> }.into_any())}
        <div class="flex justify-between gap-3"><button type="button" class=QUIET_ACTION_CLASS disabled=move || previous.get().is_empty() on:click=move |_| { before.set(previous.get_untracked().last().copied().flatten()); previous.update(|items| { items.pop(); }); }>"Previous"</button>
        {move || runs.get().and_then(Result::ok).and_then(|page| page.next_cursor).and_then(|cursor| Uuid::parse_str(&cursor).ok()).map(|cursor| view! { <button type="button" class=QUIET_ACTION_CLASS on:click=move |_| { previous.update(|items| items.push(before.get_untracked())); before.set(Some(cursor)); }>"Next"</button> })}</div>
    </section> }
}

/// Duration follows the invocation timestamps, not any individual child Run.
#[component]
fn HistoryRow(run: WorkflowRunResource) -> impl IntoView {
    let started = run.started_at.as_deref().map_or_else(
        || "Not started".to_string(),
        super::super::runs::display_time,
    );
    view! { <tr><td class="whitespace-nowrap p-3">{started}</td><td class="p-3"><StateBadge state=workflow_state(run.state) /></td><td class="whitespace-nowrap p-3">{duration(run.started_at.as_deref(), run.finished_at.as_deref())}</td><td class="p-3"><A href=workflow_run_details_path(run.id) attr:class=QUIET_ACTION_CLASS>"Open WorkflowRun"</A></td></tr> }
}
