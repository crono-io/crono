//! Live worker presence and allowlisted diagnostics from server-owned heartbeats.
//!
//! The list links to one worker's details, loaded through the same `WorkerRead`
//! authorization as the list. Missing diagnostics from older workers are shown
//! as unknown rather than inferred from the browser or server environment.

use crate::{
    api,
    components::{EmptyState, PageHeader},
    navigation::{MaterialSymbol, worker_details_path},
};
use crono_api::WorkerStatus;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};

/// List recently observed worker sessions without exposing management actions.
#[component]
pub fn WorkersPage() -> impl IntoView {
    let workers = LocalResource::new(api::list_workers);
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Workers"
                description="Heartbeat-backed worker presence, capacity, and execution activity."
            />
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="flex items-center justify-between border-b border-crono-border px-5 py-4 sm:px-6">
                    <div>
                        <h2 class="font-semibold text-crono-text">"Worker presence"</h2>
                        <p class="mt-1 text-xs text-crono-muted">"Online ≤30s · stale ≤2m · offline retained for 7 days"</p>
                    </div>
                    <button class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover" type="button" on:click=move |_| workers.refetch()>"Refresh"</button>
                </header>
                {move || workers.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! {
                        <EmptyState
                            icon=MaterialSymbol::Memory
                            title="No workers yet"
                            description="Start crono-worker to publish its first presence heartbeat."
                        />
                    }.into_any(),
                    Ok(page) => view! {
                        <div class="overflow-x-auto">
                            <table class="w-full min-w-[760px] text-left text-sm">
                                <thead class="bg-crono-primary-soft text-xs uppercase tracking-wide text-crono-muted">
                                    <tr>
                                        <th class="px-5 py-3 font-medium sm:px-6">"Worker"</th>
                                        <th class="px-5 py-3 font-medium">"Status"</th>
                                        <th class="px-5 py-3 font-medium">"Queue"</th>
                                        <th class="px-5 py-3 font-medium">"Active / capacity"</th>
                                        <th class="px-5 py-3 font-medium">"Version"</th>
                                        <th class="px-5 py-3 font-medium sm:pr-6">"Last seen"</th>
                                    </tr>
                                </thead>
                                <tbody class="divide-y divide-crono-border">
                                    {page.items.iter().cloned().map(|worker| {
                                        let details_path = worker_details_path(&worker.worker_id);
                                        let (status, status_class) = match worker.status {
                                            WorkerStatus::Online => ("online", "bg-emerald-50 text-emerald-700"),
                                            WorkerStatus::Stale => ("stale", "bg-amber-50 text-amber-700"),
                                            WorkerStatus::Offline => ("offline", "bg-slate-100 text-slate-600"),
                                        };
                                        view! {
                                            <tr>
                                                <td class="px-5 py-4 font-medium text-crono-text sm:px-6"><A href=details_path attr:class="font-mono text-crono-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary">{worker.worker_id.clone()}</A></td>
                                                <td class="px-5 py-4"><span class=format!("rounded-full px-2.5 py-1 text-xs font-medium {status_class}")>{status}</span></td>
                                                <td class="px-5 py-4 text-crono-muted">{worker.queue.clone()}</td>
                                                <td class="px-5 py-4 text-crono-muted">{format!("{} / {}", worker.active_executions, worker.concurrency)}</td>
                                                <td class="px-5 py-4 text-crono-muted">{worker.version.clone()}</td>
                                                <td class="px-5 py-4 text-xs text-crono-muted sm:pr-6"><time>{worker.last_seen_at.clone()}</time></td>
                                            </tr>
                                        }
                                    }).collect_view()}
                                </tbody>
                            </table>
                        </div>
                    }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading workers…"</p> }.into_any())}
            </section>
        </div>
    }
}

