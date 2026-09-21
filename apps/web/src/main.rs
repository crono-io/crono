//! Browser entrypoint for the independently hosted Crono GUI.
//!
//! The native entrypoint exists only for workspace tooling. Browser builds own
//! the DOM and will eventually call the server's public HTTPS API.

#[cfg(target_arch = "wasm32")]
mod app;

/// Mount the client-side application.
#[cfg(target_arch = "wasm32")]
fn main() {
    leptos::mount::mount_to_body(app::App);
}

/// Keep native workspace checks independent of browser dependencies.
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
