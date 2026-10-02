//! Searchable IANA timezone selection for recurring Schedules.
//!
//! The browser uses the same `chrono-tz` catalog as the server, so every
//! submitted value is a timezone identifier the scheduler understands. Offset
//! labels describe the current instant only; the server applies each region's
//! rules when it calculates future occurrences.

use super::{Icon, forms::INPUT_CLASS};
use crate::navigation::MaterialSymbol;
use chrono::{DateTime, Offset, Utc};
use chrono_tz::{TZ_VARIANTS, Tz};
use leptos::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TimezoneOption {
    id: String,
    label: String,
    offset_minutes: i32,
}

/// Select a supported regional timezone while storing its IANA identifier.
/// Search text never replaces the previously selected value until chosen.
#[component]
pub fn TimezoneSelect(selected: RwSignal<String>) -> impl IntoView {
    let options = RwSignal::new(timezone_options(browser_now()));
    let query = RwSignal::new(String::new());
    let open = RwSignal::new(false);
    let active = RwSignal::new(0_usize);
    let filtered = Memo::new(move |_| {
        let needle = query.get().trim().to_ascii_lowercase();
        options
            .get()
            .into_iter()
            .filter(|option| timezone_matches(option, &needle))
            .collect::<Vec<_>>()
    });
    let display = move || {
        if open.get() {
            return query.get();
        }
        let current = selected.get();
        options
            .get()
            .into_iter()
            .find(|option| option.id == current)
            .map_or(current, |option| option.label)
    };
    let choose = Callback::new(move |option: TimezoneOption| {
        selected.set(option.id);
        query.set(String::new());
        open.set(false);
    });

    view! {
        <div class="min-w-0">
            <label for="schedule-timezone" class="block text-sm font-medium text-crono-text">"Timezone"</label>
            <div class="relative mt-1.5">
                <input
                    id="schedule-timezone"
                    class=format!("pr-10 {INPUT_CLASS}")
                    type="text"
                    role="combobox"
                    autocomplete="off"
                    aria-expanded=move || open.get().to_string()
                    aria-controls="schedule-timezone-listbox"
                    aria-autocomplete="list"
                    aria-describedby="schedule-timezone-help"
                    aria-activedescendant=move || {
                        (open.get() && !filtered.get().is_empty())
                            .then(|| format!("schedule-timezone-option-{}", active.get()))
                    }
                    prop:value=display
                    on:focus=move |_| {
                        query.set(String::new());
                        active.set(0);
                        open.set(true);
                    }
                    on:input=move |event| {
                        query.set(event_target_value(&event));
                        active.set(0);
                        open.set(true);
                    }
                    on:keydown=move |event| timezone_keydown(&event, filtered, active, open, choose)
                    on:blur=move |_| open.set(false)
                />
                <Icon symbol=MaterialSymbol::ExpandMore class="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-lg text-crono-muted" />
                <Show when=move || open.get()>
                    <TimezoneOptions filtered active selected choose />
                </Show>
            </div>
            <p id="schedule-timezone-help" class="mt-1.5 text-xs text-crono-muted">
                "UTC offsets are current; regional daylight saving rules may change them."
            </p>
        </div>
    }
}

/// Read the current instant through the browser because `Utc::now` is unavailable on WASM.
fn browser_now() -> DateTime<Utc> {
    let iso: String = js_sys::Date::new_0().to_iso_string().into();
    DateTime::parse_from_rfc3339(&iso).map_or(DateTime::<Utc>::UNIX_EPOCH, |value| {
        value.with_timezone(&Utc)
    })
}

/// Build offset-labelled choices from the scheduler's supported IANA catalog.
fn timezone_options(now: DateTime<Utc>) -> Vec<TimezoneOption> {
    let mut options = TZ_VARIANTS
        .iter()
        .copied()
        .filter(|zone| *zone != Tz::UTC)
        .map(|zone| {
            let offset_minutes = now.with_timezone(&zone).offset().fix().local_minus_utc() / 60;
            TimezoneOption {
                id: zone.name().to_string(),
                label: format!("({} now) {}", format_offset(offset_minutes), zone.name()),
                offset_minutes,
            }
        })
        .collect::<Vec<_>>();
    options.sort_by(|left, right| {
        left.offset_minutes
            .cmp(&right.offset_minutes)
            .then_with(|| left.id.cmp(&right.id))
    });
    options.insert(
        0,
        TimezoneOption {
            id: "UTC".to_string(),
            label: "UTC (UTC+00:00)".to_string(),
            offset_minutes: 0,
        },
    );
    options
}

fn format_offset(minutes: i32) -> String {
    let sign = if minutes < 0 { '-' } else { '+' };
    let magnitude = minutes.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", magnitude / 60, magnitude % 60)
}

/// Match region names directly, and signed offsets only when explicitly typed.
fn timezone_matches(option: &TimezoneOption, needle: &str) -> bool {
    option.id.to_ascii_lowercase().contains(needle)
        || ((needle.starts_with("utc+")
            || needle.starts_with("utc-")
            || needle.starts_with('+')
            || needle.starts_with('-'))
            && format_offset(option.offset_minutes)
                .to_ascii_lowercase()
                .contains(needle))
}

