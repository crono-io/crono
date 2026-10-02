//! Bounded live invocation visibility and explicit Workflow cancellation.
//!
//! Only the viewed invocation is refreshed, every five seconds while nonterminal.
//! One request may be in flight; terminal results stop the timer and leaving the
//! route clears it. Refresh failures label the retained snapshot as stale. The
//! server owns node transitions and cancellation; child output stays on Run pages.

use super::{
    PANEL_CLASS, StateBadge, duration,
    graph::{WorkflowGraph, definition_edges, definition_nodes},
    invalid_url,
    node_list::{NodeExecutions, NodeSummary},
    route_id, workflow_state,
};
use crate::{
    api,
    components::resource_dialogs::DELETE_ACTION_CLASS,
    components::{Icon, Modal, QUIET_ACTION_CLASS},
    navigation::{MaterialSymbol, workflow_details_path},
    pages::resource_options,
};
use crono_api::{ExecutionTarget, WorkflowRunResource, WorkflowRunState};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use std::time::Duration;

/// Load a durable snapshot and refresh only while the server says it is active.
#[component]
pub fn WorkflowRunDetailsPage() -> impl IntoView {
    let id = route_id("workflow_run_id");
    let pending = RwSignal::new(false);
    let mutating = RwSignal::new(false);
    let snapshot = RwSignal::new(None::<WorkflowRunResource>);
    let error = RwSignal::new(None::<String>);
    let selected = RwSignal::new(None);
    let run = LocalResource::new(move || {
        let current = id.get();
        pending.set(true);
        if current != selected.get_untracked() {
            selected.set(current);
            snapshot.set(None);
            error.set(None);
        }
        async move {
            let result = match current {
                Some(id) => api::get_workflow_run(id).await,
                None => Err(invalid_url()),
            };
            pending.set(false);
            result
        }
    });
    Effect::new(move |_| {
        if let Some(result) = run.get() {
            match result {
                Ok(value) => {
                    snapshot.set(Some(value));
                    error.set(None);
                }
                Err(failure) => error.set(Some(failure.message)),
            }
        }
    });
    let refresh = Callback::new(move |()| {
        if !pending.get_untracked() && !mutating.get_untracked() {
            pending.set(true);
            run.refetch();
        }
    });
    let timer = StoredValue::new(None::<IntervalHandle>);
    Effect::new(move |_| {
        let active = snapshot.get().is_some_and(|value| is_active(value.state));
        if active && timer.get_value().is_none() {
            timer.set_value(
                set_interval_with_handle(move || refresh.run(()), Duration::from_secs(5)).ok(),
            );
        } else if !active && let Some(interval) = timer.get_value() {
            interval.clear();
            timer.set_value(None);
        }
    });
    on_cleanup(move || {
        if let Some(interval) = timer.get_value() {
            interval.clear();
        }
    });
    view! { <div class="space-y-6">
        <header class="flex flex-wrap items-center justify-between gap-3"><h1 class="text-3xl font-bold text-crono-text">"Workflow run"</h1><button type="button" class=QUIET_ACTION_CLASS disabled=move || pending.get() on:click=move |_| refresh.run(())><Icon symbol=MaterialSymbol::Refresh class="text-lg" />{move || if pending.get() { "Refreshing…" } else { "Refresh" }}</button></header>
        <Show when=move || error.get().is_some()><p class="rounded-lg bg-red-50 p-4 text-sm text-crono-failed" role="alert">{move || format!("{}{}", error.get().unwrap_or_default(), if snapshot.get().is_some() { " Displaying the last successful snapshot; it may be out of date." } else { " Use Refresh to retry." })}</p></Show>
        <Show when=move || snapshot.get().is_some() fallback=move || view! { <p class="text-sm text-crono-muted">{move || if pending.get() { "Loading Workflow run…" } else { "Workflow run unavailable." }}</p> }><Invocation snapshot /></Show>
        <CancelWorkflow snapshot refresh pending busy=mutating />
    </div> }
}

/// The API's terminal states alone control polling and available cancellation.
const fn is_active(state: WorkflowRunState) -> bool {
    matches!(state, WorkflowRunState::Pending | WorkflowRunState::Running)
}

/// Present immutable graph context and current states without deriving execution decisions.
#[component]
fn Invocation(snapshot: RwSignal<Option<WorkflowRunResource>>) -> impl IntoView {
    let namespace = RwSignal::new(None);
    Effect::new(move |_| {
        let selected = snapshot.get().map(|value| value.workflow.namespace_id);
        if namespace.get_untracked() != selected {
            namespace.set(selected);
        }
    });
    let jobs = resource_options::jobs(namespace);
    let targets = resource_options::targets(namespace);
    let sets = resource_options::target_sets(namespace);
    view! {
        {move || snapshot.get().map(|value| view! { <InvocationHeader value targets sets /> })}
        {move || snapshot.get().map(|value| view! { <NodeSummary nodes=value.nodes /> })}
        <section class=PANEL_CLASS><h2 class="font-semibold">"Execution progress"</h2><WorkflowGraph nodes=Signal::derive(move || snapshot.get().map(|value| definition_nodes(&value.workflow, &jobs.options.get(), &value.nodes)).unwrap_or_default()) edges=Signal::derive(move || snapshot.get().map(|value| definition_edges(&value.workflow)).unwrap_or_default()) /></section>
        {move || snapshot.get().map(|value| view! { <NodeExecutions value /> })}
    }
}

