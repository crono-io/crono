//! Live worker presence reported through the server control boundary.

use crate::{
    api,
    components::{EmptyState, PageHeader},
    navigation::MaterialSymbol,
};
use crono_api::WorkerStatus;
use leptos::prelude::*;

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
                                    {page.items.iter().map(|worker| {
                                        let (status, status_class) = match worker.status {
                                            WorkerStatus::Online => ("online", "bg-emerald-50 text-emerald-700"),
                                            WorkerStatus::Stale => ("stale", "bg-amber-50 text-amber-700"),
                                            WorkerStatus::Offline => ("offline", "bg-slate-100 text-slate-600"),
                                        };
                                        view! {
                                            <tr>
                                                <td class="px-5 py-4 font-medium text-crono-text sm:px-6"><code>{worker.worker_id.clone()}</code></td>
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
