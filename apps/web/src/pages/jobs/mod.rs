//! Routed Job browsing, creation, and editing.
//!
//! The browse route never owns form state. Create and edit share one form, but
//! editing first fetches the authorized Job by URL identity so a refresh or a
//! direct link always restores the right values. All transport stays in `api`.

mod form;
mod list;
mod preview;

pub use list::JobsPage;

use crate::{api, components::PageHeader};
use form::JobForm;
use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};
use uuid::Uuid;

/// Explicit authoring route, separate from browsing existing Jobs.
#[component]
pub fn CreateJobPage() -> impl IntoView {
    view! { <JobForm /> }
}

/// Load a Job by URL before mounting the edit form; never infer it from list state.
#[component]
pub fn EditJobPage() -> impl IntoView {
    let params = use_params_map();
    let job = LocalResource::new(move || {
        let id = params
            .get()
            .get("job_id")
            .and_then(|value| Uuid::parse_str(&value).ok());
        async move {
            match id {
                Some(id) => api::get_job(id).await,
                None => Err(api::ApiError {
                    code: "invalid_job_id".to_string(),
                    message: "Invalid Job URL.".to_string(),
                    field: None,
                }),
            }
        }
    });
    view! {
        {move || job.map(|result| match result {
            Ok(job) => view! { <JobForm initial_job=job.clone() /> }.into_any(),
            Err(error) => view! {
                <div class="space-y-6">
                    <PageHeader title="Edit Job" description="Update an existing Job." />
                    <div class="rounded-xl border border-crono-border bg-crono-surface p-6">
                        <p class="text-sm text-crono-failed" role="alert">{error.message.clone()}</p>
                        <A href="/jobs" attr:class="mt-4 inline-block text-sm font-medium text-crono-primary hover:text-crono-primary-hover">"Back to Jobs"</A>
                    </div>
                </div>
            }.into_any(),
        }).unwrap_or_else(|| view! { <p class="text-sm text-crono-muted">"Loading Job…"</p> }.into_any())}
    }
}
