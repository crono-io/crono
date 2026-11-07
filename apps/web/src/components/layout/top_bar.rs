//! Toolbar above the routed workspace.
//!
//! Hosts the light/dark theme switch, which reads and writes the shared
//! [`ThemeState`]. Identity controls remain a non-interactive placeholder until
//! an authentication contract exists.

use crate::{
    components::Icon,
    navigation::AppIcon,
    theme::{Theme, ThemeState},
};
use leptos::prelude::*;

/// Render the compact workspace toolbar with the theme switch.
#[component]
pub fn TopBar() -> impl IntoView {
    let state = ThemeState::current();
    let theme = state.theme;
    view! {
        <header class="flex h-16 shrink-0 items-center gap-3 border-b border-border bg-card px-4 sm:px-6 lg:px-8">
            <div class="ml-auto flex shrink-0 items-center gap-2 sm:gap-3">
                <button
                    type="button"
                    aria-label=move || match theme.get() {
                        Theme::Light => "Switch to dark theme",
                        Theme::Dark => "Switch to light theme",
                    }
                    on:click=move |_| state.toggle()
                    class="inline-flex size-9 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-card"
                >
                    {move || match theme.get() {
                        Theme::Light => view! { <Icon icon=AppIcon::Sun /> }.into_any(),
                        Theme::Dark => view! { <Icon icon=AppIcon::Moon /> }.into_any(),
                    }}
                </button>
                <div class="flex items-center gap-2 border-l border-border pl-3" aria-label="Crono user controls placeholder">
                    <span class="flex size-8 items-center justify-center rounded-full bg-primary-soft text-xs font-semibold text-primary" aria-hidden="true">"C"</span>
                    <span class="hidden text-sm font-medium text-foreground sm:inline">"Crono"</span>
                </div>
            </div>
        </header>
    }
}
