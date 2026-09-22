//! Consistent empty-resource presentation without fabricated backend data.

use super::Icon;
use crate::navigation::MaterialSymbol;
use leptos::prelude::*;

/// Explain why a content area is empty and optionally render a future action.
#[component]
pub fn EmptyState(
    title: &'static str,
    description: &'static str,
    #[prop(optional)] icon: Option<MaterialSymbol>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="flex min-h-56 flex-col items-center justify-center px-6 py-10 text-center">
            {icon.map(|symbol| view! {
                <span class="mb-4 inline-flex size-10 items-center justify-center rounded-lg bg-zinc-100 text-zinc-500">
                    <Icon symbol class="text-[22px]" />
                </span>
            })}
            <h2 class="text-sm font-semibold text-crono-text">{title}</h2>
            <p class="mt-1 max-w-md text-sm leading-6 text-crono-muted">{description}</p>
            {children.map(|children| view! {
                <div class="mt-5">{children()}</div>
            })}
        </div>
    }
}
