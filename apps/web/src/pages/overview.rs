//! Operational landing page built entirely from truthful empty states.
//!
//! The overview establishes Crono's dashboard hierarchy without inventing
//! metrics or backend responses. Resource counts remain zero until a public API
//! client supplies real values, and all operational panels explain their empty
//! state explicitly.

use crate::{
    components::{Card, EmptyState, Icon, PageHeader, SummaryCard, SummaryTone},
    navigation::{AppRoute, MaterialSymbol},
};
use leptos::prelude::*;
use leptos_router::components::A;

/// Present the initial resource, execution, and product overview.
#[component]
pub fn OverviewPage() -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Overview"
                description="Distributed workload automation for infrastructure and operations."
            />

            <section aria-label="Resource summaries" class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                <SummaryCard
                    route=AppRoute::Namespaces
                    count=0
                    empty_text="No namespaces yet"
                    tone=SummaryTone::Indigo
                />
                <SummaryCard
                    route=AppRoute::Jobs
                    count=0
                    empty_text="No jobs yet"
                    tone=SummaryTone::Sky
                />
                <SummaryCard
                    route=AppRoute::Targets
                    count=0
                    empty_text="No targets yet"
                    tone=SummaryTone::Teal
                />
                <SummaryCard
                    route=AppRoute::TargetSets
                    count=0
                    empty_text="No target sets yet"
                    tone=SummaryTone::Violet
                />
            </section>

            <section aria-label="Operational status" class="grid gap-6 xl:grid-cols-2">
                <OperationalCard
                    title="Recent Runs"
                    route=AppRoute::Runs
                    icon=MaterialSymbol::PlayCircle
                    empty_title="No runs yet"
                    empty_description="Runs will appear here when jobs are executed."
                />
                <OperationalCard
                    title="Workers"
                    route=AppRoute::Workers
                    icon=MaterialSymbol::Memory
                    empty_title="No workers yet"
                    empty_description="Workers will appear here when they register with Crono."
                />
            </section>

            <section aria-label="Crono guidance" class="grid gap-6 xl:grid-cols-2">
                <GettingStarted />
                <CronoInformation />
            </section>
        </div>
    }
}

/// Pair a compact panel heading with a centered, honest empty state.
#[component]
fn OperationalCard(
    title: &'static str,
    route: AppRoute,
    icon: MaterialSymbol,
    empty_title: &'static str,
    empty_description: &'static str,
) -> impl IntoView {
    view! {
        <Card>
            <header class="flex items-center justify-between border-b border-crono-border px-5 py-4 sm:px-6">
                <h2 class="text-base font-semibold text-crono-text">{title}</h2>
                <A
                    href=route.path()
                    attr:class="rounded-sm text-sm font-medium text-crono-primary hover:text-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary"
                >
                    "View all"
                </A>
            </header>
            <EmptyState icon title=empty_title description=empty_description />
        </Card>
    }
}

/// Explain the intended first-use sequence without creating inactive controls.
#[component]
fn GettingStarted() -> impl IntoView {
    let steps = [
        (
            "1",
            "Create a namespace",
            "Organize related jobs and targets.",
        ),
        ("2", "Define jobs", "Configure what to execute."),
        ("3", "Add targets", "Define where to run your jobs."),
        (
            "4",
            "Execute a run",
            "Run a job against a target or target set.",
        ),
    ];

    view! {
        <Card class="p-5 sm:p-6">
            <header class="flex items-center gap-2.5">
                <Icon symbol=MaterialSymbol::RocketLaunch class="text-xl text-crono-primary" />
                <h2 class="text-base font-semibold text-crono-text">"Getting started"</h2>
            </header>
            <ol class="mt-6 space-y-5">
                {steps.into_iter().map(|(number, title, description)| view! {
                    <li class="flex gap-3.5">
                        <span class="flex size-7 shrink-0 items-center justify-center rounded-full bg-crono-primary-soft text-xs font-semibold text-crono-primary">{number}</span>
                        <div>
                            <h3 class="text-sm font-semibold text-crono-text">{title}</h3>
                            <p class="mt-0.5 text-sm leading-6 text-crono-muted">{description}</p>
                        </div>
                    </li>
                }).collect_view()}
            </ol>
        </Card>
    }
}

/// Describe the product qualities that guide future UI and API work.
#[component]
fn CronoInformation() -> impl IntoView {
    let qualities = [
        (
            MaterialSymbol::Tune,
            "Executor agnostic",
            "Run automation with your preferred tools.",
        ),
        (
            MaterialSymbol::AccountTree,
            "Organize at scale",
            "Namespaces, jobs, targets and target sets.",
        ),
        (
            MaterialSymbol::Visibility,
            "Observable",
            "Track runs and outcomes.",
        ),
        (
            MaterialSymbol::Shield,
            "Built for operations",
            "Designed for reliable, repeatable execution.",
        ),
    ];

    view! {
        <Card class="p-5 sm:p-6">
            <header class="flex items-center gap-2.5">
                <Icon symbol=MaterialSymbol::Schedule class="text-xl text-crono-primary" />
                <h2 class="text-base font-semibold text-crono-text">"Crono"</h2>
            </header>
            <p class="mt-4 max-w-xl text-sm leading-6 text-crono-muted">
                "Crono helps you run infrastructure automation at scale in a consistent and auditable way."
            </p>
            <ul class="mt-6 grid gap-x-6 gap-y-5 sm:grid-cols-2">
                {qualities.into_iter().map(|(symbol, title, description)| view! {
                    <li class="flex gap-3">
                        <Icon symbol class="mt-0.5 text-xl text-zinc-400" />
                        <div>
                            <h3 class="text-sm font-semibold text-crono-text">{title}</h3>
                            <p class="mt-0.5 text-sm leading-6 text-crono-muted">{description}</p>
                        </div>
                    </li>
                }).collect_view()}
            </ul>
        </Card>
    }
}
