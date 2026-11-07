//! Root composition for the independently deployed browser client.
//!
//! The Router owns URL state, the `AppShell` owns persistent navigation and
//! responsive layout, and each route supplies only page content. Future API
//! clients will sit below pages and communicate with `crono-server` exclusively
//! over its public HTTPS boundary. The theme state is provided here so every
//! component shares one light/dark signal.

use crate::{components::layout::AppShell, routing::RouterContent, theme::ThemeState};
use leptos::prelude::*;
use leptos_router::components::Router;

/// Mount the client router inside the shared application shell.
#[component]
pub fn App() -> impl IntoView {
    ThemeState::provide();
    view! {
        <Router>
            <AppShell>
                <RouterContent />
            </AppShell>
        </Router>
    }
}
