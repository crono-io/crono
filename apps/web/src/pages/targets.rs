//! Routed Target browsing, creation, and editing.
//!
//! Browsing uses Namespace-scoped cursor pages. Create and edit share the same
//! form, while a direct edit URL fetches its Target by ID before mounting the
//! form so a refresh never depends on prior list state.

mod form;
mod list;

pub use list::TargetsPage;

use crate::{
    api,
    components::{PageHeader, QUIET_ACTION_CLASS},
};
use form::TargetForm;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

/// Render Target creation without mounting the browse list.
#[component]
pub fn CreateTargetPage() -> impl IntoView {
    view! { <TargetForm /> }
}

/// Load the authorized Target identified by the edit URL.
#[component]
pub fn EditTargetPage() -> impl IntoView {
    let params = use_params_map();
    let target = LocalResource::new(move || {
        let id = params
            .get()
            .get("target_id")
            .and_then(|value| Uuid::parse_str(&value).ok());
        async move {
            match id {
                Some(id) => api::get_target(id).await,
                None => Err(api::ApiError {
                    code: "invalid_target_id".to_string(),
                    message: "Invalid Target URL.".to_string(),
                    field: None,
                }),
            }
        }
    });
    view! {
        {move || target.map(|result| match result {
            Ok(target) => view! { <TargetForm initial_target=target.clone() /> }.into_any(),
            Err(error) => view! {
                <div class="space-y-6">
                    <PageHeader title="Edit Target" description="Update an existing Target." />
                    <div class="rounded-xl border border-crono-border bg-crono-surface p-6">
                        <p class="text-sm text-crono-failed" role="alert">{error.message.clone()}</p>
                        <div class="mt-4"><A href="/targets" attr:class=QUIET_ACTION_CLASS>"Back to Targets"</A></div>
                    </div>
                </div>
            }.into_any(),
        }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Target…"</p> }.into_any())}
    }
}