/// Show one worker's presence and allowlisted runtime facts, including legacy unknowns.
#[component]
pub fn WorkerDetailsPage() -> impl IntoView {
    let params = use_params_map();
    let worker = LocalResource::new(move || {
        let id = params.get().get("worker_id").unwrap_or_default();
        async move { api::get_worker(&id).await }
    });
    view! {
        <div class="space-y-6">
            <PageHeader title="Worker details" description="Heartbeat-backed identity and safe execution diagnostics.">
                <A href="/workers" attr:class="text-sm font-medium text-crono-primary hover:underline">"← All Workers"</A>
            </PageHeader>
            {move || worker.map(|result| match result {
                Ok(details) => {
                    let item = &details.worker;
                    let diagnostics = details.diagnostics.as_ref();
                    let (status, status_class) = match item.status {
                        WorkerStatus::Online => ("Online", "bg-emerald-50 text-emerald-700"),
                        WorkerStatus::Stale => ("Stale", "bg-amber-50 text-amber-700"),
                        WorkerStatus::Offline => ("Offline", "bg-slate-100 text-slate-600"),
                    };
                    view! {
                        <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                            <div class="flex flex-wrap items-center justify-between gap-3"><h2 class="break-all font-mono text-lg font-semibold text-crono-text">{item.worker_id.clone()}</h2><span class=format!("rounded-full px-2.5 py-1 text-xs font-medium {status_class}")>{status}</span></div>
                            <dl class="mt-5 grid gap-4 text-sm sm:grid-cols-2 lg:grid-cols-3">
                                <div><dt class="text-crono-muted">"Queue"</dt><dd>{item.queue.clone()}</dd></div>
                                <div><dt class="text-crono-muted">"Active / capacity"</dt><dd>{format!("{} / {}", item.active_executions, item.concurrency)}</dd></div>
                                <div><dt class="text-crono-muted">"Version"</dt><dd>{item.version.clone()}</dd></div>
                                <div><dt class="text-crono-muted">"Started"</dt><dd>{item.started_at.clone()}</dd></div>
                                <div><dt class="text-crono-muted">"Last seen"</dt><dd>{item.last_seen_at.clone()}</dd></div>
                                {diagnostics.map(|facts| view! {
                                    <div><dt class="text-crono-muted">"Hostname"</dt><dd>{facts.hostname.clone()}</dd></div>
                                    <div><dt class="text-crono-muted">"Platform"</dt><dd>{format!("{} / {}", facts.os, facts.architecture)}</dd></div>
                                    <div><dt class="text-crono-muted">"Default shell"</dt><dd><code>{facts.default_shell_path.clone()}</code>{if facts.default_shell_present { " · present" } else { " · not found" }}</dd></div>
                                    <div><dt class="text-crono-muted">"Worker dry run"</dt><dd>{if facts.dry_run { "On" } else { "Off" }}</dd></div>
                                })}
                            </dl>
                        </section>
                        <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                            <h2 class="font-semibold text-crono-text">"Child environment"</h2>
                            <p class="mt-1 text-sm text-crono-muted">"Crono clears the worker environment before execution. It injects CRONO_RUN_ID and CRONO_INPUTS_FILE; only LANG, LC_ALL, and TZ may be copied. PATH and SHELL are not inherited. Custom Job interpreters are checked when a process starts."</p>
                            {diagnostics.map_or_else(
                                || view! { <p class="mt-3 text-sm text-crono-muted">"This worker has not reported diagnostics; it may be an older version."</p> }.into_any(),
                                |facts| view! { <dl class="mt-4 grid gap-3 text-sm sm:grid-cols-3"><div><dt class="text-crono-muted">"LANG"</dt><dd>{facts.lang.clone().unwrap_or_else(|| "Unset".to_string())}</dd></div><div><dt class="text-crono-muted">"LC_ALL"</dt><dd>{facts.lc_all.clone().unwrap_or_else(|| "Unset".to_string())}</dd></div><div><dt class="text-crono-muted">"TZ"</dt><dd>{facts.tz.clone().unwrap_or_else(|| "Unset".to_string())}</dd></div></dl> }.into_any(),
                            )}
                        </section>
                    }.into_any()
                }
                Err(error) => view! { <p class="rounded-xl border border-crono-border bg-crono-surface p-6 text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading worker…"</p> }.into_any())}
        </div>
    }
}
