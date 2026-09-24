//! Accessible form controls shared by Crono resource workflows.
//!
//! The controls keep canonical-name feedback, required-field presentation, and
//! searchable UUID selection consistent while leaving API ownership in pages.
//! Search is entirely client-side over one bounded collection load.

use super::Icon;
use crate::navigation::MaterialSymbol;
use leptos::prelude::*;
use uuid::Uuid;

const INPUT_CLASS: &str = "w-full rounded-md border border-crono-border bg-white px-3 py-2.5 text-sm text-crono-text shadow-sm outline-none transition placeholder:text-zinc-400 focus:border-crono-primary focus:ring-2 focus:ring-crono-primary-soft disabled:cursor-not-allowed disabled:bg-zinc-50 disabled:text-zinc-500";

/// UUID-backed option displayed by its canonical human-readable name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceOption {
    pub id: Uuid,
    pub label: String,
}

/// Return concise DNS-1123 feedback without changing the supplied value.
#[must_use]
pub fn name_validation_message(value: &str, required: bool) -> Option<String> {
    if value.is_empty() && !required {
        return None;
    }
    crono_api::validate_resource_name(value)
        .err()
        .map(|error| error.to_string())
}

/// Show invalid non-empty names immediately, but defer the required message
/// until the user attempts to submit the form.
#[must_use]
pub fn visible_name_validation(value: &str, attempted: bool) -> Option<String> {
    if value.is_empty() && !attempted {
        return None;
    }
    name_validation_message(value, true)
}

/// Parse and validate the JSON object entered in an input editor.
///
/// # Errors
///
/// Returns a concise syntax, shape, key, or size error suitable for inline UI.
pub fn parse_input_object(value: &str) -> Result<serde_json::Value, String> {
    let parsed = serde_json::from_str(value).map_err(|error| format!("Invalid JSON: {error}"))?;
    crono_execution::validate_inputs(&parsed).map_err(|error| error.to_string())?;
    Ok(parsed)
}

/// Multi-line JSON object editor shared by Job, Target, Target Set, Schedule,
/// and manual Run input layers.
#[component]
pub fn JsonObjectInput(
    id: &'static str,
    label: &'static str,
    value: RwSignal<String>,
    error: Signal<Option<String>>,
) -> impl IntoView {
    let error_id = format!("{id}-error");
    view! {
        <div>
            <label for=id class="block text-sm font-medium text-crono-text">{label}</label>
            <textarea
                id=id
                class=format!("mt-1.5 min-h-32 font-mono {INPUT_CLASS}")
                spellcheck="false"
                aria-describedby=error_id.clone()
                aria-invalid=move || error.get().is_some().then_some("true")
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
            />
            <p class="mt-1.5 text-xs text-crono-muted">
                "JSON object. Values are not treated as secrets."
            </p>
            <p id=error_id class="mt-1 text-sm text-crono-failed" role="alert">
                {move || error.get().unwrap_or_default()}
            </p>
        </div>
    }
}

