//! Compact zero-state summaries for first-class Crono resources.
//!
//! Summary cards expose only counts supplied by their caller. The initial
//! overview passes truthful zero values; this component does not fabricate or
//! fetch data and can later receive counts from the public API client layer.

use super::{Card, Icon};
use crate::navigation::AppRoute;
use leptos::prelude::*;
use leptos_router::components::A;

/// Restrained visual accents available to overview resource summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryTone {
    Indigo,
    Sky,
    Teal,
    Violet,
}

impl SummaryTone {
    const fn classes(self) -> &'static str {
        match self {
            Self::Indigo => "bg-indigo-50 text-indigo-600",
            Self::Sky => "bg-sky-50 text-sky-600",
            Self::Teal => "bg-teal-50 text-teal-600",
            Self::Violet => "bg-violet-50 text-violet-600",
        }
    }
}

/// Render a linked resource count with an explicit empty-state explanation.
#[component]
pub fn SummaryCard(
    route: AppRoute,
    count: u64,
    empty_text: &'static str,
    tone: SummaryTone,
) -> impl IntoView {
    view! {
        <Card class="p-5">
            <div class="flex items-center gap-3">
                <span class=format!("inline-flex size-11 items-center justify-center rounded-lg {}", tone.classes())>
                    <Icon symbol=route.symbol() class="text-[22px]" />
                </span>
                <A
                    href=route.path()
                    attr:class="text-sm font-semibold text-crono-text hover:text-crono-primary focus-visible:rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary"
                >
                    {route.label()}
                </A>
            </div>
            <p class="mt-5 text-3xl font-semibold tracking-tight text-crono-text">{count}</p>
            <p class="mt-1 text-sm text-crono-muted">{empty_text}</p>
        </Card>
    }
}
