//! Consistent empty-resource presentation without fabricated backend data.

use super::Icon;
use crate::navigation::AppIcon;
use leptos::prelude::*;

/// Explain why a content area is empty and optionally render a future action.
#[component]
pub fn EmptyState(
    title: &'static str,
    description: &'static str,
    #[prop(optional)] icon: Option<AppIcon>,
    #[prop(optional)] children: Option<Children>,
) -> impl IntoView {
    view! {
        <div class="flex min-h-56 flex-col items-center justify-center px-6 py-10 text-center">
            {icon.map(|icon| view! {
                <span class="mb-4 inline-flex size-10 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                    <Icon icon size=20 />
                </span>
            })}
            <h2 class="text-sm font-semibold text-foreground">{title}</h2>
            <p class="mt-1 max-w-md text-sm leading-6 text-muted-foreground">{description}</p>
            {children.map(|children| view! {
                <div class="mt-5">{children()}</div>
            })}
        </div>
    }
}