/// Ordered argv editor; each row remains exactly one process argument.
#[component]
pub fn ArgumentListInput(
    id: &'static str,
    label: &'static str,
    values: RwSignal<Vec<String>>,
    #[prop(optional, into)] error: Signal<Option<String>>,
) -> impl IntoView {
    view! {
        <fieldset>
            <legend class="text-sm font-medium text-crono-text">{label}</legend>
            <div class="mt-1.5 space-y-2">
                {move || values.get().into_iter().enumerate().map(|(index, value)| {
                    view! {
                        <div class="flex gap-2">
                            <input
                                id=format!("{id}-{index}")
                                class=format!("font-mono {INPUT_CLASS}")
                                type="text"
                                autocomplete="off"
                                prop:value=value
                                on:input=move |event| values.update(|items| {
                                    if let Some(item) = items.get_mut(index) {
                                        *item = event_target_value(&event);
                                    }
                                })
                            />
                            <button
                                type="button"
                                class="rounded-md border border-crono-border px-3 text-sm text-crono-muted hover:bg-zinc-50 hover:text-crono-failed"
                                aria-label=format!("Remove argument {}", index + 1)
                                on:click=move |_| values.update(|items| {
                                    if index < items.len() {
                                        items.remove(index);
                                    }
                                })
                            >
                                "Remove"
                            </button>
                        </div>
                    }
                }).collect_view()}
            </div>
            <button
                type="button"
                class="mt-2 text-sm font-medium text-crono-primary hover:text-crono-primary-hover"
                on:click=move |_| values.update(|items| items.push(String::new()))
            >
                "+ Add argument"
            </button>
            <p class="mt-1.5 text-xs text-crono-muted">
                "Use {{ path.to.value }} for scalar inputs. Each row is one argv item; no shell is used."
            </p>
            <p class="mt-1 text-sm text-crono-failed" role="alert">
                {move || error.get().unwrap_or_default()}
            </p>
        </fieldset>
    }
}

/// Canonical resource-name field with stable guidance and inline errors.
#[component]
pub fn ResourceNameInput(
    id: &'static str,
    label: &'static str,
    value: RwSignal<String>,
    error: Signal<Option<String>>,
) -> impl IntoView {
    let help_id = format!("{id}-help");
    let error_id = format!("{id}-error");
    let described_by = format!("{help_id} {error_id}");
    view! {
        <div>
            <label for=id class="block text-sm font-medium text-crono-text">
                {label}<span class="ml-1 text-crono-failed" aria-hidden="true">"*"</span>
            </label>
            <input
                id=id
                class=INPUT_CLASS
                type="text"
                autocomplete="off"
                required
                aria-describedby=described_by
                aria-invalid=move || error.get().is_some().then_some("true")
                prop:value=move || value.get()
                on:input=move |event| value.set(event_target_value(&event))
            />
            <p id=help_id class="mt-1.5 text-xs text-crono-muted">
                "Lowercase letters, numbers and hyphens. Maximum 63 characters."
            </p>
            <p id=error_id class="mt-1 text-sm text-crono-failed" role="alert">
                {move || error.get().unwrap_or_default()}
            </p>
        </div>
    }
}

/// Searchable single-resource combobox that stores only the selected UUID.
#[component]
pub fn ResourceSelect(
    id: &'static str,
    label: &'static str,
    placeholder: &'static str,
    options: Signal<Vec<ResourceOption>>,
    selected: RwSignal<Option<Uuid>>,
    loading: Signal<bool>,
    load_error: Signal<Option<String>>,
    #[prop(optional, into)] field_error: Signal<Option<String>>,
    #[prop(optional)] optional: bool,
) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let open = RwSignal::new(false);
    let active = RwSignal::new(0_usize);
    let list_id = format!("{id}-listbox");
    let error_id = format!("{id}-error");
    let filtered = Memo::new(move |_| {
        let needle = query.get().to_ascii_lowercase();
        options
            .get()
            .into_iter()
            .filter(|option| option.label.contains(&needle))
            .collect::<Vec<_>>()
    });
    clear_stale_selection(selected, options, loading, load_error);
    let display_value = move || {
        if open.get() {
            return query.get();
        }
        selected
            .get()
            .and_then(|id| {
                options
                    .get()
                    .into_iter()
                    .find(|option| option.id == id)
                    .map(|option| option.label)
            })
            .unwrap_or_default()
    };
    let disabled = move || loading.get() || load_error.get().is_some() || options.get().is_empty();
    let visible_error = move || load_error.get().or_else(|| field_error.get());
    let choose = Callback::new(move |option: ResourceOption| {
        selected.set(Some(option.id));
        query.set(String::new());
        open.set(false);
    });
    let keydown = move |event| select_keydown(&event, filtered, active, open, choose);

    view! {
        <div>
            <label for=id class="block text-sm font-medium text-crono-text">
                {label}
                <Show when=move || !optional>
                    <span class="ml-1 text-crono-failed" aria-hidden="true">"*"</span>
                </Show>
            </label>
            <div class="relative mt-1.5">
                <input
                    id=id
                    class=INPUT_CLASS
                    type="text"
                    role="combobox"
                    autocomplete="off"
                    required=!optional
                    placeholder=move || if loading.get() { "Loading…" } else { placeholder }
                    disabled=disabled
                    aria-expanded=move || open.get().to_string()
                    aria-controls=list_id.clone()
                    aria-autocomplete="list"
                    aria-invalid=move || field_error.get().is_some().then_some("true")
                    aria-describedby=error_id.clone()
                    prop:value=display_value
                    on:focus=move |_| {
                        query.set(String::new());
                        active.set(0);
                        open.set(true);
                    }
                    on:input=move |event| {
                        query.set(event_target_value(&event));
                        selected.set(None);
                        active.set(0);
                        open.set(true);
                    }
                    on:keydown=keydown
                    on:blur=move |_| open.set(false)
                />
                <Icon symbol=MaterialSymbol::ExpandMore class="pointer-events-none absolute right-3 top-2.5 text-lg text-crono-muted" />
                <Show when=move || open.get() && !disabled()>
                    <ResourceSelectOptions
                        list_id=list_id.clone()
                        filtered=filtered
                        selected=selected
                        active=active
                        choose=choose
                    />
                </Show>
            </div>
            <p id=error_id class="mt-1 text-sm text-crono-failed" role="alert">
                {move || visible_error().unwrap_or_default()}
            </p>
        </div>
    }
}

