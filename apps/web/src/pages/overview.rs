//! Live control-plane overview with resource summaries and recent activity.

use super::runs::run_status;
use crate::{
    api,
    components::{Icon, PageHeader, QUIET_ACTION_CLASS},
    navigation::AppRoute,
};
use crono_api::WorkerStatus;
use leptos::prelude::*;
use leptos_router::components::A;

/// Present visible catalog counts, recent Runs, and worker presence.
#[component]
pub fn OverviewPage() -> impl IntoView {
    let overview = LocalResource::new(api::overview);
    let runs = LocalResource::new(api::list_runs);
    let workers = LocalResource::new(api::list_workers);
    view! {
        <div class="space-y-8">
            <PageHeader title="Overview" description="Test the Crono control plane from Namespace creation through durable NATS dispatch." />
            {move || overview.map(|result| match result {
                Ok(counts) => view! {
                    <section aria-label="Resource summaries" class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                        <LiveSummary route=AppRoute::Namespaces count=counts.namespaces />
                        <LiveSummary route=AppRoute::Jobs count=counts.jobs />
                        <LiveSummary route=AppRoute::Targets count=counts.targets />
                        <LiveSummary route=AppRoute::Runs count=counts.runs />
                    </section>
                }.into_any(),
                Err(error) => view! {
                    <section class="rounded-xl border border-border bg-card px-6 py-10 text-center">
                        <p class="text-sm text-crono-failed">{error.message.clone()}</p>
                    </section>
                }.into_any(),
            }).unwrap_or_else(|| view! {
                <section class="rounded-xl border border-border bg-card px-6 py-10 text-center text-sm text-muted-foreground">"Loading control-plane counts…"</section>
            }.into_any())}

            <div class="grid gap-6 xl:grid-cols-2">
                <section class="overflow-hidden rounded-xl border border-border bg-card">
                    <header class="flex items-center justify-between border-b border-border px-5 py-4 sm:px-6">
                        <h2 class="font-semibold text-foreground">"Recent Runs"</h2>
                        <A href=AppRoute::Runs.path() attr:class=QUIET_ACTION_CLASS><Icon icon=AppRoute::Runs.symbol() size=18 />"View all"</A>
                    </header>
                    {move || runs.map(|result| match result {
                        Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-muted-foreground">"No Runs yet."</p> }.into_any(),
                        Ok(page) => view! { <ul class="divide-y divide-border">{page.items.iter().take(5).map(|run| view! {
                            <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6">
                                <div class="min-w-0"><p class="truncate text-sm font-medium text-foreground">{format!("{} → {}", run.job, run.target)}</p><p class="mt-1 truncate text-xs text-muted-foreground">{run.created_at.clone()}</p></div>
                                <span class="shrink-0 rounded-full bg-primary-soft px-2.5 py-1 text-xs font-medium text-primary">{run_status(run.status)}</span>
                            </li>
                        }).collect_view()}</ul> }.into_any(),
                        Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                    }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-muted-foreground">"Loading recent Runs…"</p> }.into_any())}
                </section>

                <section class="overflow-hidden rounded-xl border border-border bg-card">
                    <header class="flex items-center justify-between border-b border-border px-5 py-4 sm:px-6">
                        <h2 class="font-semibold text-foreground">"Workers"</h2>
                        <A href=AppRoute::Workers.path() attr:class=QUIET_ACTION_CLASS><Icon icon=AppRoute::Workers.symbol() size=18 />"View all"</A>
                    </header>
                    {move || workers.map(|result| match result {
                        Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-muted-foreground">"No workers have reported presence."</p> }.into_any(),
                        Ok(page) => view! { <ul class="divide-y divide-border">{page.items.iter().take(5).map(|worker| {
                            let (status, class) = match worker.status {
                                WorkerStatus::Online => ("online", "bg-emerald-50 text-emerald-700"),
                                WorkerStatus::Stale => ("stale", "bg-amber-50 text-amber-700"),
                                WorkerStatus::Offline => ("offline", "bg-muted text-muted-foreground"),
                            };
                            view! {
                                <li class="flex items-center justify-between gap-4 px-5 py-4 sm:px-6">
                                    <div class="min-w-0"><p class="truncate text-sm font-medium text-foreground">{worker.worker_id.clone()}</p><p class="mt-1 text-xs text-muted-foreground">{format!("{} · {} / {} active", worker.queue, worker.active_executions, worker.concurrency)}</p></div>
                                    <span class=format!("shrink-0 rounded-full px-2.5 py-1 text-xs font-medium {class}")>{status}</span>
                                </li>
                            }
                        }).collect_view()}</ul> }.into_any(),
                        Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p> }.into_any(),
                    }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-muted-foreground">"Loading workers…"</p> }.into_any())}
                </section>
            </div>

            <section class="rounded-xl border border-border bg-card p-6">
                <h2 class="font-semibold text-foreground">"Development authorization"</h2>
                <p class="mt-2 max-w-3xl text-sm leading-6 text-muted-foreground">"Every request currently receives the server-owned development/local principal. The same capability checks and visibility scopes are already exercised, while the active policy grants all operations."</p>
            </section>
        </div>
    }
}

#[component]
fn LiveSummary(route: AppRoute, count: u64) -> impl IntoView {
    view! {
        <A href=route.path() attr:class="group rounded-xl border border-border bg-card p-5 transition hover:border-primary/40 hover:shadow-sm">
            <div class="flex items-start justify-between gap-4">
                <div><p class="text-sm font-medium text-muted-foreground">{route.label()}</p><p class="mt-3 text-3xl font-bold text-foreground">{count}</p></div>
                <span class="inline-flex size-10 items-center justify-center rounded-lg bg-primary-soft text-primary transition group-hover:bg-primary/20">
                    <Icon icon=route.symbol() size=20 />
                </span>
            </div>
        </A>
    }
}
