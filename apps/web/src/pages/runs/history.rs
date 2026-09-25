//! Server-filtered, cursor-paged Run history with explicit repeat actions.
//!
//! The list shows only operational metadata. Output and exact saved execution
//! inputs stay behind dedicated `RunRead` endpoints, with output opened on demand.

use super::{
    ACTION_CLASS, RUN_ACTION_CLASS, display_duration, display_time, history_time_parts,
    notice::{CreatedRunNotice, show_created_run},
    output::RunAttemptOutput,
    run_job_name, run_namespace, run_target_name, run_triggered_at, status_class, status_label,
    trigger_label,
};
use crate::{
    api,
    components::{EmptyState, Icon, PageHeader, ResourceSelect},
    navigation::{AppRoute, MaterialSymbol, run_details_path},
    pages::resource_options,
};
use crono_api::{Page, RunResource, RunStatus};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use uuid::Uuid;

/// Browse Runs first; manual execution is a separate routed action.
#[component]
pub fn RunsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let job_id = RwSignal::new(None);
    let target_id = RwSignal::new(None);
    let target_set_id = RwSignal::new(None);
    let status = RwSignal::new(None::<RunStatus>);
    let before = RwSignal::new(None::<Uuid>);
    let created_run_id = RwSignal::new(None::<Uuid>);
    let page_stack = RwSignal::new(Vec::<Option<Uuid>>::new());
    let namespace_choices = resource_options::namespaces();
    let job_choices = resource_options::jobs(namespace_id);
    let target_choices = resource_options::targets(namespace_id);
    let target_set_choices = resource_options::target_sets(namespace_id);
    let previous_namespace = RwSignal::new(None);
    Effect::new(move |_| {
        let current = namespace_id.get();
        if previous_namespace.get_untracked() != current {
            job_id.set(None);
            target_id.set(None);
            target_set_id.set(None);
            before.set(None);
            page_stack.set(Vec::new());
            previous_namespace.set(current);
        }
    });
    let previous_filters =
        RwSignal::new((None::<Uuid>, None::<Uuid>, None::<Uuid>, None::<RunStatus>));
    Effect::new(move |_| {
        let current = (
            job_id.get(),
            target_id.get(),
            target_set_id.get(),
            status.get(),
        );
        if previous_filters.get_untracked() != current {
            before.set(None);
            page_stack.set(Vec::new());
            previous_filters.set(current);
        }
    });
    let runs = LocalResource::new(move || {
        api::filtered_runs(
            namespace_id.get(),
            status.get(),
            job_id.get(),
            target_id.get(),
            target_set_id.get(),
            before.get(),
        )
    });
    let reset_page = move || {
        before.set(None);
        page_stack.set(Vec::new());
    };

    view! {
        <div class="space-y-6">
            <PageHeader title="Runs" description="Observe executions, inspect failures, and repeat previous runs.">
                <A href=AppRoute::RunsNew.path() attr:class=ACTION_CLASS>"Run a Job"</A>
            </PageHeader>
            <CreatedRunNotice notice=created_run_id />
            <section class="grid gap-3 rounded-xl border border-crono-border bg-crono-surface p-4 sm:grid-cols-2 lg:grid-cols-5">
                <ResourceSelect id="runs-namespace-filter" label="Namespace" placeholder="All namespaces" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error optional=true />
                <div><label for="runs-status-filter" class="mb-2 block text-sm font-medium text-crono-text">"Status"</label>
                    <select id="runs-status-filter" class="w-full rounded-md border border-crono-border bg-white px-3 py-2 text-sm text-crono-text" on:change=move |event| { status.set(parse_status(&event_target_value(&event))); reset_page(); }>
                        <option value="">"All statuses"</option>
                        <option value="pending_dispatch">"Pending dispatch"</option><option value="queued">"Queued"</option>
                        <option value="running">"Running"</option><option value="retry_wait">"Retry wait"</option>
                        <option value="succeeded">"Succeeded"</option><option value="failed">"Failed"</option>
                        <option value="dead">"Dead"</option><option value="skipped">"Skipped"</option>
                        <option value="cancelled">"Cancelled"</option><option value="unknown">"Unknown"</option>
                    </select>
                </div>
                <ResourceSelect id="runs-job-filter" label="Job" placeholder="All Jobs" options=job_choices.options selected=job_id loading=job_choices.loading load_error=job_choices.load_error optional=true />
                <ResourceSelect id="runs-target-filter" label="Target" placeholder="All Targets" options=target_choices.options selected=target_id loading=target_choices.loading load_error=target_choices.load_error optional=true />
                <ResourceSelect id="runs-target-set-filter" label="Target Set" placeholder="All Target Sets" options=target_set_choices.options selected=target_set_id loading=target_set_choices.loading load_error=target_set_choices.load_error optional=true />
            </section>
            <div class="flex justify-end"><button type="button" class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover" on:click=move |_| runs.refetch()>"Refresh"</button></div>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                {move || runs.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! {
                        <EmptyState icon=MaterialSymbol::PlayCircle title="No Runs yet" description="No executions match these filters. Start a Job or choose another filter.">
                            <A href=AppRoute::RunsNew.path() attr:class=ACTION_CLASS>"Run a Job"</A>
                        </EmptyState>
                    }.into_any(),
                    Ok(page) => view! { <RunList page=page.clone() on_open=Callback::new(move |()| created_run_id.set(None)) on_created=Callback::new(move |id| { show_created_run(created_run_id, id); runs.refetch(); }) /> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Runs…"</p> }.into_any())}
            </section>
            <div class="flex items-center justify-between">
                <button type="button" class="text-sm font-medium text-crono-primary disabled:text-crono-muted" disabled=move || page_stack.get().is_empty() on:click=move |_| {
                    page_stack.update(|stack| { before.set(stack.pop().flatten()); });
                }>"← Previous"</button>
                {move || runs.get().and_then(Result::ok).and_then(|page| page.next_cursor).and_then(|cursor| Uuid::parse_str(&cursor).ok()).map(|cursor| view! {
                    <button type="button" class="text-sm font-medium text-crono-primary" on:click=move |_| { page_stack.update(|stack| stack.push(before.get_untracked())); before.set(Some(cursor)); }>"Next →"</button>
                })}
            </div>
        </div>
    }
}

