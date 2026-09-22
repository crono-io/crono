//! Responsive control-plane shell for routed content.
//!
//! Persistent product navigation and the structural toolbar surround every
//! route, while page components own only their workspace content. The sidebar
//! is fixed in the desktop grid and the workspace remains independently
//! scrollable, so navigation never has to be recreated by individual pages.

use super::{sidebar::Sidebar, top_bar::TopBar};
use leptos::prelude::*;

/// Keep application chrome outside page components and expose one main region.
#[component]
pub fn AppShell(children: Children) -> impl IntoView {
    view! {
        <a
            href="#main-content"
            class="sr-only z-50 rounded-md bg-crono-sidebar px-3 py-2 text-sm font-medium text-white focus:not-sr-only focus:fixed focus:left-4 focus:top-4"
        >
            "Skip to main content"
        </a>
        <div class="min-h-screen bg-crono-bg lg:grid lg:grid-cols-[17rem_minmax(0,1fr)]">
            <Sidebar />
            <div class="flex min-h-screen min-w-0 flex-col">
                <TopBar />
                <main id="main-content" class="min-w-0 flex-1 overflow-x-hidden px-4 py-6 sm:px-6 lg:px-8 lg:py-8">
                    <div class="mx-auto w-full max-w-[90rem]">{children()}</div>
                </main>
            </div>
        </div>
    }
}
