//! Material Symbols rendering behind one replaceable component boundary.

use crate::navigation::MaterialSymbol;
use leptos::prelude::*;

/// Render one known Material Symbol with explicit accessibility semantics.
///
/// An absent label marks the symbol decorative. A supplied label exposes the
/// symbol as an image, though controls and navigation must still provide text.
#[component]
pub fn Icon(
    symbol: MaterialSymbol,
    #[prop(optional)] class: &'static str,
    #[prop(optional, into)] label: Option<String>,
) -> impl IntoView {
    let classes = if class.is_empty() {
        "material-symbols-outlined".to_string()
    } else {
        format!("material-symbols-outlined {class}")
    };
    let aria_hidden = label.is_none().then_some("true");
    let role = label.as_ref().map(|_| "img");

    view! {
        <span class=classes aria-hidden=aria_hidden aria-label=label role=role>
            {symbol.as_str()}
        </span>
    }
}
