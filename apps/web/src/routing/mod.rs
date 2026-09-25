//! URL-to-page composition for the CSR application.
//!
//! Routes select page components only. They do not perform HTTP requests or
//! contain resource behavior, keeping future API access behind a separate
//! client/service boundary.

use crate::pages::{
    CreateJobPage, EditJobPage, JobsPage, MonitorPage, NamespacesPage, NotFoundPage, OverviewPage,
    QueuesPage, RunDetailsPage, RunJobPage, RunsPage, SchedulesPage, SettingsPage, TargetSetsPage,
    TargetsPage, WorkerDetailsPage, WorkersPage,
};
use leptos::prelude::*;
use leptos_router::{
    components::{Route, Routes},
    path,
};

/// Declare every route supported by the initial browser shell.
#[component]
pub fn RouterContent() -> impl IntoView {
    view! {
        <Routes fallback=NotFoundPage>
            <Route path=path!("/") view=OverviewPage />
            <Route path=path!("/namespaces") view=NamespacesPage />
            <Route path=path!("/queues") view=QueuesPage />
            <Route path=path!("/jobs") view=JobsPage />
            <Route path=path!("/jobs/new") view=CreateJobPage />
            <Route path=path!("/jobs/:job_id/edit") view=EditJobPage />
            <Route path=path!("/targets") view=TargetsPage />
            <Route path=path!("/target-sets") view=TargetSetsPage />
            <Route path=path!("/schedules") view=SchedulesPage />
            <Route path=path!("/runs") view=RunsPage />
            <Route path=path!("/runs/new") view=RunJobPage />
            <Route path=path!("/runs/:run_id") view=RunDetailsPage />
            <Route path=path!("/workers") view=WorkersPage />
            <Route path=path!("/workers/:worker_id") view=WorkerDetailsPage />
            <Route path=path!("/monitor") view=MonitorPage />
            <Route path=path!("/settings") view=SettingsPage />
        </Routes>
    }
}
