//! Minimal GUI shell; application screens and API calls follow later.

use leptos::prelude::*;

/// Display the product identity and the inherited workspace release version.
#[component]
pub fn App() -> impl IntoView {
    view! {
        <main>
            <p class="eyebrow">"Distributed workload automation"</p>
            <h1>"Crono"</h1>
            <p>"Define the job centrally. Execute it where the capability exists."</p>
            <footer>"Version " <span id="version">{env!("CARGO_PKG_VERSION")}</span></footer>
        </main>
    }
}
