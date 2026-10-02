//! Native modal dialogs shared by resource workflows.
//!
//! The browser's modal top layer dims and blocks the surrounding page, contains
//! keyboard focus, and returns focus to the opener on close. Pages own their
//! messages and actions; this component synchronizes native open/close events
//! with reactive state. Escape is blocked while an API request is pending.

use leptos::prelude::*;

/// Show a labeled modal with browser-managed focus and guarded Escape dismissal.
///
/// Children should provide a visible close or Cancel button, with autofocus on
/// the safe initial action. `busy` prevents Escape from hiding an unfinished request.
#[component]
pub fn Modal(
    #[prop(into)] id: String,
    open: RwSignal<bool>,
    #[prop(into)] title: Signal<String>,
    #[prop(optional, into)] busy: Signal<bool>,
    children: Children,
) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    let title_id = format!("{id}-title");
    let label_id = title_id.clone();
    Effect::new(move |_| {
        let requested = open.get();
        if let Some(element) = dialog.get() {
            if requested && !element.open() {
                if element.show_modal().is_err() {
                    open.set(false);
                }
            } else if !requested && element.open() {
                element.close();
            }
        }
    });

    view! {
        <dialog
            id=id
            node_ref=dialog
            aria-labelledby=label_id
            aria-modal="true"
            aria-busy=move || busy.get().to_string()
            class="fixed inset-0 m-auto max-h-[calc(100dvh-2rem)] w-[calc(100%-2rem)] max-w-md overflow-y-auto rounded-xl border border-crono-border bg-crono-surface p-5 text-crono-text shadow-xl backdrop:bg-zinc-950/40 sm:p-6"
            on:cancel=move |event: leptos::ev::Event| {
                event.prevent_default();
                if !busy.get_untracked() {
                    open.set(false);
                }
            }
            on:close=move |_| {
                if dialog.get_untracked().is_some_and(|element| !element.open()) {
                    open.set(false);
                }
            }
        >
            <h2 id=title_id class="break-words text-lg font-semibold">{move || title.get()}</h2>
            <div class="mt-3 space-y-5">{children()}</div>
        </dialog>
    }
}

#[cfg(test)]
mod browser_tests {
    use super::Modal;
    use leptos::prelude::*;
    use std::{cell::RefCell, rc::Rc};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::{Event, HtmlDialogElement, HtmlElement};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn modal_focuses_cancel_and_guards_dismissal_during_requests() {
        let document = web_sys::window().and_then(|window| window.document());
        assert!(document.is_some());
        let Some(document) = document else {
            return;
        };
        let host = document
            .create_element("div")
            .ok()
            .and_then(|element| element.dyn_into::<HtmlElement>().ok());
        assert!(host.is_some());
        let Some(host) = host else {
            return;
        };
        assert!(
            document
                .body()
                .is_some_and(|body| body.append_child(&host).is_ok())
        );
        let captured = Rc::new(RefCell::new(None));
        let capture_for_mount = Rc::clone(&captured);
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let open = RwSignal::new(false);
            let busy = RwSignal::new(false);
            *capture_for_mount.borrow_mut() = Some((open, busy));
            view! {
                <button id="modal-test-opener" on:click=move |_| open.set(true)>"Open"</button>
                <Modal id="modal-test" open busy=Signal::derive(move || busy.get()) title=Signal::derive(|| "Confirm action".to_string())>
                    <button id="modal-test-cancel" autofocus on:click=move |_| open.set(false)>"Cancel"</button>
                    <button>"Confirm"</button>
                </Modal>
            }
        });
        let signals = *captured.borrow();
        assert!(signals.is_some());
        let Some((open, busy)) = signals else {
            return;
        };
        let opener = host
            .query_selector("#modal-test-opener")
            .ok()
            .flatten()
            .and_then(|element| element.dyn_into::<HtmlElement>().ok());
        assert!(opener.is_some());
        let Some(opener) = opener else {
            return;
        };
        assert!(opener.focus().is_ok());
        opener.click();
        leptos::task::tick().await;
        let dialog = host
            .query_selector("dialog")
            .ok()
            .flatten()
            .and_then(|element| element.dyn_into::<HtmlDialogElement>().ok());
        assert!(dialog.is_some());
        let Some(dialog) = dialog else {
            return;
        };
        assert!(dialog.open());
        assert_eq!(
            document.active_element().map(|element| element.id()),
            Some("modal-test-cancel".to_string())
        );

        busy.set(true);
        dispatch_cancel(&dialog);
        leptos::task::tick().await;
        assert!(open.get_untracked());
        assert!(dialog.open());
        busy.set(false);
        dispatch_cancel(&dialog);
        leptos::task::tick().await;
        assert!(!open.get_untracked());
        assert!(!dialog.open());
        assert_eq!(
            document.active_element().map(|element| element.id()),
            Some("modal-test-opener".to_string())
        );
        drop(handle);
        host.remove();
    }

    /// Exercise the native cancel event without bypassing the modal's busy guard.
    fn dispatch_cancel(dialog: &HtmlDialogElement) {
        let cancel = Event::new("cancel");
        assert!(cancel.is_ok());
        if let Ok(cancel) = cancel {
            assert!(dialog.dispatch_event(&cancel).is_ok());
        }
    }
}
