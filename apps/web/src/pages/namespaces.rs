//! Live Namespace creation and guarded deletion through the public API.
//!
//! Save results use the shared modal while failed requests retain entered names.
//! The default Namespace is visibly protected; the server authorizes deletion
//! and atomically rejects dependencies. Successful deletion refreshes the list
//! and focuses its stable heading rather than the removed row.

use crate::{
    api,
    components::{
        DeleteControl, FormActions, PageHeader, ResourceFeedback, ResourceFeedbackModal,
        ResourceNameInput, focus_heading, name_validation_message, visible_name_validation,
    },
};
use leptos::{prelude::*, task::spawn_local};

/// Create and inspect Namespaces through the public API.
#[component]
pub fn NamespacesPage() -> impl IntoView {
    let name = RwSignal::new(String::new());
    let attempted = RwSignal::new(false);
    let server_error = RwSignal::new(None::<String>);
    let feedback = RwSignal::new(None::<ResourceFeedback>);
    let deleted = RwSignal::new(None::<String>);
    let heading = NodeRef::<leptos::html::H2>::new();
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
        if submitting.get_untracked() {
            return;
        }
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
                    feedback.set(Some(ResourceFeedback::saved(
                        format!("Created Namespace {}.", namespace.name),
                        "Create another Namespace",
                    )));
                    namespaces.refetch();
                }
                Err(error) => {
                    if error.field.as_deref() == Some("name") || error.code == "already_exists" {
                        server_error.set(Some(error.message.clone()));
                    }
                    feedback.set(Some(ResourceFeedback::failed(error.message)));
                }
            }
            submitting.set(false);
        });
    };
    let on_deleted = Callback::new(move |name: String| {
        deleted.set(Some(format!("Deleted Namespace {name}.")));
        namespaces.refetch();
        focus_heading(heading);
    });

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
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                    <h2 node_ref=heading tabindex="-1" class="text-base font-semibold text-crono-text">"Available Namespaces"</h2>
                </header>
                <p class="px-5 text-sm text-crono-muted sm:px-6" role="status">{move || deleted.get().unwrap_or_default()}</p>
                {move || namespaces.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Namespaces yet."</p>
                    }.into_any(),
                    Ok(page) => view! {
                        <ul class="divide-y divide-crono-border">
                            {page.items.iter().cloned().map(|namespace| view! {
                                <NamespaceRow namespace on_deleted />
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
            <ResourceFeedbackModal id="namespace-save-result" resource="Namespace" plural="Namespaces" feedback on_view=Callback::new(move |()| focus_heading(heading)) />
        </div>
    }
}

/// Submit only confirmed UUID deletion, retaining dependency failures inside the modal.
#[component]
fn NamespaceRow(
    namespace: crono_api::NamespaceResource,
    on_deleted: Callback<String>,
) -> impl IntoView {
    let id = namespace.id;
    let protected = namespace.name == "default";
    let name = StoredValue::new(namespace.name);
    let open = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None);
    let on_confirm = Callback::new(move |()| {
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            let result = api::delete_namespace(id).await;
            busy.set(false);
            match result {
                Ok(()) => { open.set(false); on_deleted.run(name.get_value()); }
                Err(failure) => error.set(Some(if failure.code == "resource_in_use" {
                    "This Namespace still contains resources or Run requests and cannot be deleted. Remove its resources first.".to_string()
                } else { failure.message })),
            }
        });
    });
    view! {
        <li class="flex flex-wrap items-center justify-between gap-4 px-5 py-4 sm:px-6">
            <p class="min-w-0 break-words font-medium text-crono-text">{name.get_value()}</p>
            <Show when=move || !protected fallback=|| view! { <span class="text-xs text-crono-muted">"Protected default Namespace"</span> }>
                <DeleteControl id=format!("delete-namespace-{id}") resource="Namespace" name=name.get_value() description="This permanently removes the empty Namespace and cannot be undone. Namespaces containing resources or Run requests cannot be deleted." open busy error on_confirm />
            </Show>
        </li>
    }
}
