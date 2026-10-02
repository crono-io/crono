//! Namespace-filtered, cursor-paged Workflow browsing and guarded deletion.
//!
//! The API owns visibility and delete safety. Opaque cursors never cross a
//! Namespace change; the page retains only the current bounded result. Row
//! removal refreshes that result and returns focus to a stable heading.

use super::{ACTION_CLASS, ApiFailure, PANEL_CLASS};
use crate::{
    api,
    components::{
        DeleteControl, EmptyState, Icon, PageHeader, QUIET_ACTION_CLASS, ResourceSelect,
        focus_heading,
    },
    navigation::{
        AppRoute, MaterialSymbol, workflow_details_path, workflow_edit_path, workflow_launch_path,
    },
    pages::resource_options,
};
use crono_api::{Page, WorkflowResource};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use uuid::Uuid;

/// Browse definitions and navigate to explicit creation or launch routes.
#[component]
pub fn WorkflowsPage() -> impl IntoView {
    let namespace = RwSignal::new(None);
    let choices = resource_options::namespaces();
    let after = RwSignal::new(None::<String>);
    let previous = RwSignal::new(Vec::<Option<String>>::new());
    let selected = RwSignal::new(None::<Uuid>);
    Effect::new(move |_| {
        let current = namespace.get();
        if current != selected.get_untracked() {
            after.set(None);
            previous.set(Vec::new());
            selected.set(current);
        }
    });
    let workflows = LocalResource::new(move || {
        let id = namespace.get();
        let cursor = after.get();
        async move {
            match id {
                Some(id) => api::list_workflows(id, cursor.as_deref()).await,
                None => Ok(Page {
                    items: Vec::new(),
                    next_cursor: None,
                }),
            }
        }
    });
    let feedback = RwSignal::new(None::<String>);
    let heading = NodeRef::<leptos::html::H2>::new();
    let on_deleted = Callback::new(move |name: String| {
        feedback.set(Some(format!("Deleted Workflow {name}.")));
        workflows.refetch();
        focus_heading(heading);
    });
    view! {
        <div class="space-y-6">
            <PageHeader title="Workflows" description="A Workflow connects existing Jobs through dependencies: on success, on failure, or always.">
                <button type="button" class=QUIET_ACTION_CLASS on:click=move |_| { choices.reload.run(()); workflows.refetch(); }><Icon symbol=MaterialSymbol::Refresh class="text-lg" />"Refresh"</button>
                <A href=AppRoute::WorkflowsNew.path() attr:class=ACTION_CLASS>"Create Workflow"</A>
            </PageHeader>
            <div class="max-w-xl"><ResourceSelect id="workflows-namespace-filter" label="Namespace" placeholder="Select a Namespace to browse Workflows…" options=choices.options selected=namespace loading=choices.loading load_error=choices.load_error optional=true select_single=true /></div>
            <section class=PANEL_CLASS>
                <h2 node_ref=heading tabindex="-1" class="sr-only">"Workflows in Namespace"</h2>
                <Show when=move || feedback.get().is_some()><p class="text-sm text-crono-muted" role="status">{move || feedback.get().unwrap_or_default()}</p></Show>
                {move || if choices.loading.get() { view! { <p class="text-sm text-crono-muted">"Loading Namespaces…"</p> }.into_any() }
                else if namespace.get().is_none() { view! { <EmptyState icon=MaterialSymbol::AccountTree title="Select a Namespace" description="Choose a Namespace to browse its Workflows. Create a Namespace first if none exist." /> }.into_any() }
                else { view! { <WorkflowResults workflows on_deleted /> }.into_any() }}
                <div class="flex items-center justify-between border-t border-crono-border pt-3">
                    <button type="button" class=QUIET_ACTION_CLASS disabled=move || previous.get().is_empty() on:click=move |_| { let cursor = previous.get_untracked().last().cloned().flatten(); previous.update(|items| { items.pop(); }); after.set(cursor); }>"Previous"</button>
                    <span class="text-xs text-crono-muted">{move || format!("Page {}", previous.get().len() + 1)}</span>
                    {move || workflows.get().and_then(Result::ok).and_then(|page| page.next_cursor).map(|cursor| view! { <button type="button" class=QUIET_ACTION_CLASS on:click=move |_| { previous.update(|items| items.push(after.get_untracked())); after.set(Some(cursor.clone())); }>"Next"</button> })}
                </div>
            </section>
        </div>
    }
}

