//! Namespace-scoped Target creation and inspection.

use crate::{api, components::PageHeader};
use leptos::{prelude::*, task::spawn_local};

/// Create identity-only Targets without premature executor configuration.
#[component]
pub fn TargetsPage() -> impl IntoView {
    let namespace = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let feedback = RwSignal::new(None::<String>);
    let targets = LocalResource::new(move || {
        let selected = namespace.get();
        async move {
            if selected.is_empty() {
                Ok(crono_api::Page {
                    items: Vec::new(),
                    next_cursor: None,
                })
            } else {
                api::list_targets(&selected).await
            }
        }
    });
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let selected = namespace.get_untracked();
        let target_name = name.get_untracked();
        if selected.is_empty() || target_name.is_empty() {
            feedback.set(Some("Enter a Namespace and Target name.".to_string()));
            return;
        }
        feedback.set(Some("Creating Target…".to_string()));
        spawn_local(async move {
            match api::create_target(&selected, target_name).await {
                Ok(target) => {
                    name.set(String::new());
                    feedback.set(Some(format!("Created {}.", target.qualified_name)));
                    targets.refetch();
                }
                Err(error) => feedback.set(Some(error)),
            }
        });
    };

    view! {
        <div class="space-y-8">
            <PageHeader title="Targets" description="Targets identify where or against what a Job executes, without coupling that identity to an executor." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Target"</h2>
                <form class="mt-4 grid gap-3 sm:grid-cols-[1fr_1fr_auto]" on:submit=submit>
                    <input class="rounded-md border border-crono-border px-3 py-2 text-sm" placeholder="Namespace" prop:value=move || namespace.get() on:input=move |event| namespace.set(event_target_value(&event)) />
                    <input class="rounded-md border border-crono-border px-3 py-2 text-sm" placeholder="Target name" prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event)) />
                    <button class="rounded-md bg-crono-primary px-4 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover" type="submit">"Create"</button>
                </form>
                {move || feedback.get().map(|message| view! { <p class="mt-3 text-sm text-crono-muted" role="status">{message}</p> })}
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Targets in Namespace"</h2></header>
                {move || targets.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Enter a Namespace or create its first Target."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|target| view! {
                        <li class="flex items-center justify-between px-5 py-4 sm:px-6">
                            <span class="font-medium text-crono-text">{target.qualified_name.clone()}</span>
                            <code class="text-xs text-crono-muted">{target.id.to_string()}</code>
                        </li>
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Targets…"</p> }.into_any())}
            </section>
        </div>
    }
}
