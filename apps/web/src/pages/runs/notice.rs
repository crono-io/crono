//! Short-lived confirmation for a newly repeated Run.
//!
//! Both the history and details pages use the same notice. A timeout is tied
//! to the created Run ID, so an older timeout cannot hide a newer confirmation.
//! Disposed page signals are ignored when a timeout fires after navigation.

use crate::navigation::run_details_path;
use leptos::prelude::*;
use leptos_router::components::A;
use std::time::Duration;
use uuid::Uuid;

const NOTICE_DURATION: Duration = Duration::from_secs(8);

/// Show the new Run link briefly without letting an older timer clear it.
pub(super) fn show_created_run(notice: RwSignal<Option<Uuid>>, id: Uuid) {
    notice.set(Some(id));
    set_timeout(move || expire_notice(notice, id), NOTICE_DURATION);
}

/// Clear only the notice belonging to this timeout, if its page still exists.
fn expire_notice(notice: RwSignal<Option<Uuid>>, id: Uuid) {
    if notice.try_get_untracked() == Some(Some(id)) {
        notice.set(None);
    }
}

/// Keep the success link visible for a short time and allow immediate dismissal.
#[component]
pub(super) fn CreatedRunNotice(notice: RwSignal<Option<Uuid>>) -> impl IntoView {
    move || {
        notice.get().map(|id| view! {
            <div class="flex flex-wrap items-center justify-between gap-3 rounded-md border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-800" role="status">
                <p>"New Run created. "<A href=run_details_path(id) attr:class="font-semibold underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary">"View new Run →"</A></p>
                <button type="button" class="rounded-md px-2 py-1 font-medium hover:bg-emerald-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary" on:click=move |_| notice.set(None)>"Dismiss"</button>
            </div>
        })
    }
}

#[cfg(test)]
mod browser_tests {
    use super::{CreatedRunNotice, expire_notice, show_created_run};
    use leptos::prelude::*;
    use leptos_router::components::Router;
    use std::{cell::RefCell, rc::Rc};
    use uuid::Uuid;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::{Event, HtmlElement};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    async fn repeated_run_notice_replaces_old_link_and_can_be_dismissed() {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let host = document
            .create_element("div")
            .ok()
            .and_then(|element| element.dyn_into::<HtmlElement>().ok());
        assert!(host.is_some(), "browser test requires a host element");
        let Some(host) = host else {
            return;
        };
        let Some(body) = document.body() else {
            return;
        };
        assert!(body.append_child(&host).is_ok());

        let captured = Rc::new(RefCell::new(None));
        let capture_for_mount = Rc::clone(&captured);
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let notice = RwSignal::new(None);
            *capture_for_mount.borrow_mut() = Some(notice);
            view! { <Router><CreatedRunNotice notice /></Router> }
        });
        let notice = *captured.borrow();
        assert!(notice.is_some(), "notice signal must be mounted");
        let Some(notice) = notice else {
            return;
        };
        let first = Uuid::from_u128(1);
        let second = Uuid::from_u128(2);
        show_created_run(notice, first);
        leptos::task::tick().await;
        assert!(host.inner_text().contains("New Run created"));
        show_created_run(notice, second);
        expire_notice(notice, first);
        leptos::task::tick().await;
        assert!(
            host.query_selector(&format!("a[href='/runs/{second}']"))
                .ok()
                .flatten()
                .is_some()
        );
        expire_notice(notice, second);
        leptos::task::tick().await;
        assert!(
            host.query_selector("[role='status']")
                .ok()
                .flatten()
                .is_none()
        );

        show_created_run(notice, second);
        leptos::task::tick().await;
        let dismiss = host.query_selector("[role='status'] button").ok().flatten();
        assert!(dismiss.is_some(), "notice must be dismissible");
        let click = Event::new("click");
        assert!(click.is_ok());
        if let (Some(dismiss), Ok(click)) = (dismiss, click) {
            assert!(dismiss.dispatch_event(&click).is_ok());
            leptos::task::tick().await;
            assert!(
                host.query_selector("[role='status']")
                    .ok()
                    .flatten()
                    .is_none()
            );
        }
        drop(handle);
        host.remove();
    }
}