/// Keep loading, empty results, and server error text explicit.
#[component]
fn WorkflowResults(
    workflows: LocalResource<api::ApiResult<Page<WorkflowResource>>>,
    on_deleted: Callback<String>,
) -> impl IntoView {
    view! { {move || workflows.map(|result| match result {
        Ok(page) if page.items.is_empty() => view! { <EmptyState icon=MaterialSymbol::AccountTree title="No workflows yet" description="A Workflow connects Jobs through dependencies such as success, failure and always."><A href="/workflows/new" attr:class=ACTION_CLASS>"Create Workflow"</A></EmptyState> }.into_any(),
        Ok(page) => view! {
            <div class="overflow-x-auto"><table class="w-full text-left text-sm">
                <thead class="border-b border-crono-border text-xs text-crono-muted"><tr><th class="p-3">"Name"</th><th class="p-3">"Namespace"</th><th class="p-3">"Nodes"</th><th class="p-3">"Dependencies"</th><th class="p-3">"Updated"</th><th class="p-3">"Actions"</th></tr></thead>
                <tbody class="divide-y divide-crono-border">{page.items.iter().cloned().map(|workflow| view! { <WorkflowRow workflow on_deleted /> }).collect_view()}</tbody>
            </table></div>
        }.into_any(),
        Err(error) => view! { <ApiFailure message=error.message.clone() on_retry=Callback::new(move |()| workflows.refetch()) /> }.into_any(),
    }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Workflows…"</p> }.into_any())} }
}

/// Record links stay in the page; the sidebar contains only static navigation.
#[component]
fn WorkflowRow(workflow: WorkflowResource, on_deleted: Callback<String>) -> impl IntoView {
    let id = workflow.id;
    let displayed_name = workflow.name.clone();
    view! {
        <tr><td class="p-3 font-medium"><A href=workflow_details_path(id) attr:class="rounded text-crono-primary hover:underline focus-visible:ring-2 focus-visible:ring-crono-primary">{displayed_name}</A></td>
            <td class="p-3">{workflow.namespace}</td><td class="p-3">{workflow.nodes.len()}</td><td class="p-3">{workflow.edges.len()}</td><td class="whitespace-nowrap p-3" title=workflow.updated_at.clone()>{super::super::runs::display_time(&workflow.updated_at)}</td>
            <td class="p-3"><div class="flex flex-wrap gap-1"><A href=workflow_details_path(id) attr:class=QUIET_ACTION_CLASS>"Open"</A><A href=workflow_launch_path(id) attr:class=QUIET_ACTION_CLASS>"Run"</A><A href=workflow_edit_path(id) attr:class=QUIET_ACTION_CLASS>"Edit"</A><DeleteWorkflow id name=workflow.name on_deleted /></div></td>
        </tr>
    }
}

/// Confirm deletion and surface the API's history guard without frontend cascades.
#[component]
pub(super) fn DeleteWorkflow(
    id: Uuid,
    name: String,
    on_deleted: Callback<String>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None);
    let deleted_name = StoredValue::new(name.clone());
    let confirm = Callback::new(move |()| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = api::delete_workflow(id).await;
            if busy.is_disposed() {
                return;
            }
            match result {
                Ok(()) => {
                    open.set(false);
                    on_deleted.run(deleted_name.get_value());
                }
                Err(failure) => error.set(Some(failure.message)),
            }
            let _ = busy.try_set(false);
        });
    });
    view! { <DeleteControl id=format!("delete-workflow-{id}") resource="Workflow" name description="Only Workflows without execution history can be deleted. The server protects existing WorkflowRun history." open busy error on_confirm=confirm /> }
}