/// Align execution times across Runs without coupling the list to output loading.
#[component]
fn RunList(
    page: Page<RunResource>,
    on_open: Callback<()>,
    on_created: Callback<Uuid>,
) -> impl IntoView {
    view! {
        <div class="hidden grid-cols-[minmax(0,3fr)_minmax(0,1.3fr)_minmax(0,1.3fr)_minmax(0,1fr)] gap-x-5 border-b border-crono-border bg-crono-primary-soft px-5 py-3 text-xs font-medium uppercase tracking-wide text-crono-muted sm:px-6 xl:grid">
            <span>"Job / Target"</span><span>"Started"</span><span>"Finished"</span><span>"Status"</span>
        </div>
        <ul class="divide-y divide-crono-border">{page.items.into_iter().map(|run| view! {
            <li class="px-5 py-4 sm:px-6"><RunListItem run on_open on_created /></li>
        }).collect_view()}</ul>
    }
}

/// Keep dates, clock times, and statuses in stable columns across the history.
#[component]
fn RunListItem(
    run: RunResource,
    on_open: Callback<()>,
    on_created: Callback<Uuid>,
) -> impl IntoView {
    let output_open = RwSignal::new(false);
    let run_id = run.id;
    let destination = run.target_set.as_ref().map_or_else(
        || run_target_name(&run).to_string(),
        |set| format!("{} (from set {set})", run_target_name(&run)),
    );
    let details_path = run_details_path(run_id);
    let rerun_allowed = run.rerunnable;
    let rerun_record = run.clone();
    let triggered_at = run_triggered_at(&run).to_string();
    let started_at = run.started_at.clone();
    let completed_at = run.completed_at.clone();
    let queue = if run.queue.is_empty() {
        "—"
    } else {
        &run.queue
    };
    let attempts = if run.attempt_count == 1 {
        "attempt"
    } else {
        "attempts"
    };
    view! {
        <div>
            <div class="grid grid-cols-2 gap-x-4 gap-y-3 xl:grid-cols-[minmax(0,3fr)_minmax(0,1.3fr)_minmax(0,1.3fr)_minmax(0,1fr)] xl:gap-x-5 xl:gap-y-0">
                <div class="col-span-2 min-w-0 xl:col-span-1">
                    <h2 class="font-semibold text-crono-text">{run_job_name(&run).to_string()}</h2>
                    <p class="text-sm text-crono-muted">{format!("{} → {}", run_namespace(&run), destination)}</p>
                    <p class="mt-1 text-xs text-crono-muted" title=triggered_at.clone()>{format!("Triggered by {} · {}", trigger_label(&run), display_time(&triggered_at))}</p>
                    <p class="text-xs text-crono-muted">{format!("Queue {queue} · {} {attempts} · Run …{}", run.attempt_count, run.id.simple().to_string().chars().skip(24).collect::<String>())}</p>
                </div>
                <div class="min-w-0">
                    <p class="mb-1 text-xs font-medium uppercase tracking-wide text-crono-muted xl:hidden">"Started"</p>
                    <RunHistoryTime value=started_at />
                </div>
                <div class="min-w-0">
                    <p class="mb-1 text-xs font-medium uppercase tracking-wide text-crono-muted xl:hidden">"Finished"</p>
                    <RunHistoryTime value=completed_at />
                    {run.duration_ms.map(|duration| view! {
                        <p class="mt-1 inline-flex items-center gap-1 text-xs text-crono-muted">
                            <Icon symbol=MaterialSymbol::Timer class="text-base" />
                            <span>{format!("Elapsed {}", display_duration(Some(duration)))}</span>
                        </p>
                    })}
                </div>
                <div class="col-span-2 xl:col-span-1">
                    <p class="mb-1 text-xs font-medium uppercase tracking-wide text-crono-muted xl:hidden">"Status"</p>
                    <span class=format!("inline-flex rounded-full px-2.5 py-1 text-xs font-medium {}", status_class(run.status))>{status_label(run.status)}</span>
                </div>
            </div>
            <div class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                <A href=details_path attr:class=RUN_ACTION_CLASS><Icon symbol=MaterialSymbol::Info class="text-lg" />"Details"</A>
                <button type="button" class=RUN_ACTION_CLASS aria-expanded=move || output_open.get().to_string() on:click=move |_| output_open.update(|open| *open = !*open)><Icon symbol=MaterialSymbol::Terminal class="text-lg" />{move || if output_open.get() { "Hide output" } else { "Output" }}</button>
                <Show when=move || rerun_allowed><RerunAction run=rerun_record.clone() on_open on_created /></Show>
            </div>
            <Show when=move || output_open.get()><RunAttemptOutput run_id status=run.status /></Show>
        </div>
    }
}

