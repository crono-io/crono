//! Live Namespace collection and consistent DNS-1123 creation workflow.

use crate::{
    api,
    components::{
        FormActions, PageHeader, ResourceNameInput, name_validation_message,
        visible_name_validation,
    },
};
use leptos::{prelude::*, task::spawn_local};

/// Create and inspect Namespaces through the public API.
#[component]
pub fn NamespacesPage() -> impl IntoView {
    let name = RwSignal::new(String::new());
    let attempted = RwSignal::new(false);
    let server_error = RwSignal::new(None::<String>);
    let feedback = RwSignal::new(None::<String>);
    let submitting = RwSignal::new(false);
    let namespaces = LocalResource::new(api::list_namespaces);
    let name_error = Signal::derive(move || {
        server_error
            .get()
            .or_else(|| visible_name_validation(&name.get(), attempted.get()))
    });
    let disabled = Signal::derive(move || {
        submitting.get() || name_validation_message(&name.get(), true).is_some()
    });
    let reset = Callback::new(move |()| {
        name.set(String::new());
        attempted.set(false);
        server_error.set(None);
        feedback.set(None);
    });
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        attempted.set(true);
        server_error.set(None);
        feedback.set(None);
        let requested_name = name.get_untracked();
        if name_validation_message(&requested_name, true).is_some() {
            return;
        }
        submitting.set(true);
        spawn_local(async move {
            match api::create_namespace(requested_name).await {
                Ok(namespace) => {
                    name.set(String::new());
                    attempted.set(false);
                    feedback.set(Some(format!("Created {}.", namespace.name)));
                    namespaces.refetch();
                }
                Err(error) => server_error.set(Some(error.message)),
            }
            submitting.set(false);
        });
    };

    view! {
        <div class="space-y-8">
            <PageHeader
                title="Namespaces"
                description="Namespaces are the authorization and organization boundary for Jobs, Targets, and Target Sets."
            />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Namespace"</h2>
                <form class="mt-5 space-y-4" on:submit=submit novalidate>
                    <ResourceNameInput id="namespace-name" label="Name" value=name error=name_error />
                    <FormActions submit_label="Create Namespace" disabled=disabled on_cancel=reset />
                </form>
                <p class="mt-3 text-sm text-crono-success" role="status">{move || feedback.get().unwrap_or_default()}</p>
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
                                <li class="px-5 py-4 font-medium text-crono-text sm:px-6">{namespace.name.clone()}</li>
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Err(error) => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p>
                    }.into_any(),
                }).unwrap_or_else(|| view! {
                    <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Namespaces…"</p>
                }.into_any())}
            </section>
        </div>
    }
}
