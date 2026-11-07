//! Attempt output is loaded only when an authorized operator opens it.
//!
//! The list API never embeds stdout, stderr, or errors. The server enforces
//! `RunRead` for this separate request, and each Attempt remains bounded there.
//! Refresh is useful while a Run can still change, but terminal output is
//! fetched once on opening rather than showing a redundant refresh control.

use super::status_can_change;
use crate::{
    api,
    components::{Icon, QUIET_ACTION_CLASS},
    navigation::AppIcon,
};
use crono_api::RunStatus;
use leptos::prelude::*;
use uuid::Uuid;

/// Fetch bounded Attempt output on mount; offer refresh only for live Runs.
#[component]
pub(super) fn RunAttemptOutput(run_id: Uuid, status: RunStatus) -> impl IntoView {
    let attempts = LocalResource::new(move || api::list_run_attempts(run_id));
    view! {
        <div class="mt-3 rounded-md border border-border bg-muted p-3">
            {status_can_change(status).then(|| view! {
                <button type="button" class=QUIET_ACTION_CLASS on:click=move |_| attempts.refetch()>
                    <Icon icon=AppIcon::RefreshCw size=18 />
                    <span>"Refresh output"</span>
                </button>
            })}
            {move || attempts.map(|result| match result {
                Ok(items) if items.is_empty() => view! { <p class="mt-2 text-sm text-muted-foreground">"No attempts yet."</p> }.into_any(),
                Ok(items) => view! { <div class="mt-2 space-y-3">{items.iter().cloned().map(|attempt| {
                    let status = attempt_status(attempt.status);
                    let stdout = attempt.stdout_tail.filter(|value| !value.is_empty());
                    let stderr = attempt.stderr_tail.filter(|value| !value.is_empty());
                    let error = attempt.error;
                    let no_output = stdout.is_none() && stderr.is_none() && error.is_none();
                    view! {
                        <section class="rounded-md border border-border bg-card p-3">
                            <h3 class="text-sm font-medium text-foreground">{format!("Attempt {} · {}", attempt.attempt, status)}</h3>
                            <p class="text-xs text-muted-foreground">{format!("Started: {} · Completed: {}", attempt.started_at.as_deref().unwrap_or("—"), attempt.completed_at.as_deref().unwrap_or("—"))}</p>
                            {attempt.worker_id.map(|worker| view! { <p class="text-xs text-muted-foreground">{format!("Worker: {worker}")}</p> })}
                            {attempt.exit_code.map(|code| view! { <p class="text-xs text-muted-foreground">{format!("Exit code: {code}")}</p> })}
                            {stdout.map(|value| view! { <div class="mt-2"><p class="text-xs font-medium text-muted-foreground">"stdout"</p><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-foreground">{value}</pre></div> })}
                            {stderr.map(|value| view! { <div class="mt-2"><p class="text-xs font-medium text-muted-foreground">"stderr"</p><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-foreground">{value}</pre></div> })}
                            {error.map(|value| view! { <p class="mt-2 text-xs text-crono-failed">{value}</p> })}
                            {no_output.then(|| view! { <p class="mt-2 text-xs text-muted-foreground">"No output captured."</p> })}
                        </section>
                    }
                }).collect_view()}</div> }.into_any(),
                Err(error) => view! { <p class="mt-2 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="mt-2 text-sm text-muted-foreground">"Loading output…"</p> }.into_any())}
        </div>
    }
}

const fn attempt_status(status: crono_api::AttemptStatus) -> &'static str {
    match status {
        crono_api::AttemptStatus::PendingDispatch => "pending dispatch",
        crono_api::AttemptStatus::Queued => "queued",
        crono_api::AttemptStatus::Running => "running",
        crono_api::AttemptStatus::Succeeded => "succeeded",
        crono_api::AttemptStatus::Skipped => "skipped",
        crono_api::AttemptStatus::Failed => "failed",
        crono_api::AttemptStatus::Dead => "dead",
        crono_api::AttemptStatus::Unknown => "unknown",
    }
}

#[cfg(test)]
mod browser_tests {
    use super::RunAttemptOutput;
    use crono_api::RunStatus;
    use leptos::prelude::*;
    use uuid::Uuid;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::HtmlElement;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn output_refresh_is_absent_for_finished_runs_but_present_for_running_runs() {
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
                <div id="finished-output"><RunAttemptOutput run_id=Uuid::from_u128(1) status=RunStatus::Succeeded /></div>
                <div id="running-output"><RunAttemptOutput run_id=Uuid::from_u128(2) status=RunStatus::Running /></div>
            }
        });
        leptos::task::tick().await;
        assert!(
            host.query_selector("#finished-output button")
                .ok()
                .flatten()
                .is_none()
        );
        let refresh = host.query_selector("#running-output button").ok().flatten();
        assert!(
            refresh
                .as_ref()
                .and_then(|button| button.text_content())
                .is_some_and(|text| text.contains("Refresh output"))
        );
        let icon = refresh.and_then(|button| button.query_selector(".app-icon").ok().flatten());
        assert!(icon.is_some(), "refresh action should show its icon");
        assert_eq!(
            icon.and_then(|icon| icon.get_attribute("aria-hidden"))
                .as_deref(),
            Some("true")
        );
        drop(handle);
        host.remove();
    }
}