/// Show the server's exact timestamp in a tooltip while making the clock scannable.
#[component]
fn RunHistoryTime(value: Option<String>) -> impl IntoView {
    match value {
        Some(value) => {
            let (clock, date) = history_time_parts(&value);
            view! {
                <time datetime=value.clone() title=value class="block text-sm text-crono-text">
                    <span class="block font-medium tabular-nums">{clock}</span>
                    <span class="block text-xs text-crono-muted tabular-nums">{date}</span>
                </time>
            }
            .into_any()
        }
        None => view! { <span class="text-sm text-crono-muted">"Not recorded"</span> }.into_any(),
    }
}

/// Require confirmation before repeating a saved, potentially disruptive command.
/// The parent owns success feedback; this control retains only local failures.
#[component]
pub(super) fn RerunAction(
    run: RunResource,
    on_open: Callback<()>,
    on_created: Callback<Uuid>,
) -> impl IntoView {
    let confirming = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let request_id = RwSignal::new(Uuid::now_v7());
    let run_id = run.id;
    let job = run.job;
    let target = run.target;
    let set_note = run
        .target_set
        .map(|set| format!("This repeats only {target}, one member of Target Set {set}."));
    view! {
        <div class="contents">
            <button type="button" class=RUN_ACTION_CLASS aria-expanded=move || confirming.get().to_string() disabled=move || submitting.get() on:click=move |_| { on_open.run(()); request_id.set(Uuid::now_v7()); error.set(None); confirming.set(true); }><Icon symbol=MaterialSymbol::Replay class="text-lg" />"Re-run"</button>
            <Show when=move || confirming.get()>
                <div class="mt-2 w-full basis-full rounded-md border border-amber-200 bg-amber-50 p-4 text-sm text-crono-text" role="group" aria-label="Confirm re-run">
                    <p class="font-semibold">"Run again?"</p>
                    <p class="mt-1">{format!("Job: {job} · Destination: {target}")}</p>
                    <p class="mt-1 text-crono-muted">"The original saved execution command, inputs, and policy will be reused. Secret values are not shown here."</p>
                    {set_note.clone().map(|note| view! { <p class="mt-1 text-crono-muted">{note}</p> })}
                    <div class="mt-3 flex flex-wrap gap-3">
                        <button type="button" class="rounded-md px-3 py-2 font-medium text-crono-muted hover:bg-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary" on:click=move |_| { confirming.set(false); error.set(None); }>"Cancel"</button>
                        <button type="button" class="rounded-md bg-crono-primary px-3 py-2 font-semibold text-white hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2 disabled:opacity-50" disabled=move || submitting.get() on:click=move |_| {
                            submitting.set(true); error.set(None);
                            let request_id = request_id.get_untracked();
                            spawn_local(async move {
                                match api::rerun_run(run_id, request_id).await {
                                    Ok(created) => { confirming.set(false); on_created.run(created.id); }
                                    Err(failure) => error.set(Some(failure.message)),
                                }
                                submitting.set(false);
                            });
                        }>"Run again"</button>
                    </div>
                </div>
            </Show>
            {move || error.get().map(|message| view! { <span class="basis-full text-sm text-crono-failed" role="alert">{message}</span> })}
        </div>
    }
}

