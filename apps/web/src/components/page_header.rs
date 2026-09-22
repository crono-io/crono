//! Shared page title, description, and optional action placement.

use leptos::prelude::*;

/// Render a page's primary heading and optional action content.
#[component]
pub fn PageHeader(
    title: &'static str,
    description: &'static str,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <header class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div>
                <h1 class="text-3xl font-bold tracking-tight text-crono-text">{title}</h1>
                <p class="mt-2 max-w-3xl text-base leading-7 text-crono-muted">{description}</p>
            </div>
            {children.map(|children| view! {
                <div class="shrink-0">{children()}</div>
            })}
        </header>
    }
}
