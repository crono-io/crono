//! Namespace-filtered, cursor-paged Target browsing.
//!
//! The API lists one Namespace at a time in name order. The page keeps only
//! opaque cursors needed for Previous and Next navigation; changing Namespace
//! discards that history so no cursor crosses a resource scope.
//! Each row confirms deletion in a modal before submitting its UUID, then refreshes the
//! current page. The server protects the starter Target and rejects targets in use.

use super::super::resource_options;
use crate::{
    api,
    components::{
        DeleteControl, EmptyState, PageHeader, QUIET_ACTION_CLASS, ResourceSelect, focus_heading,
    },
    navigation::{AppRoute, MaterialSymbol, target_edit_path},
};
use crono_api::{Page, TargetResource};
use leptos::{prelude::*, task::spawn_local};
use leptos_router::components::A;
use uuid::Uuid;

const ACTION_CLASS: &str = "inline-flex items-center justify-center rounded-md bg-crono-primary px-4 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2";

/// Browse, edit, and confirm deletion of Targets within the selected Namespace.
#[component]
pub fn TargetsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let namespace_choices = resource_options::namespaces();
    let after = RwSignal::new(None::<String>);
    let page_stack = RwSignal::new(Vec::<Option<String>>::new());
    let previous_namespace = RwSignal::new(None::<Uuid>);
    Effect::new(move |_| {
        let selected = namespace_id.get();
        if previous_namespace.get_untracked() != selected {
            after.set(None);
            page_stack.set(Vec::new());
            previous_namespace.set(selected);
        }
    });
    let targets = LocalResource::new(move || load_targets(namespace_id.get(), after.get()));

    view! {
        <div class="space-y-8">
            <PageHeader title="Targets" description="Manage destination-specific arguments and inputs for Jobs.">
                <A href=AppRoute::TargetsNew.path() attr:class=ACTION_CLASS>"+ Create Target"</A>
            </PageHeader>
            <div class="max-w-xl">
                <ResourceSelect id="targets-namespace-filter" label="Namespace" placeholder="Select a Namespace to browse Targets…" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error optional=true select_single=true />
            </div>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                {move || {
                    if namespace_choices.loading.get() {
                        view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Namespaces…"</p> }.into_any()
                    } else if namespace_choices.load_error.get().is_some() {
                        view! { <p class="px-6 py-10 text-center text-sm text-crono-failed" role="alert">"Namespaces could not be loaded. Retry by refreshing the page."</p> }.into_any()
                    } else if namespace_choices.options.get().is_empty() {
                        view! {
                            <EmptyState icon=MaterialSymbol::Dns title="No Namespaces yet" description="Create a Namespace before adding Targets.">
                                <A href=AppRoute::Namespaces.path() attr:class=ACTION_CLASS>"Go to Namespaces"</A>
                            </EmptyState>
                        }.into_any()
                    } else if namespace_id.get().is_none() {
                        view! { <EmptyState icon=MaterialSymbol::Dns title="Select a Namespace" description="Choose a Namespace above to browse and manage its Targets." /> }.into_any()
                    } else {
                        view! { <TargetResults targets after page_stack /> }.into_any()
                    }
                }}
            </section>
        </div>
    }
}

