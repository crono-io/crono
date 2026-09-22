//! Structural toolbar above the routed workspace.
//!
//! Search, theme, and identity controls are deliberately non-functional until
//! their contracts exist. Keeping the affordances disabled avoids implying
//! authentication or search behavior while reserving stable layout space for
//! those capabilities.

use crate::{components::Icon, navigation::MaterialSymbol};
use leptos::prelude::*;

/// Render the compact workspace toolbar without introducing application state.
#[component]
pub fn TopBar() -> impl IntoView {
    view! {
        <header class="flex h-16 shrink-0 items-center gap-3 border-b border-crono-border bg-crono-surface px-4 sm:px-6 lg:px-8">
            <button
                type="button"
                disabled
                aria-label="Sidebar menu is not available"
                class="inline-flex size-9 shrink-0 cursor-not-allowed items-center justify-center rounded-md text-zinc-400 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary"
            >
                <Icon symbol=MaterialSymbol::Menu class="text-xl" />
            </button>

            <label class="relative block w-full max-w-96">
                <span class="sr-only">"Search is not available yet"</span>
                <span class="pointer-events-none absolute inset-y-0 left-3 flex items-center text-zinc-400">
                    <Icon symbol=MaterialSymbol::Search class="text-xl" />
                </span>
                <input
                    type="search"
                    readonly
                    placeholder="Search..."
                    aria-label="Search is not available yet"
                    class="h-10 w-full rounded-lg border-0 bg-zinc-100 py-2 pr-3 pl-10 text-sm text-crono-text placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-crono-primary/30"
                />
            </label>

            <div class="ml-auto flex shrink-0 items-center gap-2 sm:gap-3">
                <button
                    type="button"
                    disabled
                    aria-label="Theme controls are not available"
                    class="inline-flex size-9 cursor-not-allowed items-center justify-center rounded-md text-zinc-400 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary"
                >
                    <Icon symbol=MaterialSymbol::LightMode class="text-xl" />
                </button>
                <div class="flex items-center gap-2 border-l border-crono-border pl-3" aria-label="Crono user controls placeholder">
                    <span class="flex size-8 items-center justify-center rounded-full bg-crono-primary-soft text-xs font-semibold text-crono-primary" aria-hidden="true">"C"</span>
                    <span class="hidden text-sm font-medium text-zinc-700 sm:inline">"Crono"</span>
                </div>
            </div>
        </header>
    }
}
