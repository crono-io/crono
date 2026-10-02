//! Global Queue administration for worker routing.
//!
//! Queue names remain convenient operator lookup keys while every Job and
//! worker relationship submits the immutable UUID. Disabling is reversible;
//! deletion is server-guarded and succeeds only when no durable relationship
//! still references the Queue. Shared modals report creates and edits, while
//! deletion requires explicit confirmation and retains failures for retry.

use crate::{
    api,
    components::{
        DeleteControl, FormActions, PageHeader, QUIET_ACTION_CLASS, ResourceFeedback,
        ResourceFeedbackModal, ResourceNameInput, focus_heading, forms::FIELD_CLASS,
        name_validation_message, visible_name_validation,
    },
};
use crono_api::QueueResource;
use leptos::{prelude::*, task::spawn_local};

/// Create and administer global worker Queues.
#[component]
pub fn QueuesPage() -> impl IntoView {
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let name_server_error = RwSignal::new(None::<String>);
    let description_server_error = RwSignal::new(None::<String>);
    let feedback = RwSignal::new(None::<ResourceFeedback>);
    let deleted = RwSignal::new(None::<String>);
    let heading = NodeRef::<leptos::html::H2>::new();
    let queues = LocalResource::new(api::list_queues);
    let name_error = Signal::derive(move || {
        name_server_error
            .get()
            .or_else(|| visible_name_validation(&name.get(), attempted.get()))
    });
    let description_error = Signal::derive(move || {
        description_server_error
            .get()
            .or_else(|| description_validation(&description.get()))
    });
    let disabled = Signal::derive(move || {
        submitting.get()
            || name_validation_message(&name.get(), true).is_some()
            || description_validation(&description.get()).is_some()
    });
    let reset = Callback::new(move |()| {
        name.set(String::new());
        description.set(String::new());
        attempted.set(false);
        name_server_error.set(None);
        description_server_error.set(None);
        feedback.set(None);
    });
    let submit = Callback::new(move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        if submitting.get_untracked() {
            return;
        }
        attempted.set(true);
        name_server_error.set(None);
        description_server_error.set(None);
        feedback.set(None);
        let requested_name = name.get_untracked();
        let requested_description = description.get_untracked();
        if name_validation_message(&requested_name, true).is_some()
            || description_validation(&requested_description).is_some()
        {
            return;
        }
        submitting.set(true);
        spawn_local(async move {
            match api::create_queue(requested_name, optional_description(requested_description))
                .await
            {
                Ok(queue) => {
                    name.set(String::new());
                    description.set(String::new());
                    attempted.set(false);
                    feedback.set(Some(ResourceFeedback::saved(
                        format!("Created Queue {}.", queue.name),
                        "Create another Queue",
                    )));
                    queues.refetch();
                }
                Err(error) => {
                    queue_create_error(&error, name_server_error, description_server_error);
                    feedback.set(Some(ResourceFeedback::failed(error.message)));
                }
            }
            submitting.set(false);
        });
    });
    let changed = Callback::new(move |()| queues.refetch());
    let on_deleted = Callback::new(move |name: String| {
        deleted.set(Some(format!("Deleted Queue {name}.")));
        queues.refetch();
        focus_heading(heading);
    });
    queue_page(QueuePageState {
        name,
        description,
        feedback,
        name_error,
        description_error,
        disabled,
        reset,
        submit,
        queues,
        changed,
        deleted,
        heading,
        on_deleted,
    })
}

#[derive(Clone, Copy)]
struct QueuePageState {
    name: RwSignal<String>,
    description: RwSignal<String>,
    feedback: RwSignal<Option<ResourceFeedback>>,
    name_error: Signal<Option<String>>,
    description_error: Signal<Option<String>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
    queues: LocalResource<api::ApiResult<crono_api::Page<QueueResource>>>,
    changed: Callback<()>,
    deleted: RwSignal<Option<String>>,
    heading: NodeRef<leptos::html::H2>,
    on_deleted: Callback<String>,
}