fn clear_stale_selection(
    selected: RwSignal<Option<Uuid>>,
    options: Signal<Vec<ResourceOption>>,
    loading: Signal<bool>,
    load_error: Signal<Option<String>>,
) {
    Effect::new(move |_| {
        let current = selected.get();
        let available = options.get();
        if !loading.get()
            && load_error.get().is_none()
            && current.is_some_and(|id| !available.iter().any(|option| option.id == id))
        {
            selected.set(None);
        }
    });
}

fn select_keydown(
    event: &leptos::ev::KeyboardEvent,
    filtered: Memo<Vec<ResourceOption>>,
    active: RwSignal<usize>,
    open: RwSignal<bool>,
    choose: Callback<ResourceOption>,
) {
    match event.key().as_str() {
        "ArrowDown" => {
            event.prevent_default();
            open.set(true);
            let last = filtered.get_untracked().len().saturating_sub(1);
            active.update(|index| *index = index.saturating_add(1).min(last));
        }
        "ArrowUp" => {
            event.prevent_default();
            open.set(true);
            active.update(|index| *index = index.saturating_sub(1));
        }
        "Home" => active.set(0),
        "End" => active.set(filtered.get_untracked().len().saturating_sub(1)),
        "Enter" if open.get_untracked() => {
            event.prevent_default();
            if let Some(option) = filtered
                .get_untracked()
                .get(active.get_untracked())
                .cloned()
            {
                choose.run(option);
            }
        }
        "Escape" => open.set(false),
        _ => {}
    }
}

