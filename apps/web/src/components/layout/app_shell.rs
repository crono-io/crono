//! Responsive control-plane shell for routed content.
//!
//! Persistent product navigation and the structural toolbar surround every
//! route, while page components own only their workspace content. On desktop
//! the shell is exactly one viewport tall and clips its own overflow: the
//! sidebar scrolls only its navigation, the toolbar stays put, and `main`
//! scrolls independently, so no region can spill over another. Below the
//! desktop breakpoint the layout stacks and the page itself scrolls.

use super::{sidebar::Sidebar, top_bar::TopBar};
use leptos::prelude::*;

/// Keep application chrome outside page components and expose one main region.
#[component]
pub fn AppShell(children: Children) -> impl IntoView {
    view! {
        <a
            href="#main-content"
            class="sr-only z-50 rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground focus:not-sr-only focus:fixed focus:left-4 focus:top-4"
        >
            "Skip to main content"
        </a>
        <div class="min-h-screen bg-background lg:grid lg:h-dvh lg:min-h-0 lg:grid-cols-[17rem_minmax(0,1fr)] lg:grid-rows-[minmax(0,1fr)] lg:overflow-hidden">
            <Sidebar />
            <div class="flex min-h-screen min-w-0 flex-col lg:h-full lg:min-h-0">
                <TopBar />
                <main id="main-content" class="min-w-0 flex-1 overflow-x-hidden px-4 py-6 sm:px-6 lg:min-h-0 lg:overflow-y-auto lg:px-8 lg:py-8">
                    <div class="mx-auto w-full max-w-[90rem]">{children()}</div>
                </main>
            </div>
        </div>
    }
}
