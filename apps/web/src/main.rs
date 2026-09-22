//! Browser entrypoint for the independently hosted Crono GUI.
//!
//! The native entrypoint exists only for workspace tooling. Browser builds own
//! the DOM and will eventually call the server's public HTTPS API. The pure
//! navigation model is shared with native unit tests without introducing a
//! browser or server dependency.

#[cfg(any(target_arch = "wasm32", test))]
mod navigation;

#[cfg(target_arch = "wasm32")]
mod api;
#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod components;
#[cfg(target_arch = "wasm32")]
mod pages;
#[cfg(target_arch = "wasm32")]
mod routing;

/// Mount the client-side application.
#[cfg(target_arch = "wasm32")]
fn main() {
    leptos::mount::mount_to_body(app::App);
}

/// Keep native workspace checks independent of browser dependencies.
#[cfg(not(target_arch = "wasm32"))]
fn main() {}
