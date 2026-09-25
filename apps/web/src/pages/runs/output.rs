//! Attempt output is loaded only when an authorized operator opens it.
//!
//! The list API never embeds stdout, stderr, or errors. The server enforces
//! `RunRead` for this separate request, and each Attempt remains bounded there.
//! Refresh is useful while a Run can still change, but terminal output is
//! fetched once on opening rather than showing a redundant refresh control.

use super::status_can_change;
use crate::{api, components::Icon, navigation::MaterialSymbol};
use crono_api::RunStatus;
use leptos::prelude::*;
use uuid::Uuid;

/// Fetch bounded Attempt output on mount; offer refresh only for live Runs.
#[component]
pub(super) fn RunAttemptOutput(run_id: Uuid, status: RunStatus) -> impl IntoView {
    let attempts = LocalResource::new(move || api::list_run_attempts(run_id));
    view! {
        <div class="mt-3 rounded-md border border-crono-border bg-zinc-50 p-3">
            {status_can_change(status).then(|| view! {
                <button type="button" class="inline-flex items-center gap-1.5 rounded-md border border-crono-border bg-white px-2.5 py-1.5 text-xs font-medium text-crono-primary shadow-sm transition-colors hover:border-crono-primary/40 hover:bg-crono-primary-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary" on:click=move |_| attempts.refetch()>
                    <Icon symbol=MaterialSymbol::Refresh class="text-base leading-none" />
                    <span>"Refresh output"</span>
                </button>
            })}
            {move || attempts.map(|result| match result {
                Ok(items) if items.is_empty() => view! { <p class="mt-2 text-sm text-crono-muted">"No attempts yet."</p> }.into_any(),
                Ok(items) => view! { <div class="mt-2 space-y-3">{items.iter().cloned().map(|attempt| {
                    let status = attempt_status(attempt.status);
                    let stdout = attempt.stdout_tail.filter(|value| !value.is_empty());
                    let stderr = attempt.stderr_tail.filter(|value| !value.is_empty());
                    let error = attempt.error;
                    let no_output = stdout.is_none() && stderr.is_none() && error.is_none();
                    view! {
                        <section class="rounded-md border border-crono-border bg-white p-3">
                            <h3 class="text-sm font-medium text-crono-text">{format!("Attempt {} · {}", attempt.attempt, status)}</h3>
                            <p class="text-xs text-crono-muted">{format!("Started: {} · Completed: {}", attempt.started_at.as_deref().unwrap_or("—"), attempt.completed_at.as_deref().unwrap_or("—"))}</p>
                            {attempt.worker_id.map(|worker| view! { <p class="text-xs text-crono-muted">{format!("Worker: {worker}")}</p> })}
                            {attempt.exit_code.map(|code| view! { <p class="text-xs text-crono-muted">{format!("Exit code: {code}")}</p> })}
                            {stdout.map(|value| view! { <div class="mt-2"><p class="text-xs font-medium text-crono-muted">"stdout"</p><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-crono-text">{value}</pre></div> })}
                            {stderr.map(|value| view! { <div class="mt-2"><p class="text-xs font-medium text-crono-muted">"stderr"</p><pre class="overflow-auto whitespace-pre-wrap break-all text-xs text-crono-text">{value}</pre></div> })}
                            {error.map(|value| view! { <p class="mt-2 text-xs text-crono-failed">{value}</p> })}
                            {no_output.then(|| view! { <p class="mt-2 text-xs text-crono-muted">"No output captured."</p> })}
                        </section>
                    }
                }).collect_view()}</div> }.into_any(),
                Err(error) => view! { <p class="mt-2 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="mt-2 text-sm text-crono-muted">"Loading output…"</p> }.into_any())}
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
        assert_eq!(
            refresh
                .and_then(|button| button
                    .query_selector(".material-symbols-outlined")
                    .ok()
                    .flatten())
                .and_then(|icon| icon.text_content())
                .as_deref(),
            Some("refresh")
        );
        drop(handle);
        host.remove();
    }
}
