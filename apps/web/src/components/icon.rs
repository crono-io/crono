//! Lucide icon rendering behind one replaceable component boundary.
//!
//! Every page renders icons through [`Icon`], which maps the typed [`AppIcon`]
//! inventory to `lucide-leptos` components inside one wrapper span. Centralizing
//! the match keeps accessibility semantics and `currentColor` styling consistent
//! and confines the Lucide dependency to this module.

use crate::navigation::AppIcon;
use leptos::prelude::*;
use lucide_leptos::{
    Activity, Boxes, CalendarClock, Check, ChevronDown, CircleCheck, CirclePlay, CircleX, Copy,
    Folder, GitBranch, Hash, Info, LayoutDashboard, ListOrdered, Moon, Play, RefreshCw, Repeat,
    RotateCcw, ScrollText, SearchX, Server, Settings, Sun, Target, Timer, Trash, User, Workflow,
};

/// Render one known Lucide icon with explicit accessibility semantics.
///
/// An absent label marks the icon decorative; a supplied label exposes the
/// wrapper as an image. Colors inherit `currentColor`, so callers style the
/// wrapper through `class` while `size` sets the rendered pixel box.
#[component]
pub fn Icon(
    icon: AppIcon,
    #[prop(default = 20)] size: usize,
    #[prop(optional)] class: &'static str,
    #[prop(optional, into)] label: Option<String>,
) -> impl IntoView {
    let classes = if class.is_empty() {
        "app-icon inline-flex shrink-0 items-center justify-center".to_string()
    } else {
        format!("app-icon inline-flex shrink-0 items-center justify-center {class}")
    };
    let aria_hidden = label.is_none().then_some("true");
    let role = label.as_ref().map(|_| "img");
    let glyph = match icon {
        AppIcon::Activity => view! { <Activity size=size /> }.into_any(),
        AppIcon::Boxes => view! { <Boxes size=size /> }.into_any(),
        AppIcon::CalendarClock => view! { <CalendarClock size=size /> }.into_any(),
        AppIcon::Check => view! { <Check size=size /> }.into_any(),
        AppIcon::ChevronDown => view! { <ChevronDown size=size /> }.into_any(),
        AppIcon::CircleCheck => view! { <CircleCheck size=size /> }.into_any(),
        AppIcon::CirclePlay => view! { <CirclePlay size=size /> }.into_any(),
        AppIcon::CircleX => view! { <CircleX size=size /> }.into_any(),
        AppIcon::Copy => view! { <Copy size=size /> }.into_any(),
        AppIcon::Hash => view! { <Hash size=size /> }.into_any(),
        AppIcon::Repeat => view! { <Repeat size=size /> }.into_any(),
        AppIcon::User => view! { <User size=size /> }.into_any(),
        AppIcon::Folder => view! { <Folder size=size /> }.into_any(),
        AppIcon::GitBranch => view! { <GitBranch size=size /> }.into_any(),
        AppIcon::Info => view! { <Info size=size /> }.into_any(),
        AppIcon::LayoutDashboard => view! { <LayoutDashboard size=size /> }.into_any(),
        AppIcon::ListOrdered => view! { <ListOrdered size=size /> }.into_any(),
        AppIcon::Moon => view! { <Moon size=size /> }.into_any(),
        AppIcon::Play => view! { <Play size=size /> }.into_any(),
        AppIcon::RefreshCw => view! { <RefreshCw size=size /> }.into_any(),
        AppIcon::RotateCcw => view! { <RotateCcw size=size /> }.into_any(),
        AppIcon::ScrollText => view! { <ScrollText size=size /> }.into_any(),
        AppIcon::SearchX => view! { <SearchX size=size /> }.into_any(),
        AppIcon::Server => view! { <Server size=size /> }.into_any(),
        AppIcon::Settings => view! { <Settings size=size /> }.into_any(),
        AppIcon::Sun => view! { <Sun size=size /> }.into_any(),
        AppIcon::Target => view! { <Target size=size /> }.into_any(),
        AppIcon::Timer => view! { <Timer size=size /> }.into_any(),
        AppIcon::Trash => view! { <Trash size=size /> }.into_any(),
        AppIcon::Workflow => view! { <Workflow size=size /> }.into_any(),
    };

    view! {
        <span class=classes aria-hidden=aria_hidden aria-label=label role=role>
            {glyph}
        </span>
    }
}
