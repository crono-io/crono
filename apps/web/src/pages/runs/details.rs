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
    navigation::{AppIcon, AppRoute},
};
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

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
                <A href=AppRoute::Runs.path() attr:class=RUN_ACTION_CLASS>"← All Runs"</A>
            </PageHeader>
            <CreatedRunNotice notice=created_run_id />
            {move || run.map(|result| match result {
                Ok(item) => view! { <RunDetails run=item.clone() on_refresh=Callback::new(move |()| run.refetch()) on_open=Callback::new(move |()| created_run_id.set(None)) on_created=Callback::new(move |id| { show_created_run(created_run_id, id); run.refetch(); }) /> }.into_any(),
                Err(error) => view! { <p class="rounded-xl border border-border bg-card p-6 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="text-sm text-muted-foreground">"Loading Run…"</p> }.into_any())}
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
    let trigger = trigger_label(&run);
    let triggered_at = run_triggered_at(&run).to_string();
    let rerun_source = run.clone();
    view! {
        <section class="space-y-5 rounded-xl border border-border bg-card p-5 sm:p-6">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    <h2 class="break-words text-xl font-semibold tracking-tight text-foreground">{run.job.clone()}</h2>
                    <p class="mt-0.5 break-words text-sm text-muted-foreground">{format!("Destination: {}", run.target)}</p>
                </div>
                <span class=format!("shrink-0 rounded-full px-3 py-1 text-xs font-semibold ring-1 ring-inset ring-current/20 {}", status_class(run.status))>{status_label(run.status)}</span>
            </div>
            <dl class="grid gap-x-6 gap-y-5 border-t border-border pt-5 sm:grid-cols-2 lg:grid-cols-3">
                <Stat label="Run ID" icon=AppIcon::Hash class="sm:col-span-2 lg:col-span-2">
                    <span class="flex flex-wrap items-center gap-x-3 gap-y-1">
                        <span class="break-all font-mono text-xs text-foreground">{run.id.to_string()}</span>
                        <CopyRunId id=run.id />
                    </span>
                </Stat>
                <Stat label="Triggered by" icon=AppIcon::User>{trigger}</Stat>
                <Stat label="Triggered at" icon=AppIcon::CalendarClock><span title=triggered_at.clone()>{display_time(&triggered_at)}</span></Stat>
                <Stat label="Created at" icon=AppIcon::CalendarClock><span title=run.created_at.clone()>{display_time(&run.created_at)}</span></Stat>
                {run.schedule_id.map(|_| view! { <Stat label="Scheduled for" icon=AppIcon::CalendarClock><span title=run.scheduled_at.clone()>{display_time(&run.scheduled_at)}</span></Stat> })}
                <Stat label="Started at" icon=AppIcon::CalendarClock>{optional_time(run.started_at.as_deref())}</Stat>
                <Stat label="Completed at" icon=AppIcon::CalendarClock>{optional_time(run.completed_at.as_deref())}</Stat>
                <Stat label="Duration" icon=AppIcon::Timer><span class="text-base font-semibold">{display_duration(run.duration_ms)}</span></Stat>
                <Stat label="Attempts" icon=AppIcon::Repeat>{format!("{} / {}", run.attempt_count, run.max_attempts)}</Stat>
                <Stat label="Queue" icon=AppIcon::ListOrdered>{if run.queue.is_empty() { "—".to_string() } else { run.queue.clone() }}</Stat>
                {run.target_set.clone().map(|set| view! { <Stat label="Origin Target Set">{set}</Stat> })}
                {run.rerun_of_run_id.map(|id| view! { <Stat label="Re-run of" icon=AppIcon::RotateCcw><A href=crate::navigation::run_details_path(id) attr:class="break-all rounded-sm font-mono text-xs font-medium text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary">{id.to_string()}</A></Stat> })}
            </dl>
            {run.terminal_reason.clone().map(|reason| view! { <p class="rounded-md bg-red-50 p-3 text-sm text-crono-failed">{reason}</p> })}
            <div class="border-t border-border pt-3"><div class="-ml-2 flex flex-wrap items-center gap-x-1 gap-y-1 text-sm sm:-ml-3">
                {status_can_change(run_status).then(|| view! {
                    <button type="button" class=RUN_ACTION_CLASS on:click=move |_| on_refresh.run(())><Icon icon=AppIcon::RefreshCw size=16 />"Refresh status"</button>
                })}
                <button type="button" class=RUN_ACTION_CLASS aria-expanded=move || output_open.get().to_string() on:click=move |_| output_open.update(|open| *open = !*open)><Icon icon=AppIcon::ScrollText size=16 />{move || if output_open.get() { "Hide output" } else { "View output" }}</button>
                <Show when=move || run.rerunnable><RerunAction run=rerun_source.clone() on_open on_created /></Show>
            </div></div>
            <Show when=move || output_open.get()><RunAttemptOutput run_id status=run_status /></Show>
        </section>
        <section class="rounded-xl border border-border bg-card p-5 sm:p-6">
            <div class="flex flex-wrap items-center justify-between gap-2"><h2 class="font-semibold text-foreground">"Execution timeline"</h2><TimelineRefreshButton status=run_status on_refresh=Callback::new(move |()| events.refetch()) /></div>
            <p class="mt-1 text-xs text-muted-foreground">"Server-observed lifecycle events. Worker execution events are not persisted here yet."</p>
            {move || events.map(|result| match result {
                Ok(items) if items.is_empty() => view! { <p class="mt-4 text-sm text-muted-foreground">"No timeline events recorded."</p> }.into_any(),
                Ok(items) => view! { <ol class="mt-4 space-y-3 border-l border-border pl-4">{items.iter().map(|event| view! {
                    <li class="text-sm"><time class="font-mono text-xs text-muted-foreground" title=event.created_at.clone()>{display_event_time(&event.created_at)}</time><span class="ml-3 font-medium text-foreground">{event.event_type.replace('_', " ")}</span></li>
                }).collect_view()}</ol> }.into_any(),
                Err(error) => view! { <p class="mt-4 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="mt-4 text-sm text-muted-foreground">"Loading timeline…"</p> }.into_any())}
        </section>
    }
}

/// One metadata item: a small muted label (optionally with a 14px icon) above a
/// darker, heavier value, so labels recede and values scan first.
#[component]
fn Stat(
    label: &'static str,
    #[prop(optional)] icon: Option<AppIcon>,
    #[prop(optional)] class: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <div class=format!("min-w-0 {class}")>
            <dt class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                {icon.map(|icon| view! { <Icon icon size=14 /> })}
                {label}
            </dt>
            <dd class="mt-1 break-words text-sm font-medium tabular-nums text-foreground">{children()}</dd>
        </div>
    }
}

/// Timestamps that have not happened yet render as a muted dash.
fn optional_time(value: Option<&str>) -> AnyView {
    match value {
        Some(value) => {
            view! { <span title=value.to_string()>{display_time(value)}</span> }.into_any()
        }
        None => view! { <span class="font-normal text-muted-foreground">"—"</span> }.into_any(),
    }
}

/// How long the copy outcome stays visible before the button resets.
const COPY_FEEDBACK: std::time::Duration = std::time::Duration::from_secs(2);

/// Copy `text` and report the real outcome through `done`.
///
/// Secure contexts use the async Clipboard API; `done(true)` runs only after
/// the browser confirms the write. `navigator.clipboard` is `undefined` on
/// plain HTTP away from localhost (for example a LAN address), so absence — or
/// a rejected write — falls back to [`legacy_copy`]. Must be called directly
/// from the click handler so the browser still honors the user gesture.
fn write_clipboard(text: &str, done: impl Fn(bool) + Clone + 'static) {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        done(false);
        return;
    };
    let supported = js_sys::Reflect::get(&navigator, &JsValue::from_str("clipboard"))
        .is_ok_and(|clipboard| !clipboard.is_undefined() && !clipboard.is_null());
    if !supported {
        done(legacy_copy(text));
        return;
    }
    let promise = navigator.clipboard().write_text(text);
    let text = text.to_string();
    let (on_ok, on_err) = (done.clone(), done);
    // The callbacks must outlive this handler; they are two tiny closures per click.
    let ok = Closure::new(move |_: JsValue| on_ok(true));
    let err = Closure::new(move |_: JsValue| on_err(legacy_copy(&text)));
    let _ = promise.then2(&ok, &err);
    ok.forget();
    err.forget();
}

/// Copy through a transient off-screen textarea and `document.execCommand("copy")`.
///
/// Deprecated but still the only option in insecure contexts. It reports the
/// command's own result, restores the previously focused element, and never
/// leaves the textarea in the DOM.
fn legacy_copy(text: &str) -> bool {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return false;
    };
    let Some(body) = document.body() else {
        return false;
    };
    let Some(textarea) = document
        .create_element("textarea")
        .ok()
        .and_then(|element| element.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
    else {
        return false;
    };
    textarea.set_value(text);
    let _ = textarea.set_attribute("readonly", "");
    let _ = textarea.set_attribute("aria-hidden", "true");
    let _ = textarea.set_attribute(
        "style",
        "position:fixed;top:0;left:0;opacity:0;pointer-events:none",
    );
    let previous = document
        .active_element()
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok());
    if body.append_child(&textarea).is_err() {
        return false;
    }
    let _ = textarea.focus();
    textarea.select();
    let copied = document
        .dyn_into::<web_sys::HtmlDocument>()
        .is_ok_and(|document| document.exec_command("copy").unwrap_or(false));
    textarea.remove();
    if let Some(previous) = previous {
        let _ = previous.focus();
    }
    copied
}

/// Copy the Run ID to the clipboard with visible, announced feedback.
///
/// Success is shown only after a copy actually succeeds; any failure shows
/// "Copy failed". Feedback resets after [`COPY_FEEDBACK`], and a new click
/// cancels the previous reset so rapid clicks cannot clear fresh feedback.
#[component]
fn CopyRunId(id: Uuid) -> impl IntoView {
    let state = RwSignal::new(None::<bool>);
    let reset = StoredValue::new(None::<TimeoutHandle>);
    let finish = move |copied: bool| {
        state.set(Some(copied));
        if let Some(previous) = reset.get_value() {
            previous.clear();
        }
        reset.set_value(set_timeout_with_handle(move || state.set(None), COPY_FEEDBACK).ok());
    };
    view! {
        <button
            type="button"
            aria-label="Copy Run ID"
            on:click=move |_| write_clipboard(&id.to_string(), finish)
            class="inline-flex items-center gap-1 rounded-md px-1.5 py-1 text-xs font-medium text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        >
            {move || if state.get() == Some(true) { view! { <Icon icon=AppIcon::Check size=14 /> } } else { view! { <Icon icon=AppIcon::Copy size=14 /> } }}
            {move || match state.get() { Some(true) => "Copied!", Some(false) => "Copy failed", None => "Copy" }}
        </button>
        <span class="sr-only" role="status">{move || match state.get() { Some(true) => "Run ID copied", Some(false) => "Could not copy Run ID", None => "" }}</span>
    }
}

/// Offer timeline refresh only while the server can still append Run events.
#[component]
fn TimelineRefreshButton(status: crono_api::RunStatus, on_refresh: Callback<()>) -> impl IntoView {
    status_can_change(status).then(|| {
        view! {
            <button type="button" class=RUN_ACTION_CLASS on:click=move |_| on_refresh.run(())>
                <Icon icon=AppIcon::RefreshCw size=18 />
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
        let icon = refresh.and_then(|button| button.query_selector(".app-icon svg").ok().flatten());
        assert!(icon.is_some(), "refresh action should show its icon");
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