/// Keep page controls beside the result they navigate, including error states.
#[component]
fn TargetResults(
    targets: LocalResource<api::ApiResult<Page<TargetResource>>>,
    after: RwSignal<Option<String>>,
    page_stack: RwSignal<Vec<Option<String>>>,
) -> impl IntoView {
    let feedback = RwSignal::new(None::<String>);
    let heading = NodeRef::<leptos::html::H2>::new();
    let on_deleted = Callback::new(move |name: String| {
        feedback.set(Some(format!("Deleted Target {name}.")));
        targets.refetch();
        focus_heading(heading);
    });
    view! {
        <div>
            <h2 node_ref=heading tabindex="-1" class="sr-only">"Targets in Namespace"</h2>
            <Show when=move || feedback.get().is_some()>
                <p class="px-5 py-3 text-sm text-crono-muted sm:px-6" role="status">{move || feedback.get().unwrap_or_default()}</p>
            </Show>
            {move || targets.map(|result| match result {
                Ok(page) if page.items.is_empty() && page_stack.get().is_empty() => view! {
                    <EmptyState icon=MaterialSymbol::Dns title="No Targets yet" description="Create the first Target in this Namespace to provide destination-specific inputs.">
                        <A href=AppRoute::TargetsNew.path() attr:class=ACTION_CLASS>"Create Target"</A>
                    </EmptyState>
                }.into_any(),
                Ok(page) if page.items.is_empty() => view! {
                    <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Targets on this page. Go back to the previous page."</p>
                }.into_any(),
                Ok(page) => view! { <TargetRows page=page.clone() on_deleted /> }.into_any(),
                Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
            }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Targets…"</p> }.into_any())}
            <div class="flex items-center justify-between border-t border-crono-border px-4 py-2 sm:px-5">
                <button type="button" aria-label="Previous page" class=QUIET_ACTION_CLASS disabled=move || page_stack.get().is_empty() on:click=move |_| {
                    let previous = page_stack.get_untracked().last().cloned().flatten();
                    page_stack.update(|stack| { stack.pop(); });
                    after.set(previous);
                }>"← Previous"</button>
                <span class="text-xs text-crono-muted">{move || format!("Page {}", page_stack.get().len() + 1)}</span>
                {move || targets.get().and_then(Result::ok).and_then(|page| page.next_cursor).map(|cursor| view! {
                    <button type="button" aria-label="Next page" class=QUIET_ACTION_CLASS on:click=move |_| {
                        page_stack.update(|stack| stack.push(after.get_untracked()));
                        after.set(Some(cursor.clone()));
                    }>"Next →"</button>
                })}
            </div>
        </div>
    }
}

/// Render only the current page so the list stays bounded as Targets grow.
#[component]
fn TargetRows(page: Page<TargetResource>, on_deleted: Callback<String>) -> impl IntoView {
    view! {
        <ul class="divide-y divide-crono-border">
            {page.items.into_iter().map(|target| view! {
                <TargetRow target on_deleted />
            }).collect_view()}
        </ul>
    }
}

/// Confirm deletion in a modal, keeping errors visible there and preventing repeat requests.
#[component]
fn TargetRow(target: TargetResource, on_deleted: Callback<String>) -> impl IntoView {
    let id = target.id;
    let protected = target.namespace == "default" && target.name == "default";
    let qualified_name = StoredValue::new(target.qualified_name.clone());
    let confirming = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let on_confirm = Callback::new(move |()| {
        if busy.get_untracked() || !confirming.get_untracked() {
            return;
        }
        error.set(None);
        busy.set(true);
        spawn_local(async move {
            let result = api::delete_target(id).await;
            busy.set(false);
            match result {
                Ok(()) => {
                    confirming.set(false);
                    on_deleted.run(qualified_name.get_value());
                }
                Err(api_error) => {
                    error.set(Some(if api_error.code == "resource_in_use" {
                        "This Target is still referenced by a Target Set, Schedule, Run request, or Run history and cannot be deleted.".to_string()
                    } else {
                        api_error.message
                    }));
                }
            }
        });
    });
    view! {
        <li class="px-5 py-4 sm:px-6">
            <div class="flex flex-wrap items-center justify-between gap-4">
                <div class="min-w-0 break-words">
                    <p class="font-medium text-crono-text">{target.name}</p>
                    <p class="text-sm text-crono-muted">{target.qualified_name}</p>
                    <p class="mt-1 text-xs text-crono-muted">{format!("{} additional argv items", target.arguments.len())}</p>
                </div>
                <div class="flex flex-wrap items-center gap-3">
                    <A href=target_edit_path(id) attr:class=QUIET_ACTION_CLASS>"Edit"</A>
                    <Show when=move || !protected fallback=|| view! {
                        <span class="text-xs text-crono-muted">"Protected default Target"</span>
                    }>
                        <DeleteControl id=format!("delete-target-{id}") resource="Target" name=qualified_name.get_value() description="This permanently removes the Target and cannot be undone. Targets referenced by a Target Set, Schedule, Run request, or Run history cannot be deleted." open=confirming busy error on_confirm />
                    </Show>
                </div>
            </div>
        </li>
    }
}

/// Keep the Namespace-scoped API call bounded to one 25-item page.
async fn load_targets(
    namespace: Option<Uuid>,
    after: Option<String>,
) -> api::ApiResult<Page<TargetResource>> {
    match namespace {
        Some(id) => api::list_targets(id, after.as_deref()).await,
        None => Ok(Page {
            items: Vec::new(),
            next_cursor: None,
        }),
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod browser_tests {
    use super::TargetResults;
    use crate::api;
    use crono_api::{Page, TargetResource};
    use leptos::prelude::*;
    use leptos_router::components::Router;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::HtmlElement;

    wasm_bindgen_test_configure!(run_in_browser);

    fn target(name: &str) -> TargetResource {
        TargetResource {
            id: uuid::Uuid::nil(),
            namespace_id: uuid::Uuid::nil(),
            namespace: "default".to_string(),
            name: name.to_string(),
            qualified_name: format!("default/{name}"),
            arguments: Vec::new(),
            inputs: serde_json::json!({}),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[wasm_bindgen_test]
    async fn next_and_previous_show_bounded_target_pages() {
        let document = web_sys::window().and_then(|window| window.document());
        assert!(document.is_some());
        let Some(document) = document else {
            return;
        };
        let host = document.create_element("div");
        assert!(host.is_ok());
        let Ok(host) = host else {
            return;
        };
        let host = host.dyn_into::<HtmlElement>();
        assert!(host.is_ok());
        let Ok(host) = host else {
            return;
        };
        assert!(
            document
                .body()
                .is_some_and(|body| body.append_child(&host).is_ok())
        );
        let handle = leptos::mount::mount_to(host.clone(), || {
            let after = RwSignal::new(None::<String>);
            let page_stack = RwSignal::new(Vec::<Option<String>>::new());
            let targets = LocalResource::new(move || {
                let cursor = after.get();
                async move {
                    Ok::<Page<TargetResource>, api::ApiError>(if cursor.is_some() {
                        Page {
                            items: vec![target("beta")],
                            next_cursor: None,
                        }
                    } else {
                        Page {
                            items: vec![target("alpha")],
                            next_cursor: Some("alpha".to_string()),
                        }
                    })
                }
            });
            view! { <Router><TargetResults targets after page_stack /></Router> }
        });
        leptos::task::tick().await;
        assert!(
            host.text_content()
                .is_some_and(|text| text.contains("alpha"))
        );
        let next = host
            .query_selector("button[aria-label='Next page']")
            .ok()
            .flatten();
        assert!(next.is_some());
        let next = next.and_then(|element| element.dyn_into::<HtmlElement>().ok());
        if let Some(next) = next {
            assert!(
                next.text_content()
                    .is_some_and(|text| text.contains("Next"))
            );
            next.click();
        }
        leptos::task::tick().await;
        leptos::task::tick().await;
        let page_text = host.text_content().unwrap_or_default();
        assert!(page_text.contains("beta"), "{page_text}");
        let previous = host
            .query_selector("button[aria-label='Previous page']")
            .ok()
            .flatten();
        let previous = previous.and_then(|element| element.dyn_into::<HtmlElement>().ok());
        assert!(previous.is_some());
        if let Some(previous) = previous {
            previous.click();
        }
        leptos::task::tick().await;
        leptos::task::tick().await;
        let page_text = host.text_content().unwrap_or_default();
        assert!(page_text.contains("alpha"), "{page_text}");
        drop(handle);
        host.remove();
    }
}