#[component]
fn ResourceSelectOptions(
    list_id: String,
    filtered: Memo<Vec<ResourceOption>>,
    selected: RwSignal<Option<Uuid>>,
    active: RwSignal<usize>,
    choose: Callback<ResourceOption>,
) -> impl IntoView {
    view! {
        <ul id=list_id role="listbox" class="absolute z-20 mt-1 max-h-60 w-full overflow-auto rounded-md border border-crono-border bg-white py-1 shadow-lg">
            {move || {
                let visible = filtered.get();
                if visible.is_empty() {
                    return view! { <li class="px-3 py-3 text-sm text-crono-muted">"No matches."</li> }.into_any();
                }
                visible.into_iter().enumerate().map(|(index, option)| {
                    let selected_option = selected.get_untracked() == Some(option.id);
                    let option_for_click = option.clone();
                    view! {
                        <li role="option" aria-selected=selected_option.to_string()>
                            <button
                                type="button"
                                class=move || format!("flex w-full items-center justify-between px-3 py-2 text-left text-sm hover:bg-crono-primary-soft {}", if active.get() == index { "bg-crono-primary-soft" } else { "" })
                                on:mousedown=move |event| {
                                    event.prevent_default();
                                    choose.run(option_for_click.clone());
                                }
                            >
                                <span>{option.label}</span>
                                <Show when=move || selected.get() == Some(option.id)>
                                    <Icon symbol=MaterialSymbol::Check class="text-base text-crono-primary" />
                                </Show>
                            </button>
                        </li>
                    }
                }).collect_view().into_any()
            }}
        </ul>
    }
}

/// Searchable multi-select used for explicit Target Set membership.
#[component]
pub fn ResourceMultiSelect(
    id: &'static str,
    label: &'static str,
    options: Signal<Vec<ResourceOption>>,
    selected: RwSignal<Vec<Uuid>>,
    loading: Signal<bool>,
    load_error: Signal<Option<String>>,
    #[prop(optional, into)] field_error: Signal<Option<String>>,
) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let filtered = Memo::new(move |_| {
        let needle = query.get().to_ascii_lowercase();
        options
            .get()
            .into_iter()
            .filter(|option| option.label.contains(&needle))
            .collect::<Vec<_>>()
    });
    Effect::new(move |_| {
        let available = options.get();
        if !loading.get() && load_error.get().is_none() {
            selected.update(|ids| {
                ids.retain(|id| available.iter().any(|option| option.id == *id));
            });
        }
    });
    view! {
        <fieldset>
            <legend class="text-sm font-medium text-crono-text">
                {label}<span class="ml-1 text-crono-failed" aria-hidden="true">"*"</span>
            </legend>
            <input
                id=id
                class=format!("mt-1.5 {INPUT_CLASS}")
                type="search"
                autocomplete="off"
                placeholder=move || if loading.get() { "Loading targets…" } else { "Search targets…" }
                disabled=move || loading.get() || load_error.get().is_some() || options.get().is_empty()
                prop:value=move || query.get()
                on:input=move |event| query.set(event_target_value(&event))
            />
            <div role="listbox" aria-multiselectable="true" class="mt-2 max-h-56 overflow-auto rounded-md border border-crono-border bg-white">
                {move || {
                    let visible = filtered.get();
                    if visible.is_empty() {
                        return view! { <p class="px-3 py-4 text-sm text-crono-muted">"No targets match."</p> }.into_any();
                    }
                    visible.into_iter().map(|option| {
                        let id = option.id;
                        view! {
                            <label class="flex cursor-pointer items-center gap-3 border-b border-crono-border px-3 py-2.5 text-sm last:border-b-0 hover:bg-crono-primary-soft">
                                <input
                                    type="checkbox"
                                    class="size-4 rounded border-crono-border text-crono-primary focus:ring-crono-primary"
                                    prop:checked=move || selected.get().contains(&id)
                                    on:change=move |event| {
                                        let checked = event_target_checked(&event);
                                        selected.update(|ids| {
                                            if checked && !ids.contains(&id) {
                                                ids.push(id);
                                            } else if !checked {
                                                ids.retain(|candidate| *candidate != id);
                                            }
                                        });
                                    }
                                />
                                <span>{option.label}</span>
                            </label>
                        }
                    }).collect_view().into_any()
                }}
            </div>
            <p class="mt-1.5 text-xs text-crono-muted">{move || format!("{} selected", selected.get().len())}</p>
            <p class="mt-1 text-sm text-crono-failed" role="alert">
                {move || load_error.get().or_else(|| field_error.get()).unwrap_or_default()}
            </p>
        </fieldset>
    }
}

