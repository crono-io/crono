//! URL-to-page composition for the CSR application.
//!
//! Routes select page components only. They do not perform HTTP requests or
//! contain resource behavior, keeping future API access behind a separate
//! client/service boundary.

use crate::pages::{
    JobsPage, NamespacesPage, NotFoundPage, OverviewPage, QueuesPage, RunsPage, SettingsPage,
    TargetSetsPage, TargetsPage, WorkersPage,
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
            <Route path=path!("/targets") view=TargetsPage />
            <Route path=path!("/target-sets") view=TargetSetsPage />
            <Route path=path!("/runs") view=RunsPage />
            <Route path=path!("/workers") view=WorkersPage />
            <Route path=path!("/settings") view=SettingsPage />
        </Routes>
    }
}
