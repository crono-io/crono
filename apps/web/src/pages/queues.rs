//! Global Queue administration for worker routing.
//!
//! Queue names remain convenient operator lookup keys while every Job and
//! worker relationship submits the immutable UUID. Disabling is reversible;
//! deletion is server-guarded and succeeds only when no durable relationship
//! still references the Queue.

use crate::{
    api,
    components::{
        FormActions, PageHeader, ResourceNameInput, name_validation_message,
        visible_name_validation,
    },
};
use crono_api::QueueResource;
use leptos::{prelude::*, task::spawn_local};

const INPUT_CLASS: &str = "w-full rounded-md border border-crono-border bg-white px-3 py-2.5 text-sm text-crono-text shadow-sm outline-none transition placeholder:text-zinc-400 focus:border-crono-primary focus:ring-2 focus:ring-crono-primary-soft disabled:cursor-not-allowed disabled:bg-zinc-50 disabled:text-zinc-500";

/// Create and administer global worker Queues.
#[component]
pub fn QueuesPage() -> impl IntoView {
    let name = RwSignal::new(String::new());
    let description = RwSignal::new(String::new());
    let attempted = RwSignal::new(false);
    let submitting = RwSignal::new(false);
    let name_server_error = RwSignal::new(None::<String>);
    let description_server_error = RwSignal::new(None::<String>);
    let feedback = RwSignal::new(None::<String>);
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
                    feedback.set(Some(format!("Created Queue {}.", queue.name)));
                    queues.refetch();
                }
                Err(error) => match error.field.as_deref() {
                    Some("name") => name_server_error.set(Some(error.message)),
                    Some("description") => description_server_error.set(Some(error.message)),
                    _ => feedback.set(Some(error.message)),
                },
            }
            submitting.set(false);
        });
    });
    let changed = Callback::new(move |()| queues.refetch());
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
    })
}

#[derive(Clone, Copy)]
struct QueuePageState {
    name: RwSignal<String>,
    description: RwSignal<String>,
    feedback: RwSignal<Option<String>>,
    name_error: Signal<Option<String>>,
    description_error: Signal<Option<String>>,
    disabled: Signal<bool>,
    reset: Callback<()>,
    submit: Callback<leptos::ev::SubmitEvent>,
    queues: LocalResource<api::ApiResult<crono_api::Page<QueueResource>>>,
    changed: Callback<()>,
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
                            class=INPUT_CLASS
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
                <p class="mt-3 text-sm text-crono-muted" role="status">{move || state.feedback.get().unwrap_or_default()}</p>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                    <h2 class="font-semibold text-crono-text">"Worker Queues"</h2>
                </header>
                {move || state.queues.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! {
                        <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Queues exist yet."</p>
                    }.into_any(),
                    Ok(page) => view! {
                        <ul class="divide-y divide-crono-border">
                            {page.items.clone().into_iter().map(|queue| view! {
                                <QueueRow queue=queue on_changed=state.changed />
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
        </div>
    }
}

#[component]
fn QueueRow(queue: QueueResource, on_changed: Callback<()>) -> impl IntoView {
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
    let save = move |_| {
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
                Ok(_) => {
                    editing.set(false);
                    on_changed.run(());
                }
                Err(api_error) => error.set(Some(api_error.message)),
            }
            busy.set(false);
        });
    };
    let delete = move |_| {
        if !confirming_delete.get_untracked() {
            confirming_delete.set(true);
            return;
        }
        error.set(None);
        busy.set(true);
        spawn_local(async move {
            match api::delete_queue(id).await {
                Ok(()) => on_changed.run(()),
                Err(api_error) => {
                    error.set(Some(api_error.message));
                    confirming_delete.set(false);
                }
            }
            busy.set(false);
        });
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
                        <button type="button" class="self-start rounded-md border border-crono-border px-3 py-2 text-sm font-medium text-crono-text hover:bg-zinc-50" on:click=move |_| editing.set(true)>"Edit"</button>
                    </div>
                }
            >
                <div class="space-y-4">
                    <div>
                        <label class="block text-sm font-medium text-crono-text">"Name"</label>
                        <input class=INPUT_CLASS type="text" disabled=system prop:value=move || name.get() on:input=move |event| name.set(event_target_value(&event)) />
                        <Show when=move || system>
                            <p class="mt-1.5 text-xs text-crono-muted">"The system default Queue name is fixed."</p>
                        </Show>
                    </div>
                    <div>
                        <label class="block text-sm font-medium text-crono-text">"Description"</label>
                        <textarea class=INPUT_CLASS rows="2" maxlength="500" prop:value=move || description.get() on:input=move |event| description.set(event_target_value(&event))></textarea>
                    </div>
                    <label class="flex items-center gap-2 text-sm font-medium text-crono-text">
                        <input type="checkbox" disabled=system prop:checked=move || enabled.get() on:change=move |event| enabled.set(event_target_checked(&event)) />
                        "Enabled for new Jobs"
                    </label>
                    <p class="text-sm text-crono-failed" role="alert">{move || error.get().unwrap_or_default()}</p>
                    <div class="flex flex-wrap justify-between gap-3">
                        <Show
                            when=move || !system
                            fallback=move || view! { <span class="self-center text-xs text-crono-muted">"Protected system Queue"</span> }
                        >
                            <button
                                type="button"
                                class=move || format!("rounded-md border px-3 py-2 text-sm font-medium {}", if confirming_delete.get() { "border-crono-failed bg-red-50 text-crono-failed" } else { "border-crono-border text-crono-failed hover:bg-red-50" })
                                disabled=move || busy.get()
                                on:click=delete
                            >
                                {move || if confirming_delete.get() { "Confirm delete" } else { "Delete" }}
                            </button>
                        </Show>
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
