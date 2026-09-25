//! Routed page content for the initial Crono information architecture.
//!
//! Pages compose shared visual primitives and render explicit loading, empty,
//! and failure states. All HTTP transport stays in the browser API client so
//! routed components remain focused on user workflows.

mod jobs;
mod monitor;
mod namespaces;
mod overview;
mod queues;
mod resource_options;
mod runs;
mod schedules;
mod settings;
mod target_sets;
mod targets;
mod workers;

pub use jobs::{CreateJobPage, EditJobPage, JobsPage};
pub use monitor::MonitorPage;
pub use namespaces::NamespacesPage;
pub use overview::OverviewPage;
pub use queues::QueuesPage;
pub use runs::{RunDetailsPage, RunJobPage, RunsPage};
pub use schedules::SchedulesPage;
pub use settings::SettingsPage;
pub use target_sets::TargetSetsPage;
pub use targets::TargetsPage;
pub use workers::{WorkerDetailsPage, WorkersPage};

use crate::{
    components::{Card, EmptyState},
    navigation::MaterialSymbol,
};
use leptos::prelude::*;
use leptos_router::components::A;

/// Render a useful fallback without treating an unknown URL as Overview.
#[component]
pub fn NotFoundPage() -> impl IntoView {
    view! {
        <div class="pt-10">
            <Card>
                <EmptyState
                    icon=MaterialSymbol::SearchOff
                    title="Page not found"
                    description="The requested Crono page does not exist."
                >
                    <A
                        href="/"
                        attr:class="inline-flex items-center rounded-md bg-crono-primary px-3 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2"
                    >
                        "Return to overview"
                    </A>
                </EmptyState>
            </Card>
        </div>
    }
}