/// Label the immutable invocation destination, with UUID fallback when listing is denied.
#[component]
fn InvocationHeader(
    value: WorkflowRunResource,
    targets: resource_options::ResourceOptions,
    sets: resource_options::ResourceOptions,
) -> impl IntoView {
    let target = value.target;
    let name = value.workflow.name.clone();
    view! {
        <section class=PANEL_CLASS><div class="flex flex-wrap items-center justify-between gap-3"><A href=workflow_details_path(value.workflow.id) attr:class="break-words text-xl font-semibold text-crono-primary hover:underline">{name}</A><StateBadge state=workflow_state(value.state) /></div>
            <p class="break-all text-xs text-crono-muted">{format!("WorkflowRun {} · definition revision {}", value.id, value.workflow.revision)}</p>
            <div class="grid gap-3 text-sm sm:grid-cols-2 lg:grid-cols-4"><p>"Started (UTC): "{value.started_at.as_deref().map_or_else(|| "Not started".to_string(), super::super::runs::display_time)}</p><p>"Finished (UTC): "{value.finished_at.as_deref().map_or_else(|| "—".to_string(), super::super::runs::display_time)}</p><p>"Duration: "{duration(value.started_at.as_deref(), value.finished_at.as_deref())}</p><p class="break-all">{move || {
                let (id, kind, choices) = match target { ExecutionTarget::Target { id } => (id, "Target", targets.options.get()), ExecutionTarget::TargetSet { id } => (id, "Target Set", sets.options.get()) };
                format!("{kind}: {}", choices.iter().find(|choice| choice.id == id).map_or_else(|| id.to_string(), |choice| choice.label.clone()))
            }}</p></div>
            <p class="text-xs text-crono-muted">{if is_active(value.state) { "Refreshes every 5 seconds while active. This graph uses the launch snapshot; later definition edits do not change it." } else { "This Workflow run is terminal. Automatic refresh has stopped." }}</p>
            {value.cancellation_requested.then(|| view! { <p role="status" class="text-sm text-crono-muted">"Cancellation requested. No further Jobs will start; active child Runs will finish normally."</p> })}
        </section>
    }
}

/// Confirm normal server cancellation; no browser process-killing semantics exist.
#[component]
fn CancelWorkflow(
    snapshot: RwSignal<Option<WorkflowRunResource>>,
    refresh: Callback<()>,
    pending: RwSignal<bool>,
    busy: RwSignal<bool>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let allowed = Signal::derive(move || {
        snapshot
            .get()
            .is_some_and(|run| is_active(run.state) && !run.cancellation_requested)
    });
    let confirm = move |_| {
        if busy.get_untracked() || pending.get_untracked() || !allowed.get_untracked() {
            return;
        }
        let Some(run) = snapshot.get_untracked() else {
            return;
        };
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = api::cancel_workflow_run(run.id).await;
            if busy.is_disposed() {
                return;
            }
            match result {
                Ok(value) => {
                    snapshot.set(Some(value));
                    open.set(false);
                    busy.set(false);
                    refresh.run(());
                }
                Err(failure) => error.set(Some(failure.message)),
            }
            busy.set(false);
        });
    };
    view! {
        <Show when=move || allowed.get()><button type="button" class=DELETE_ACTION_CLASS disabled=move || pending.get() || busy.get() on:click=move |_| { error.set(None); open.set(true); }>"Cancel Workflow run"</button></Show>
        <Modal id="workflow-run-cancellation" open busy title=Signal::derive(|| "Cancel Workflow run?".to_string())>
            <p class="text-sm text-crono-muted">"Pending Jobs will not start. Active child Runs finish normally because Crono does not yet expose safe Run cancellation. The Workflow becomes cancelled after those Runs terminate."</p>
            <Show when=move || error.get().is_some()><p role="alert" class="text-sm text-crono-failed">{move || error.get().unwrap_or_default()}</p></Show>
            <div class="flex flex-wrap justify-end gap-3"><button type="button" class=QUIET_ACTION_CLASS autofocus disabled=move || busy.get() on:click=move |_| open.set(false)>"Keep running"</button><button type="button" class=DELETE_ACTION_CLASS disabled=move || busy.get() || pending.get() || !allowed.get() on:click=confirm>"Cancel Workflow run"</button></div>
        </Modal>
    }
}