/// Consistent reset and primary-submit actions for resource forms.
#[component]
pub fn FormActions(
    submit_label: &'static str,
    disabled: Signal<bool>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="flex items-center justify-end gap-3 pt-2">
            <button type="button" class="rounded-md px-4 py-2 text-sm font-medium text-crono-muted hover:bg-zinc-100 hover:text-crono-text" on:click=move |_| on_cancel.run(())>
                "Cancel"
            </button>
            <button type="submit" disabled=move || disabled.get() class="rounded-md bg-crono-primary px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50">
                {submit_label}
            </button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::{name_validation_message, visible_name_validation};

    #[test]
    fn name_feedback_uses_the_shared_dns_rule() {
        assert!(name_validation_message("postgres-backup", true).is_none());
        assert!(name_validation_message("Postgres Backup", true).is_some());
        assert!(name_validation_message(&"a".repeat(64), true).is_some());
    }

    #[test]
    fn visible_feedback_defers_only_the_untouched_required_error() {
        assert!(visible_name_validation("", false).is_none());
        assert!(visible_name_validation("", true).is_some());
        assert!(visible_name_validation("Production", false).is_some());
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod browser_tests {
    use super::{
        FormActions, ResourceNameInput, ResourceOption, ResourceSelect, visible_name_validation,
    };
    use leptos::prelude::*;
    use uuid::Uuid;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::{Event, HtmlElement, HtmlInputElement, MouseEvent};

    wasm_bindgen_test_configure!(run_in_browser);

    fn test_host() -> Option<HtmlElement> {
        let document = web_sys::window()?.document()?;
        let host: HtmlElement = document.create_element("div").ok()?.dyn_into().ok()?;
        document.body()?.append_child(&host).ok()?;
        Some(host)
    }

    #[wasm_bindgen_test]
    async fn resource_select_filters_and_stores_the_uuid() {
        let host = test_host();
        assert!(host.is_some(), "browser test requires a document body");
        let Some(host) = host else {
            return;
        };
        let production_id = Uuid::from_u128(1);
        let staging_id = Uuid::from_u128(2);
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let selected = RwSignal::new(None);
            let options = Signal::derive(move || {
                vec![
                    ResourceOption {
                        id: production_id,
                        label: "production".to_string(),
                    },
                    ResourceOption {
                        id: staging_id,
                        label: "staging".to_string(),
                    },
                ]
            });
            view! {
                <ResourceSelect
                    id="namespace"
                    label="Namespace"
                    placeholder="Search/select namespace…"
                    options=options
                    selected=selected
                    loading=Signal::derive(|| false)
                    load_error=Signal::derive(|| None)
                />
                <output id="selected-id">{move || selected.get().map(|id| id.to_string()).unwrap_or_default()}</output>
            }
        });

        let input = host.query_selector("#namespace").ok().flatten();
        assert!(input.is_some(), "selector input must render");
        let Some(input) = input else {
            return;
        };
        let input = input.dyn_into::<HtmlInputElement>();
        assert!(input.is_ok(), "selector must render an input");
        let Ok(input) = input else {
            return;
        };
        input.set_value("prod");
        let input_event = Event::new("input");
        assert!(input_event.is_ok(), "input event must be constructible");
        let Ok(input_event) = input_event else {
            return;
        };
        assert!(input.dispatch_event(&input_event).is_ok());
        leptos::task::tick().await;
        assert!(host.inner_text().contains("production"));
        assert!(!host.inner_text().contains("staging"));

        let option = host
            .query_selector("[role=\"option\"] button")
            .ok()
            .flatten();
        assert!(option.is_some(), "filtered option must render");
        let Some(option) = option else {
            return;
        };
        let mouse_event = MouseEvent::new("mousedown");
        assert!(mouse_event.is_ok(), "mouse event must be constructible");
        let Ok(mouse_event) = mouse_event else {
            return;
        };
        assert!(option.dispatch_event(&mouse_event).is_ok());
        leptos::task::tick().await;
        assert!(host.inner_text().contains(&production_id.to_string()));
        drop(handle);
        host.remove();
    }

    #[wasm_bindgen_test]
    async fn resource_select_exposes_loading_empty_and_api_errors() {
        let host = test_host();
        assert!(host.is_some(), "browser test requires a document body");
        let Some(host) = host else {
            return;
        };
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let selected = RwSignal::new(None);
            let loading = RwSignal::new(true);
            let load_error = RwSignal::new(None::<String>);
            view! {
                <ResourceSelect
                    id="stateful-namespace"
                    label="Namespace"
                    placeholder="Search/select namespace…"
                    options=Signal::derive(Vec::new)
                    selected=selected
                    loading=loading.into()
                    load_error=load_error.into()
                />
                <button id="empty" on:click=move |_| loading.set(false)>"empty"</button>
                <button id="error" on:click=move |_| load_error.set(Some("Namespace API failed.".to_string()))>"error"</button>
            }
        });
        let input = host.query_selector("#stateful-namespace").ok().flatten();
        assert!(input.is_some(), "selector input must render");
        let Some(input) = input else {
            return;
        };
        assert_eq!(
            input.get_attribute("placeholder").as_deref(),
            Some("Loading…")
        );

        let click = MouseEvent::new("click");
        assert!(click.is_ok(), "mouse event must be constructible");
        let Ok(click) = click else {
            return;
        };
        let empty = host.query_selector("#empty").ok().flatten();
        assert!(empty.is_some(), "empty-state trigger must render");
        let Some(empty) = empty else {
            return;
        };
        assert!(empty.dispatch_event(&click).is_ok());
        leptos::task::tick().await;
        assert!(input.has_attribute("disabled"));

        let error = host.query_selector("#error").ok().flatten();
        assert!(error.is_some(), "error-state trigger must render");
        let Some(error) = error else {
            return;
        };
        assert!(error.dispatch_event(&click).is_ok());
        leptos::task::tick().await;
        assert!(host.inner_text().contains("Namespace API failed."));
        drop(handle);
        host.remove();
    }

    #[wasm_bindgen_test]
    fn resource_name_input_blocks_invalid_names_and_surfaces_server_errors() {
        let host = test_host();
        assert!(host.is_some(), "browser test requires a document body");
        let Some(host) = host else {
            return;
        };
        let handle = leptos::mount::mount_to(host.clone(), move || {
            let invalid_name = RwSignal::new("Production".to_string());
            let valid_name = RwSignal::new("production".to_string());
            view! {
                <ResourceNameInput
                    id="invalid-name"
                    label="Name"
                    value=invalid_name
                    error=Signal::derive(move || visible_name_validation(&invalid_name.get(), false))
                />
                <FormActions
                    submit_label="Create"
                    disabled=Signal::derive(move || visible_name_validation(&invalid_name.get(), false).is_some())
                    on_cancel=Callback::new(|()| {})
                />
                <ResourceNameInput
                    id="server-name"
                    label="Name"
                    value=valid_name
                    error=Signal::derive(|| Some("Name already exists.".to_string()))
                />
            }
        });
        assert!(
            host.inner_text()
                .contains("start and end with a lowercase letter or number")
        );
        let submit = host
            .query_selector("button[type=\"submit\"]")
            .ok()
            .flatten();
        assert!(submit.is_some(), "submit button must render");
        let Some(submit) = submit else {
            return;
        };
        assert!(submit.has_attribute("disabled"));
        assert!(host.inner_text().contains("Name already exists."));
        let server_input = host.query_selector("#server-name").ok().flatten();
        assert!(server_input.is_some(), "server-validated field must render");
        let Some(server_input) = server_input else {
            return;
        };
        assert_eq!(
            server_input.get_attribute("aria-invalid").as_deref(),
            Some("true")
        );
        drop(handle);
        host.remove();
    }
}
