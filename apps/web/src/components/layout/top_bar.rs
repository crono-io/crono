//! Structural toolbar above the routed workspace.
//!
//! Theme and identity controls are deliberately non-functional until their
//! contracts exist. Keeping the affordances disabled avoids implying behavior
//! that the application does not yet support.

use crate::{components::Icon, navigation::MaterialSymbol};
use leptos::prelude::*;

/// Render the compact workspace toolbar without introducing application state.
#[component]
pub fn TopBar() -> impl IntoView {
    view! {
        <header class="flex h-16 shrink-0 items-center gap-3 border-b border-crono-border bg-crono-surface px-4 sm:px-6 lg:px-8">
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
