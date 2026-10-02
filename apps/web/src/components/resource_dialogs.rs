//! Consistent resource save outcomes and guarded deletion confirmations.
//!
//! Pages own API requests, authorization-sensitive data, and refresh callbacks.
//! These dialogs present their outcomes without discarding form input. Feedback
//! belongs above refreshed lists so replacing a row cannot dismiss a save result.
//! The native Modal contains focus and blocks Escape during pending deletion.

use super::{Icon, Modal, QUIET_ACTION_CLASS};
use crate::navigation::MaterialSymbol;
use leptos::{prelude::*, task::spawn_local};

/// Style destructive actions quietly until hover, with a visible keyboard focus ring.
pub(crate) const DELETE_ACTION_CLASS: &str = "inline-flex items-center justify-center gap-1.5 rounded-md px-3 py-2 text-sm font-medium text-crono-muted transition-colors hover:bg-red-50 hover:text-crono-failed focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-failed focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-crono-muted";

/// A server-confirmed outcome with a safe continuation and optional resource navigation.
#[derive(Clone)]
pub struct ResourceFeedback {
    pub message: String,
    pub succeeded: bool,
    pub continue_label: &'static str,
    pub view_on_error: bool,
}

impl ResourceFeedback {
    /// Show a committed save result; pages retain their existing reset/edit behavior.
    pub fn saved(message: String, continue_label: &'static str) -> Self {
        Self {
            message,
            succeeded: true,
            continue_label,
            view_on_error: false,
        }
    }

    /// Return to the unchanged form after an API failure.
    pub fn failed(message: String) -> Self {
        Self {
            message,
            succeeded: false,
            continue_label: "Back to form",
            view_on_error: false,
        }
    }
}

/// Present each new outcome once, preserving accessible error text and page-owned actions.
#[component]
pub fn ResourceFeedbackModal(
    id: &'static str,
    resource: &'static str,
    plural: &'static str,
    feedback: RwSignal<Option<ResourceFeedback>>,
    on_view: Callback<()>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    Effect::new(move |_| open.set(feedback.get().is_some()));
    let succeeded = Signal::derive(move || feedback.get().is_some_and(|value| value.succeeded));
    view! {
        <Modal id open title=Signal::derive(move || if succeeded.get() { format!("{resource} saved") } else { format!("{resource} could not be saved") })>
            <div class="flex items-start gap-3">
                <Show when=move || succeeded.get() fallback=|| view! { <Icon symbol=MaterialSymbol::Error class="text-crono-failed" /> }>
                    <Icon symbol=MaterialSymbol::Check class="text-crono-success" />
                </Show>
                <p class="break-words text-sm text-crono-muted" role=move || if succeeded.get() { "status" } else { "alert" }>{move || feedback.get().map(|value| value.message).unwrap_or_default()}</p>
            </div>
            <div class="flex flex-wrap justify-end gap-3">
                <button type="button" class=QUIET_ACTION_CLASS autofocus on:click=move |_| open.set(false)>
                    {move || feedback.get().map_or("Back to form", |value| value.continue_label)}
                </button>
                <Show when=move || feedback.get().is_some_and(|value| value.succeeded || value.view_on_error)>
                    <button type="button" class="rounded-md bg-crono-primary px-4 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2" on:click=move |_| { open.set(false); on_view.run(()); }>{format!("View {plural}")}</button>
                </Show>
            </div>
        </Modal>
    }
}

/// Open a deletion confirmation; only an explicit, idle confirmation invokes the page.
///
/// Pages set `busy` synchronously before requesting deletion and close `open` only
/// after success. Errors remain here, allowing retry or cancellation without a
/// second dialog. Protected resources must omit this control entirely.
#[component]
pub fn DeleteControl(
    #[prop(into)] id: String,
    resource: &'static str,
    #[prop(into)] name: String,
    description: &'static str,
    open: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    on_confirm: Callback<()>,
) -> impl IntoView {
    let name = StoredValue::new(name);
    view! {
        <button type="button" class=DELETE_ACTION_CLASS disabled=move || busy.get() on:click=move |_| { error.set(None); open.set(true); }>
            <Icon symbol=MaterialSymbol::Delete class="text-[18px]" />"Delete"
        </button>
        <Modal id open title=Signal::derive(move || format!("Delete {resource} {}?", name.get_value())) busy=Signal::derive(move || busy.get())>
            <p class="text-sm text-crono-muted">{description}</p>
            <Show when=move || error.get().is_some()>
                <p class="break-words text-sm text-crono-failed" role="alert">{move || error.get().unwrap_or_default()}</p>
            </Show>
            <div class="flex flex-wrap justify-end gap-3">
                <button type="button" class=QUIET_ACTION_CLASS autofocus disabled=move || busy.get() on:click=move |_| open.set(false)>"Cancel"</button>
                <button type="button" class=DELETE_ACTION_CLASS disabled=move || busy.get() on:click=move |_| {
                    if open.get_untracked() && !busy.get_untracked() { on_confirm.run(()); }
                }><Icon symbol=MaterialSymbol::Delete class="text-[18px]" />{move || if busy.get() { "Deleting…" } else { "Confirm delete" }}</button>
            </div>
        </Modal>
    }
}

