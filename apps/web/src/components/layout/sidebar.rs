//! Canonical sidebar rendering and active-route presentation.
//!
//! The current URL determines active links and initially opens the matching
//! resource submenu. Children are static navigation actions, not resource data;
//! the same metadata can support other resource types later.

use crate::{
    components::Icon,
    navigation::{NAVIGATION_GROUPS, is_active_path, is_section_active_path},
};
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_location};

// Active routes are marked by neutral weight/background plus a thin accent bar,
// never by accent-colored text. Hover and expansion stay unaccented so they
// cannot be mistaken for the current route.
const ACTIVE_CLASSES: &str = "border-primary bg-sidebar-accent text-sidebar-foreground font-semibold [&_.app-icon]:text-sidebar-foreground";
const INACTIVE_CLASSES: &str = "border-transparent text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground [&_.app-icon]:text-muted-foreground hover:[&_.app-icon]:text-sidebar-foreground";
const CHILD_ACTIVE_CLASSES: &str = "bg-sidebar-accent/70 font-semibold text-sidebar-foreground";
const CHILD_INACTIVE_CLASSES: &str =
    "text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground";

fn item_classes(active: bool) -> &'static str {
    if active {
        ACTIVE_CLASSES
    } else {
        INACTIVE_CLASSES
    }
}

/// Render product identity, grouped navigation, and release version.
#[component]
pub fn Sidebar() -> impl IntoView {
    let location = use_location();

    view! {
        <aside class="flex flex-col border-b border-border bg-sidebar text-sidebar-foreground lg:h-full lg:min-h-0 lg:border-b-0 lg:border-r">
            <div class="flex min-h-0 flex-1 flex-col px-4 py-5 lg:px-5 lg:py-6">
                <A href="/" attr:class="mb-6 flex shrink-0 items-center gap-3 rounded-md px-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-sidebar lg:mb-6">
                    <span class="flex size-8 items-center justify-center rounded-lg bg-primary" aria-hidden="true">
                        <img
                            src="/crono-logo-white.svg"
                            alt=""
                            class="size-6"
                        />
                    </span>
                    <span class="text-lg font-semibold tracking-tight text-sidebar-foreground">"Crono"</span>
                </A>

                <nav aria-label="Primary" class="grid grid-cols-2 gap-x-3 gap-y-6 sm:grid-cols-4 lg:-mx-1 lg:block lg:min-h-0 lg:flex-1 lg:space-y-7 lg:overflow-y-auto lg:overscroll-contain lg:px-1">
                    {NAVIGATION_GROUPS.into_iter().map(|group| {
                        view! {
                            <div>
                                {group.label.map(|label| view! {
                                    <h2 class="mb-2.5 px-3 text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground">{label}</h2>
                                })}
                                <ul class="space-y-1">
                                    {group.routes.iter().copied().map(|route| {
                                        let pathname = location.pathname;
                                        let children = route.children();
                                        let submenu_id = route.submenu_id();
                                        let section_active = move || is_section_active_path(&pathname.get(), route);
                                        // Seed once from the current URL; later route changes reopen the
                                        // section through the effect below, so this read is intentionally untracked.
                                        let expanded = RwSignal::new(is_section_active_path(&pathname.get_untracked(), route));
                                        Effect::new(move |_| {
                                            if section_active() {
                                                expanded.set(true);
                                            }
                                        });
                                        view! {
                                            <li>
                                                <div class="flex items-center">
                                                    <A
                                                        href=route.path()
                                                        on:click=move |_| expanded.set(true)
                                                        attr:class=move || format!(
                                                            "group flex min-w-0 flex-1 items-center gap-3 rounded-r-md border-l-2 px-3 py-2.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-sidebar {}",
                                                            item_classes(section_active())
                                                        )
                                                        attr:aria-current=move || is_active_path(&pathname.get(), route).then_some("page")
                                                    >
                                                        <Icon icon=route.symbol() />
                                                        <span>{route.label()}</span>
                                                    </A>
                                                    {(!children.is_empty()).then(|| view! {
                                                        <button
                                                            type="button"
                                                            aria-label=format!("Toggle {} submenu", route.label())
                                                            aria-controls=submenu_id
                                                            aria-expanded=move || expanded.get().to_string()
                                                            on:click=move |_| expanded.update(|value| *value = !*value)
                                                            class="ml-1 shrink-0 rounded-md p-2 text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
                                                        ><span class=move || if expanded.get() { "inline-flex" } else { "inline-flex -rotate-90" }><Icon icon=crate::navigation::AppIcon::ChevronDown size=18 /></span></button>
                                                    })}
                                                </div>
                                                <Show when=move || !children.is_empty() && expanded.get()>
                                                    <ul id=submenu_id class="ml-5 mt-1 space-y-1 border-l border-border pl-2">
                                                        {children.iter().copied().map(|child| {
                                                            view! {
                                                                <li>
                                                                    <A
                                                                        href=child.route.path()
                                                                        attr:class=move || format!("block rounded-md px-3 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary {}", if is_active_path(&pathname.get(), child.route) { CHILD_ACTIVE_CLASSES } else { CHILD_INACTIVE_CLASSES })
                                                                        attr:aria-current=move || is_active_path(&pathname.get(), child.route).then_some("page")
                                                                    >{child.label}</A>
                                                                </li>
                                                            }
                                                        }).collect_view()}
                                                    </ul>
                                                </Show>
                                            </li>
                                        }
                                    }).collect_view()}
                                </ul>
                            </div>
                        }
                    }).collect_view()}
                </nav>

                <div class="mt-4 hidden shrink-0 border-t border-border px-2 pt-4 text-center text-xs text-muted-foreground lg:block">
                    <span>"crono "</span>
                    <span id="version">{env!("CARGO_PKG_VERSION")}</span>
                </div>
            </div>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::{ACTIVE_CLASSES, INACTIVE_CLASSES, item_classes};

    #[test]
    fn active_and_inactive_navigation_styles_are_distinct() {
        assert_eq!(item_classes(true), ACTIVE_CLASSES);
        assert_eq!(item_classes(false), INACTIVE_CLASSES);
        assert_ne!(ACTIVE_CLASSES, INACTIVE_CLASSES);
    }
}
