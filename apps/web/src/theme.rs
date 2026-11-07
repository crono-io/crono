//! Light/dark theme state shared by the whole browser client.
//!
//! The `dark` class on `<html>` is the single source of truth: Tailwind's
//! `dark` variant and the semantic CSS variables in `style.css` both key off
//! it. An inline script in `index.html` resolves the initial class before first
//! paint (stored choice, else `prefers-color-scheme`), so [`ThemeState::init`]
//! only reads the class back into a signal and never decides the theme itself.
//! Toggling updates the signal; one effect then mirrors it to the class, the
//! `theme-color` meta tag, and `localStorage`. Storage failures (private mode,
//! blocked storage) are ignored: the theme still switches for the session.

use leptos::prelude::*;

/// Must match the key read by the inline script in `index.html`.
const STORAGE_KEY: &str = "crono-theme";
const LIGHT_COLOR: &str = "#fafafa";
const DARK_COLOR: &str = "#09090b";

/// Available color schemes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    /// Stable identifier persisted in `localStorage`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// The opposite scheme.
    pub const fn toggled(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}

/// Reactive handle provided through context by [`ThemeState::provide`].
#[derive(Clone, Copy)]
pub struct ThemeState {
    pub theme: RwSignal<Theme>,
}

impl ThemeState {
    /// Seed the signal from the class the bootstrap script already applied and
    /// install the effect that persists later changes. Call once from `App`.
    pub fn provide() {
        let initial = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.document_element())
            .map_or(Theme::Light, |root| {
                if root.class_list().contains("dark") {
                    Theme::Dark
                } else {
                    Theme::Light
                }
            });
        let theme = RwSignal::new(initial);
        Effect::new(move |previous: Option<()>| {
            apply(theme.get(), previous.is_some());
        });
        provide_context(Self { theme });
    }

    /// Fetch the shared state, falling back to a detached signal outside `App`.
    pub fn current() -> Self {
        use_context::<Self>().unwrap_or_else(|| Self {
            theme: RwSignal::new(Theme::Light),
        })
    }

    /// Switch to the other scheme immediately.
    pub fn toggle(self) {
        self.theme.update(|theme| *theme = theme.toggled());
    }
}

/// Mirror `theme` onto the document; persist only after a user-driven change so
/// first-time visitors keep following the system preference.
fn apply(theme: Theme, persist: bool) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(document) = window.document() {
        if let Some(root) = document.document_element() {
            let _ = root
                .class_list()
                .toggle_with_force("dark", theme == Theme::Dark);
        }
        if let Ok(Some(meta)) = document.query_selector("meta[name=\"theme-color\"]") {
            let color = match theme {
                Theme::Light => LIGHT_COLOR,
                Theme::Dark => DARK_COLOR,
            };
            let _ = meta.set_attribute("content", color);
        }
    }
    if persist && let Ok(Some(storage)) = window.local_storage() {
        let _ = storage.set_item(STORAGE_KEY, theme.as_str());
    }
}
