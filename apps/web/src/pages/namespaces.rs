//! Live Namespace collection and creation workflow.

use crate::{api, components::PageHeader};
use leptos::{prelude::*, task::spawn_local};

/// Create and inspect Namespaces through the public API.
#[component]
pub fn NamespacesPage() -> impl IntoView {
    let name = RwSignal::new(String::new());
    let feedback = RwSignal::new(None::<String>);
    let namespaces = LocalResource::new(api::list_namespaces);

    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let requested_name = name.get_untracked();
        if requested_name.is_empty() {
            feedback.set(Some("Enter a Namespace name.".to_string()));
            return;
        }
        feedback.set(Some("Creating Namespace…".to_string()));
        spawn_local(async move {
            match api::create_namespace(requested_name).await {
                Ok(namespace) => {
                    name.set(String::new());
                    feedback.set(Some(format!("Created {}.", namespace.name)));
                    namespaces.refetch();
                }
                Err(error) => feedback.set(Some(error)),
            }
        });
    };

    view! {
        <div class="space-y-8">
            <PageHeader
                title="Namespaces"
                description="Namespaces are the authorization and organization boundary for Jobs and Targets."
            />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Namespace"</h2>
                <form class="mt-4 flex flex-col gap-3 sm:flex-row" on:submit=submit>
                    <label class="grow">
                        <span class="sr-only">"Namespace name"</span>
                        <input
                            class="w-full rounded-md border border-crono-border px-3 py-2 text-sm focus:border-crono-primary focus:outline-none focus:ring-2 focus:ring-crono-primary-soft"
                            placeholder="database-prod"
                            prop:value=move || name.get()
                            on:input=move |event| name.set(event_target_value(&event))
                        />
                    </label>
                    <button class="rounded-md bg-crono-primary px-4 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover" type="submit">
                        "Create"
                    </button>
                </form>
                {move || feedback.get().map(|message| view! {
                    <p class="mt-3 text-sm text-crono-muted" role="status">{message}</p>
                })}
            </section>

            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                    <h2 class="text-base font-semibold text-crono-text">"Available Namespaces"</h2>
                </header>
                {move || namespaces.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Namespaces yet."</p>
                    }.into_any(),
                    Ok(page) => view! {
                        <ul class="divide-y divide-crono-border">
                            {page.items.iter().map(|namespace| view! {
                                <li class="flex items-center justify-between px-5 py-4 sm:px-6">
                                    <span class="font-medium text-crono-text">{namespace.name.clone()}</span>
                                    <code class="text-xs text-crono-muted">{namespace.id.to_string()}</code>
                                </li>
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Err(error) => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.clone()}</p>
                    }.into_any(),
                }).unwrap_or_else(|| view! {
                    <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Namespaces…"</p>
                }.into_any())}
            </section>
        </div>
    }
}