/// Focus a stable list heading after modal dismissal, including removal of its opener.
pub fn focus_heading(heading: NodeRef<leptos::html::H2>) {
    spawn_local(async move {
        leptos::task::tick().await;
        if let Some(element) = heading.get_untracked() {
            let _ = element.focus();
        }
    });
}

#[cfg(test)]
mod browser_tests {
    use super::{DeleteControl, ResourceFeedback, ResourceFeedbackModal};
    use leptos::prelude::*;
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::wasm_bindgen_test;
    use web_sys::HtmlElement;

    /// Attach a host so native dialogs can enter the browser modal top layer.
    fn host() -> Option<HtmlElement> {
        let document = web_sys::window()?.document()?;
        let element = document
            .create_element("div")
            .ok()?
            .dyn_into::<HtmlElement>()
            .ok()?;
        document.body()?.append_child(&element).ok()?;
        Some(element)
    }

    fn button(host: &HtmlElement, label: &str) -> Option<HtmlElement> {
        let selector = match label {
            "Delete" => "button",
            "Cancel" | "Back to form" => "dialog button[autofocus]",
            _ => "dialog button:not([autofocus])",
        };
        host.query_selector(selector)
            .ok()
            .flatten()?
            .dyn_into::<HtmlElement>()
            .ok()
            .filter(|element| {
                element
                    .text_content()
                    .is_some_and(|text| text.ends_with(label))
            })
    }

    #[wasm_bindgen_test]
    async fn deletion_requires_confirmation_and_retains_failure_for_retry() {
        let host = host();
        assert!(host.is_some());
        let Some(host) = host else {
            return;
        };
        let captured = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&captured);
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let open = RwSignal::new(false);
            let busy = RwSignal::new(false);
            let error = RwSignal::new(None);
            let calls = RwSignal::new(0);
            *capture.borrow_mut() = Some((open, busy, error, calls));
            let on_confirm = Callback::new(move |()| {
                busy.set(true);
                calls.update(|count| *count += 1);
            });
            view! { <DeleteControl id="delete-test" resource="Namespace" name="example" description="Only empty Namespaces can be deleted." open busy error on_confirm /> }
        });
        let signals = *captured.borrow();
        assert!(signals.is_some());
        let Some((open, busy, error, calls)) = signals else {
            return;
        };
        let opener = button(&host, "Delete");
        assert!(opener.is_some());
        if let Some(opener) = opener {
            opener.click();
        }
        leptos::task::tick().await;
        assert!(open.get_untracked());
        assert_eq!(calls.get_untracked(), 0);
        let confirm = button(&host, "Confirm delete");
        assert!(confirm.is_some());
        if let Some(confirm) = confirm {
            confirm.click();
            confirm.click();
        }
        assert_eq!(calls.get_untracked(), 1);
        leptos::task::tick().await;
        assert!(button(&host, "Cancel").is_some_and(|button| button.has_attribute("disabled")));
        busy.set(false);
        error.set(Some("Namespace is still in use".to_string()));
        leptos::task::tick().await;
        assert!(open.get_untracked());
        assert!(
            host.query_selector("dialog[open] [role='alert']")
                .ok()
                .flatten()
                .is_some_and(|element| element
                    .text_content()
                    .is_some_and(|text| text.contains("still in use")))
        );
        if let Some(confirm) = button(&host, "Confirm delete") {
            confirm.click();
        }
        assert_eq!(calls.get_untracked(), 2);
        busy.set(false);
        open.set(false);
        leptos::task::tick().await;
        drop(handle);
        host.remove();
    }

    #[wasm_bindgen_test]
    async fn dismissed_feedback_reopens_for_the_next_outcome_and_view_action() {
        let host = host();
        assert!(host.is_some());
        let Some(host) = host else {
            return;
        };
        let captured = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&captured);
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let feedback = RwSignal::new(Some(ResourceFeedback::failed("Not saved".to_string())));
            let viewed = RwSignal::new(false);
            *capture.borrow_mut() = Some((feedback, viewed));
            view! { <ResourceFeedbackModal id="feedback-test" resource="Queue" plural="Queues" feedback on_view=Callback::new(move |()| viewed.set(true)) /> }
        });
        let signals = *captured.borrow();
        assert!(signals.is_some());
        let Some((feedback, viewed)) = signals else {
            return;
        };
        leptos::task::tick().await;
        assert!(host.query_selector("dialog[open]").ok().flatten().is_some());
        assert!(button(&host, "View Queues").is_none());
        if let Some(back) = button(&host, "Back to form") {
            back.click();
        }
        leptos::task::tick().await;
        assert!(host.query_selector("dialog[open]").ok().flatten().is_none());
        feedback.set(Some(ResourceFeedback::saved(
            "Saved Queue".to_string(),
            "Done",
        )));
        leptos::task::tick().await;
        assert!(
            host.query_selector("dialog[open] [role='status']")
                .ok()
                .flatten()
                .is_some()
        );
        let view = button(&host, "View Queues");
        assert!(view.is_some());
        if let Some(view) = view {
            view.click();
        }
        leptos::task::tick().await;
        assert!(viewed.get_untracked());
        assert!(host.query_selector("dialog[open]").ok().flatten().is_none());
        drop(handle);
        host.remove();
    }
}