fn queue_page(state: QueuePageState) -> impl IntoView {
    view! {
        <div class="space-y-8">
            <PageHeader
                title="Queues"
                description="Queues route Jobs to worker pools. Names are editable; UUID routing keeps existing relationships stable."
            />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Create Queue"</h2>
                <form class="mt-5 space-y-4" on:submit=move |event| state.submit.run(event) novalidate>
                    <ResourceNameInput id="queue-name" label="Name" value=state.name error=state.name_error />
                    <div>
                        <label for="queue-description" class="block text-sm font-medium text-crono-text">"Description"</label>
                        <textarea
                            id="queue-description"
                            class=FIELD_CLASS
                            rows="3"
                            maxlength="500"
                            aria-invalid=move || state.description_error.get().is_some().then_some("true")
                            prop:value=move || state.description.get()
                            on:input=move |event| state.description.set(event_target_value(&event))
                        ></textarea>
                        <p class="mt-1.5 text-xs text-crono-muted">"Optional. Maximum 500 characters."</p>
                        <p class="mt-1 text-sm text-crono-failed" role="alert">{move || state.description_error.get().unwrap_or_default()}</p>
                    </div>
                    <FormActions submit_label="Create Queue" disabled=state.disabled on_cancel=state.reset />
                </form>

            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                    <h2 node_ref=state.heading tabindex="-1" class="font-semibold text-crono-text">"Worker Queues"</h2>
                </header>
                <p class="px-5 text-sm text-crono-muted sm:px-6" role="status">{move || state.deleted.get().unwrap_or_default()}</p>
                {move || state.queues.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Queues exist yet."</p>
                    }.into_any(),
                    Ok(page) => view! {
                        <ul class="divide-y divide-crono-border">
                            {page.items.clone().into_iter().map(|queue| view! {
                                <QueueRow queue=queue on_changed=state.changed on_deleted=state.on_deleted feedback=state.feedback />
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Err(error) => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.message.clone()}</p>
                    }.into_any(),
                }).unwrap_or_else(|| view! {
                    <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Queues…"</p>
                }.into_any())}
            </section>
            <ResourceFeedbackModal id="queue-save-result" resource="Queue" plural="Queues" feedback=state.feedback on_view=Callback::new(move |()| focus_heading(state.heading)) />
        </div>
    }
}