fn timezone_keydown(
    event: &leptos::ev::KeyboardEvent,
    filtered: Memo<Vec<TimezoneOption>>,
    active: RwSignal<usize>,
    open: RwSignal<bool>,
    choose: Callback<TimezoneOption>,
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
        "Home" if open.get_untracked() => active.set(0),
        "End" if open.get_untracked() => {
            active.set(filtered.get_untracked().len().saturating_sub(1));
        }
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
fn TimezoneOptions(
    filtered: Memo<Vec<TimezoneOption>>,
    active: RwSignal<usize>,
    selected: RwSignal<String>,
    choose: Callback<TimezoneOption>,
) -> impl IntoView {
    view! {
        <ul id="schedule-timezone-listbox" role="listbox" class="absolute z-20 mt-1 max-h-60 w-full overflow-auto rounded-md border border-crono-border bg-white py-1 shadow-lg">
            {move || {
                let visible = filtered.get();
                if visible.is_empty() {
                    return view! { <li class="px-3 py-3 text-sm text-crono-muted">"No matching timezones."</li> }.into_any();
                }
                visible.into_iter().enumerate().map(|(index, option)| {
                    let for_pointer = option.clone();
                    let for_click = option.clone();
                    view! {
                        <li
                            id=format!("schedule-timezone-option-{index}")
                            role="option"
                            aria-selected=move || (selected.get() == option.id).to_string()
                            class=move || format!("cursor-pointer px-3 py-2 text-sm hover:bg-crono-primary-soft {}", if active.get() == index { "bg-crono-primary-soft" } else { "" })
                            on:pointerdown=move |event| {
                                event.prevent_default();
                                choose.run(for_pointer.clone());
                            }
                            on:click=move |_| choose.run(for_click.clone())
                        >{option.label}</li>
                    }
                }).collect_view().into_any()
            }}
        </ul>
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod browser_tests {
    use super::{TimezoneSelect, format_offset, timezone_matches, timezone_options};
    use chrono::{TimeZone, Utc};
    use leptos::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    use web_sys::{Event, HtmlElement, HtmlInputElement, PointerEvent};

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn offsets_are_current_and_utc_stays_first() {
        let winter = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single();
        let summer = Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).single();
        assert!(winter.is_some() && summer.is_some());
        if let (Some(winter), Some(summer)) = (winter, summer) {
            let winter_options = timezone_options(winter);
            let summer_options = timezone_options(summer);
            assert_eq!(winter_options.len(), chrono_tz::TZ_VARIANTS.len());
            assert_eq!(
                winter_options.first().map(|option| option.id.as_str()),
                Some("UTC")
            );
            assert_eq!(format_offset(345), "UTC+05:45");
            assert_eq!(format_offset(-300), "UTC-05:00");
            let winter_new_york = winter_options
                .iter()
                .find(|option| option.id == "America/New_York");
            let summer_new_york = summer_options
                .iter()
                .find(|option| option.id == "America/New_York");
            assert_eq!(
                winter_new_york.map(|option| option.offset_minutes),
                Some(-300)
            );
            assert_eq!(
                summer_new_york.map(|option| option.offset_minutes),
                Some(-240)
            );
            let kathmandu = winter_options
                .iter()
                .find(|option| option.id == "Asia/Kathmandu");
            assert_eq!(kathmandu.map(|option| option.offset_minutes), Some(345));
            assert!(kathmandu.is_some_and(|option| timezone_matches(option, "utc+05:45")));
            assert!(kathmandu.is_some_and(|option| timezone_matches(option, "+05:45")));
            assert!(!kathmandu.is_some_and(|option| timezone_matches(option, "utc")));
            assert!(
                winter_options
                    .iter()
                    .skip(1)
                    .map(|option| option.offset_minutes)
                    .is_sorted()
            );
        }
    }

    #[wasm_bindgen_test]
    async fn searching_keeps_utc_until_a_region_is_selected() {
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
            let selected = RwSignal::new("UTC".to_string());
            view! {
                <TimezoneSelect selected />
                <output id="selected-zone">{move || selected.get()}</output>
            }
        });
        leptos::task::tick().await;
        let input = host.query_selector("#schedule-timezone").ok().flatten();
        assert!(input.is_some());
        let Some(input) = input else {
            return;
        };
        let input = input.dyn_into::<HtmlInputElement>();
        assert!(input.is_ok());
        let Ok(input) = input else {
            return;
        };
        assert!(input.value().contains("UTC"));
        assert!(input.focus().is_ok());
        input.set_value("Kathmandu");
        let event = Event::new("input");
        assert!(event.is_ok());
        let Ok(event) = event else {
            return;
        };
        assert!(input.dispatch_event(&event).is_ok());
        leptos::task::tick().await;
        let output = host.query_selector("#selected-zone").ok().flatten();
        assert!(output.is_some());
        let Some(output) = output else {
            return;
        };
        assert_eq!(output.text_content().as_deref(), Some("UTC"));
        let choice = host
            .query_selector("#schedule-timezone-listbox [role=option]")
            .ok()
            .flatten();
        assert!(choice.is_some());
        let Some(choice) = choice else {
            return;
        };
        assert!(
            choice
                .text_content()
                .is_some_and(|label| label.contains("Asia/Kathmandu"))
        );
        let pointer = PointerEvent::new("pointerdown");
        assert!(pointer.is_ok());
        let Ok(pointer) = pointer else {
            return;
        };
        assert!(choice.dispatch_event(&pointer).is_ok());
        leptos::task::tick().await;
        assert_eq!(output.text_content().as_deref(), Some("Asia/Kathmandu"));
        drop(handle);
        host.remove();
    }
}
