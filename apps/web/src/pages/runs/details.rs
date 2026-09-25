//! Deep link for one authorized Run and its server-observed lifecycle.
//!
//! The timeline shows durable server events only. Attempt output remains an
//! explicit, separately authorized read and is not embedded in the Run record.

use super::{
    RUN_ACTION_CLASS, display_duration, display_event_time, display_time,
    history::RerunAction,
    notice::{CreatedRunNotice, show_created_run},
    output::RunAttemptOutput,
    run_triggered_at, status_can_change, status_class, status_label, trigger_label,
};
use crate::{
    api,
    components::{Icon, PageHeader},
    navigation::{AppRoute, MaterialSymbol},
};
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

/// Render one Run's identity, timing, server events, and on-demand attempts.
#[component]
pub fn RunDetailsPage() -> impl IntoView {
    let created_run_id = RwSignal::new(None::<Uuid>);
    let params = use_params_map();
    // The router may reuse this page for another Run ID; do not carry its notice.
    Effect::new(move |_| {
        let _ = params.get().get("run_id");
        created_run_id.set(None);
    });
    let run_id = move || {
        params
            .get()
            .get("run_id")
            .and_then(|value| Uuid::parse_str(&value).ok())
    };
    let run = LocalResource::new(move || async move {
        match run_id() {
            Some(id) => api::get_run(id).await,
            None => Err(api::ApiError {
                code: "invalid_run_id".to_string(),
                message: "Invalid Run URL.".to_string(),
                field: None,
            }),
        }
    });
    view! {
        <div class="space-y-6">
            <PageHeader title="Run details" description="Inspect execution timing, dispatch history, and Attempt output.">
                <A href=AppRoute::Runs.path() attr:class="text-sm font-medium text-crono-primary">"← All Runs"</A>
            </PageHeader>
            <CreatedRunNotice notice=created_run_id />
            {move || run.map(|result| match result {
                Ok(item) => view! { <RunDetails run=item.clone() on_refresh=Callback::new(move |()| run.refetch()) on_open=Callback::new(move |()| created_run_id.set(None)) on_created=Callback::new(move |id| { show_created_run(created_run_id, id); run.refetch(); }) /> }.into_any(),
                Err(error) => view! { <p class="rounded-xl border border-crono-border bg-crono-surface p-6 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Run…"</p> }.into_any())}
        </div>
    }
}

#[component]
fn RunDetails(
    run: crono_api::RunResource,
    on_refresh: Callback<()>,
    on_open: Callback<()>,
    on_created: Callback<Uuid>,
) -> impl IntoView {
    let output_open = RwSignal::new(false);
    let events = LocalResource::new(move || api::list_run_events(run.id));
    let run_id = run.id;
    let run_status = run.status;
    view! {
        <section class="space-y-4 rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div><h2 class="text-xl font-semibold text-crono-text">{run.job.clone()}</h2><p class="text-sm text-crono-muted">{format!("Destination: {}", run.target)}</p></div>
                <span class=format!("rounded-full px-2.5 py-1 text-xs font-medium {}", status_class(run.status))>{status_label(run.status)}</span>
            </div>
            <dl class="grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2 lg:grid-cols-3">
                <div><dt class="text-crono-muted">"Run ID"</dt><dd class="break-all font-mono text-xs text-crono-text">{run.id.to_string()}</dd></div>
                <div><dt class="text-crono-muted">"Triggered by"</dt><dd>{trigger_label(&run)}</dd></div>
                <div><dt class="text-crono-muted">"Triggered at"</dt><dd title=run_triggered_at(&run).to_string()>{display_time(run_triggered_at(&run))}</dd></div>
                <div><dt class="text-crono-muted">"Created at"</dt><dd title=run.created_at.clone()>{display_time(&run.created_at)}</dd></div>
                {run.schedule_id.map(|_| view! { <div><dt class="text-crono-muted">"Scheduled for"</dt><dd title=run.scheduled_at.clone()>{display_time(&run.scheduled_at)}</dd></div> })}
                <div><dt class="text-crono-muted">"Started at"</dt><dd>{run.started_at.as_deref().map_or_else(|| "—".to_string(), display_time)}</dd></div>
                <div><dt class="text-crono-muted">"Completed at"</dt><dd>{run.completed_at.as_deref().map_or_else(|| "—".to_string(), display_time)}</dd></div>
                <div><dt class="text-crono-muted">"Duration"</dt><dd>{display_duration(run.duration_ms)}</dd></div>
                <div><dt class="text-crono-muted">"Attempts"</dt><dd>{format!("{} / {}", run.attempt_count, run.max_attempts)}</dd></div>
                <div><dt class="text-crono-muted">"Queue"</dt><dd>{if run.queue.is_empty() { "—".to_string() } else { run.queue.clone() }}</dd></div>
                {run.target_set.clone().map(|set| view! { <div><dt class="text-crono-muted">"Origin Target Set"</dt><dd>{set}</dd></div> })}
                {run.rerun_of_run_id.map(|id| view! { <div><dt class="text-crono-muted">"Re-run of"</dt><dd><A href=crate::navigation::run_details_path(id) attr:class="text-crono-primary">{id.to_string()}</A></dd></div> })}
            </dl>
            {run.terminal_reason.clone().map(|reason| view! { <p class="rounded-md bg-red-50 p-3 text-sm text-crono-failed">{reason}</p> })}
            <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
                {status_can_change(run_status).then(|| view! {
                    <button type="button" class=RUN_ACTION_CLASS on:click=move |_| on_refresh.run(())><Icon symbol=MaterialSymbol::Refresh class="text-lg" />"Refresh status"</button>
                })}
                <button type="button" class=RUN_ACTION_CLASS aria-expanded=move || output_open.get().to_string() on:click=move |_| output_open.update(|open| *open = !*open)><Icon symbol=MaterialSymbol::Terminal class="text-lg" />{move || if output_open.get() { "Hide output" } else { "View output" }}</button>
                <Show when=move || run.rerunnable><RerunAction run=run.clone() on_open on_created /></Show>
            </div>
            <Show when=move || output_open.get()><RunAttemptOutput run_id status=run_status /></Show>
        </section>
        <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
            <div class="flex flex-wrap items-center justify-between gap-2"><h2 class="font-semibold text-crono-text">"Execution timeline"</h2><TimelineRefreshButton status=run_status on_refresh=Callback::new(move |()| events.refetch()) /></div>
            <p class="mt-1 text-xs text-crono-muted">"Server-observed lifecycle events. Worker execution events are not persisted here yet."</p>
            {move || events.map(|result| match result {
                Ok(items) if items.is_empty() => view! { <p class="mt-4 text-sm text-crono-muted">"No timeline events recorded."</p> }.into_any(),
                Ok(items) => view! { <ol class="mt-4 space-y-3 border-l border-crono-border pl-4">{items.iter().map(|event| view! {
                    <li class="text-sm"><time class="font-mono text-xs text-crono-muted" title=event.created_at.clone()>{display_event_time(&event.created_at)}</time><span class="ml-3 font-medium text-crono-text">{event.event_type.replace('_', " ")}</span></li>
                }).collect_view()}</ol> }.into_any(),
                Err(error) => view! { <p class="mt-4 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="mt-4 text-sm text-crono-muted">"Loading timeline…"</p> }.into_any())}
        </section>
    }
}

/// Offer timeline refresh only while the server can still append Run events.
#[component]
fn TimelineRefreshButton(status: crono_api::RunStatus, on_refresh: Callback<()>) -> impl IntoView {
    status_can_change(status).then(|| {
        view! {
            <button type="button" class=RUN_ACTION_CLASS on:click=move |_| on_refresh.run(())>
                <Icon symbol=MaterialSymbol::Refresh class="text-lg" />
                "Refresh timeline"
            </button>
        }
    })
}

#[cfg(test)]
mod browser_tests {
    use super::TimelineRefreshButton;
    use crono_api::RunStatus;
    use leptos::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::HtmlElement;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn timeline_refresh_has_an_icon_only_for_active_runs() {
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
        let handle = leptos::mount::mount_to(host.clone(), || {
            view! {
                <div id="running-timeline"><TimelineRefreshButton status=RunStatus::Running on_refresh=Callback::new(|()| {}) /></div>
                <div id="finished-timeline"><TimelineRefreshButton status=RunStatus::Succeeded on_refresh=Callback::new(|()| {}) /></div>
            }
        });
        let refresh = host
            .query_selector("#running-timeline button")
            .ok()
            .flatten();
        assert!(refresh.is_some(), "running Runs should offer refresh");
        assert!(
            refresh
                .as_ref()
                .and_then(|button| button.text_content())
                .is_some_and(|text| text.contains("Refresh timeline"))
        );
        let icon = refresh.and_then(|button| {
            button
                .query_selector(".material-symbols-outlined")
                .ok()
                .flatten()
        });
        assert_eq!(
            icon.and_then(|icon| icon.text_content()).as_deref(),
            Some("refresh")
        );
        assert!(
            host.query_selector("#finished-timeline button")
                .ok()
                .flatten()
                .is_none()
        );
        drop(handle);
        host.remove();
    }
}