fn parse_status(value: &str) -> Option<RunStatus> {
    match value {
        "pending_dispatch" => Some(RunStatus::PendingDispatch),
        "queued" => Some(RunStatus::Queued),
        "running" => Some(RunStatus::Running),
        "retry_wait" => Some(RunStatus::RetryWait),
        "succeeded" => Some(RunStatus::Succeeded),
        "failed" => Some(RunStatus::Failed),
        "dead" => Some(RunStatus::Dead),
        "skipped" => Some(RunStatus::Skipped),
        "cancelled" => Some(RunStatus::Cancelled),
        "unknown" => Some(RunStatus::Unknown),
        _ => None,
    }
}

#[cfg(test)]
mod browser_tests {
    use super::RerunAction;
    use crono_api::RunResource;
    use leptos::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::{Event, HtmlElement};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn rerun_confirmation_opens_full_width_and_cancels_without_submitting() {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let host = document
            .create_element("div")
            .ok()
            .and_then(|element| element.dyn_into::<HtmlElement>().ok());
        assert!(host.is_some(), "browser test requires a host element");
        let Some(host) = host else {
            return;
        };
        let Some(body) = document.body() else {
            return;
        };
        assert!(body.append_child(&host).is_ok());

        let id = uuid::Uuid::from_u128(1);
        let run: Result<RunResource, _> = serde_json::from_value(serde_json::json!({
            "id": id, "job_id": id, "job": "demo/backup", "target_id": id,
            "target": "demo/db", "status": "succeeded", "rerunnable": true,
            "scheduled_at": "2026-09-25T12:00:00Z", "created_at": "2026-09-25T12:00:00Z",
            "attempt_count": 1, "max_attempts": 1, "lateness_seconds": 0
        }));
        assert!(run.is_ok(), "test Run must match the API contract");
        let Ok(run) = run else {
            return;
        };
        let handle = leptos::mount::mount_to(host.clone(), move || {
            view! {
                <div class="flex flex-wrap">
                    <span>"Other action"</span>
                    <RerunAction run=run.clone() on_open=Callback::new(|()| {}) on_created=Callback::new(|_| {}) />
                </div>
            }
        });
        let trigger = host.query_selector("button").ok().flatten();
        assert!(trigger.is_some(), "Re-run action must render");
        let Some(trigger) = trigger else {
            return;
        };
        assert!(
            trigger
                .text_content()
                .is_some_and(|text| text.contains("Re-run"))
        );
        let icon = trigger
            .query_selector(".material-symbols-outlined")
            .ok()
            .flatten();
        assert!(icon.is_some(), "Re-run action should have its icon");
        assert_eq!(
            icon.and_then(|icon| icon.get_attribute("aria-hidden"))
                .as_deref(),
            Some("true")
        );
        let click = Event::new("click");
        assert!(click.is_ok());
        let Ok(click) = click else {
            return;
        };
        assert!(trigger.dispatch_event(&click).is_ok());
        leptos::task::tick().await;
        assert_eq!(
            trigger.get_attribute("aria-expanded").as_deref(),
            Some("true")
        );

        let confirmation = host.query_selector("[role='group']").ok().flatten();
        assert!(confirmation.is_some(), "confirmation should open");
        let Some(confirmation) = confirmation else {
            return;
        };
        assert!(confirmation.get_attribute("class").is_some_and(|classes| {
            classes
                .split_whitespace()
                .any(|class| class == "basis-full")
        }));
        assert!(
            confirmation
                .text_content()
                .is_some_and(|text| text.contains("Run again?"))
        );
        let cancel = confirmation.query_selector("button").ok().flatten();
        assert!(cancel.is_some());
        let Some(cancel) = cancel else {
            return;
        };
        assert!(cancel.dispatch_event(&click).is_ok());
        leptos::task::tick().await;
        assert_eq!(
            trigger.get_attribute("aria-expanded").as_deref(),
            Some("false")
        );
        assert!(
            host.query_selector("[role='group']")
                .ok()
                .flatten()
                .is_none()
        );
        drop(handle);
        host.remove();
    }
}
