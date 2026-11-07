//! Restrained content container for administration pages.

use leptos::prelude::*;

/// Group related page content with a subtle boundary.
#[component]
pub fn Card(#[prop(optional)] class: &'static str, children: Children) -> impl IntoView {
    let classes = if class.is_empty() {
        "rounded-xl border border-border bg-card".to_string()
    } else {
        format!("rounded-xl border border-border bg-card {class}")
    };

    view! { <section class=classes>{children()}</section> }
}
