//! Live control-plane overview backed by authorized API counts.

use crate::{api, components::PageHeader, navigation::AppRoute};
use leptos::prelude::*;
use leptos_router::components::A;

/// Present visible catalog and Run counts with links into each workflow.
#[component]
pub fn OverviewPage() -> impl IntoView {
    let overview = LocalResource::new(api::overview);
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
                    <section class="rounded-xl border border-crono-border bg-crono-surface px-6 py-10 text-center">
                        <p class="text-sm text-crono-failed">{error.clone()}</p>
                    </section>
                }.into_any(),
            }).unwrap_or_else(|| view! {
                <section class="rounded-xl border border-crono-border bg-crono-surface px-6 py-10 text-center text-sm text-crono-muted">"Loading control-plane counts…"</section>
            }.into_any())}
            <section class="rounded-xl border border-crono-border bg-crono-surface p-6">
                <h2 class="font-semibold text-crono-text">"Development authorization"</h2>
                <p class="mt-2 max-w-3xl text-sm leading-6 text-crono-muted">"Every request currently receives the server-owned development/local principal. The same capability checks and visibility scopes are already exercised, while the active policy grants all operations."</p>
            </section>
        </div>
    }
}

#[component]
fn LiveSummary(route: AppRoute, count: u64) -> impl IntoView {
    view! {
        <A href=route.path() attr:class="rounded-xl border border-crono-border bg-crono-surface p-5 transition hover:border-indigo-200 hover:shadow-sm">
            <p class="text-sm font-medium text-crono-muted">{route.label()}</p>
            <p class="mt-3 text-3xl font-bold text-crono-text">{count}</p>
        </A>
    }
}