/// Keep API failures in the editor and the page-owned result modal after refresh.
#[component]
fn QueueRow(
    queue: QueueResource,
    on_changed: Callback<()>,
    on_deleted: Callback<String>,
    feedback: RwSignal<Option<ResourceFeedback>>,
) -> impl IntoView {
    let id = queue.id;
    let original_name = StoredValue::new(queue.name.clone());
    let original_description = StoredValue::new(queue.description.clone().unwrap_or_default());
    let original_enabled = queue.enabled;
    let system = queue.system;
    let name = RwSignal::new(queue.name);
    let description = RwSignal::new(queue.description.unwrap_or_default());
    let enabled = RwSignal::new(queue.enabled);
    let editing = RwSignal::new(false);
    let confirming_delete = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let delete_error = RwSignal::new(None::<String>);
    let save = move |_| {
        if busy.get_untracked() {
            return;
        }
        feedback.set(None);
        error.set(None);
        let requested_name = name.get_untracked();
        let requested_description = description.get_untracked();
        if let Some(message) = name_validation_message(&requested_name, true)
            .or_else(|| description_validation(&requested_description))
        {
            error.set(Some(message));
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match api::update_queue(
                id,
                requested_name,
                optional_description(requested_description),
                enabled.get_untracked(),
            )
            .await
            {
                Ok(queue) => {
                    editing.set(false);
                    feedback.set(Some(ResourceFeedback::saved(
                        format!("Saved Queue {}.", queue.name),
                        "Done",
                    )));
                    on_changed.run(());
                }
                Err(api_error) => {
                    error.set(Some(api_error.message.clone()));
                    feedback.set(Some(ResourceFeedback::failed(api_error.message)));
                }
            }
            busy.set(false);
        });
    };
    let on_confirm = Callback::new(move |()| {
        delete_error.set(None);
        busy.set(true);
        spawn_local(async move {
            let result = api::delete_queue(id).await;
            busy.set(false);
            match result {
                Ok(()) => { confirming_delete.set(false); on_deleted.run(original_name.get_value()); }
                Err(api_error) => delete_error.set(Some(if api_error.code == "resource_in_use" {
                    "This Queue is still referenced by Jobs, Run history, or worker presence and cannot be deleted.".to_string()
                } else { api_error.message })),
            }
        });
    });
    let delete_control = move || {
        view! {
            <Show when=move || !system fallback=|| view! { <span class="self-center text-xs text-crono-muted">"Protected system Queue"</span> }>
                <DeleteControl id=format!("delete-queue-{id}") resource="Queue" name=original_name.get_value() description="This permanently removes the Queue and cannot be undone. Queues referenced by Jobs, Run history, or worker presence cannot be deleted." open=confirming_delete busy error=delete_error on_confirm />
            </Show>
        }
    };

    view! {
        <li class="px-5 py-4 sm:px-6">
            <Show
                when=move || editing.get()
                fallback=move || view! {
                    <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                        <div class="min-w-0">
                            <div class="flex items-center gap-2">
                                <span class="font-medium text-crono-text">{move || name.get()}</span>
                                <span class=move || format!("rounded-full px-2 py-0.5 text-xs font-medium {}", if enabled.get() { "bg-emerald-50 text-emerald-700" } else { "bg-zinc-100 text-zinc-600" })>
                                    {move || if enabled.get() { "Enabled" } else { "Disabled" }}
                                </span>
                                <Show when=move || system>
                                    <span class="rounded-full bg-indigo-50 px-2 py-0.5 text-xs font-medium text-indigo-700">"System"</span>
                                </Show>
                            </div>
                            <p class="mt-1 text-sm text-crono-muted">{move || {
                                let value = description.get();
                                if value.is_empty() { "No description".to_string() } else { value }
                            }}</p>
                        </div>
                        <div class="flex flex-wrap items-center gap-3"><button type="button" class=QUIET_ACTION_CLASS on:click=move |_| { error.set(None); editing.set(true); }>"Edit"</button>{delete_control()}</div>
                    </div>
                }
            >
                <div class="space-y-4">
                    <div>
                        <label for=format!("queue-edit-name-{id}") class="block text-sm font-medium text-crono-text">"Name"</label>
                        <input id=format!("queue-edit-name-{id}") class=FIELD_CLASS type="text" disabled=system prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event)) />
                        <Show when=move || system>
                            <p class="mt-1.5 text-xs text-crono-muted">"The system default Queue name is fixed."</p>
                        </Show>
                    </div>
                    <div>
                        <label for=format!("queue-edit-description-{id}") class="block text-sm font-medium text-crono-text">"Description"</label>
                        <textarea id=format!("queue-edit-description-{id}") class=FIELD_CLASS rows="2" maxlength="500" prop:value=move || description.get() on:input=move |event| description.set(event_target_value(&event))></textarea>
                    </div>
                    <label class="flex items-center gap-2 text-sm font-medium text-crono-text">
                        <input type="checkbox" disabled=system prop:checked=move || enabled.get() on:change=move |event| enabled.set(event_target_checked(&event)) />
                        "Enabled for new Jobs"
                    </label>
                    <p class="text-sm text-crono-failed" role="alert">{move || error.get().unwrap_or_default()}</p>
                    <div class="flex flex-wrap justify-between gap-3">
                        {delete_control()}
                        <div class="flex gap-3">
                            <button type="button" class="rounded-md border border-crono-border px-3 py-2 text-sm font-medium text-crono-text hover:bg-zinc-50" disabled=move || busy.get() on:click=move |_| {
                                name.set(original_name.get_value());
                                description.set(original_description.get_value());
                                enabled.set(original_enabled);
                                editing.set(false);
                                confirming_delete.set(false);
                                error.set(None);
                            }>"Cancel"</button>
                            <button type="button" class="rounded-md bg-crono-primary px-3 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover disabled:opacity-50" disabled=move || busy.get() on:click=save>"Save"</button>
                        </div>
                    </div>
                </div>
            </Show>
        </li>
    }
}

fn description_validation(value: &str) -> Option<String> {
    (value.contains('\0') || value.chars().count() > 500)
        .then(|| "Description must be NUL-free and no longer than 500 characters.".to_string())
}

fn optional_description(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

/// Preserve field-specific API guidance alongside the page's error modal.
fn queue_create_error(
    error: &api::ApiError,
    name_error: RwSignal<Option<String>>,
    description_error: RwSignal<Option<String>>,
) {
    match error.field.as_deref() {
        Some("name") => name_error.set(Some(error.message.clone())),
        Some("description") => description_error.set(Some(error.message.clone())),
        _ if error.code == "already_exists" => name_error.set(Some(error.message.clone())),
        _ => {}
    }
}
