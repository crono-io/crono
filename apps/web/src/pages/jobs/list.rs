//! Namespace-filtered Job browsing and explicit edit navigation.
//!
//! The API currently lists Jobs inside one Namespace, so an unselected filter
//! is an instructional state rather than an invented cross-Namespace result.

use super::super::resource_options;
use crate::{
    api,
    components::{EmptyState, PageHeader, ResourceSelect},
    navigation::{AppRoute, MaterialSymbol, job_edit_path},
};
use crono_api::{JobResource, Page};
use leptos::prelude::*;
use leptos_router::components::A;
use uuid::Uuid;

const ACTION_CLASS: &str = "inline-flex items-center justify-center rounded-md bg-crono-primary px-4 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2";

/// Browse Jobs in a selected Namespace without mounting the create form.
#[component]
pub fn JobsPage() -> impl IntoView {
    let namespace_id = RwSignal::new(None);
    let namespace_choices = resource_options::namespaces();
    let jobs = LocalResource::new(move || load_jobs(namespace_id.get()));

    view! {
        <div class="space-y-8">
            <PageHeader title="Jobs" description="Manage executable templates and execution policies.">
                <A href=AppRoute::JobsNew.path() attr:class=ACTION_CLASS>"+ Create Job"</A>
            </PageHeader>
            <div class="max-w-xl">
                <ResourceSelect id="jobs-namespace-filter" label="Namespace" placeholder="Select a Namespace to browse Jobs…" options=namespace_choices.options selected=namespace_id loading=namespace_choices.loading load_error=namespace_choices.load_error optional=true />
            </div>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                {move || {
                    if namespace_choices.loading.get() {
                        view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Namespaces…"</p> }.into_any()
                    } else if namespace_choices.load_error.get().is_some() {
                        view! { <p class="px-6 py-10 text-center text-sm text-crono-failed" role="alert">"Namespaces could not be loaded. Retry by refreshing the page."</p> }.into_any()
                    } else if namespace_choices.options.get().is_empty() {
                        view! {
                            <EmptyState icon=MaterialSymbol::Work title="No Namespaces yet" description="Create a Namespace before adding Jobs.">
                                <A href=AppRoute::Namespaces.path() attr:class=ACTION_CLASS>"Go to Namespaces"</A>
                            </EmptyState>
                        }.into_any()
                    } else if namespace_id.get().is_none() {
                        view! { <EmptyState icon=MaterialSymbol::Work title="Select a Namespace" description="Choose a Namespace above to browse and manage its Jobs." /> }.into_any()
                    } else {
                        view! { <ResourceList jobs=jobs /> }.into_any()
                    }
                }}
            </section>
        </div>
    }
}

/// Preserve list loading and API errors while making the empty state actionable.
#[component]
fn ResourceList(jobs: LocalResource<api::ApiResult<Page<JobResource>>>) -> impl IntoView {
    view! {
        {move || jobs.map(|result| match result {
            Ok(page) if page.items.is_empty() => view! {
                <EmptyState icon=MaterialSymbol::Work title="No Jobs yet" description="Create the first Job in this Namespace to define what workers execute.">
                    <A href=AppRoute::JobsNew.path() attr:class=ACTION_CLASS>"Create Job"</A>
                </EmptyState>
            }.into_any(),
            Ok(page) => view! {
                <ul class="divide-y divide-crono-border">
                    {page.items.iter().cloned().map(|job| {
                        let edit_path = job_edit_path(job.id);
                        view! {
                            <li class="flex flex-wrap items-center justify-between gap-4 px-5 py-4 sm:px-6">
                                <div class="min-w-0">
                                    <p class="font-medium text-crono-text">{job.name}</p>
                                    <p class="text-sm text-crono-muted">{job.qualified_name}</p>
                                    <p class="mt-1 text-xs text-crono-muted">{format!("{:?} · {} · {} argv items", job.executor, job.queue, job.arguments.len())}</p>
                                </div>
                                <A href=edit_path attr:class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary">"Edit →"</A>
                            </li>
                        }
                    }).collect_view()}
                </ul>
            }.into_any(),
            Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed" role="alert">{error.message.clone()}</p> }.into_any(),
        }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Jobs…"</p> }.into_any())}
    }
}

/// Keep the existing Namespace-scoped API and its bounded page size.
async fn load_jobs(namespace: Option<Uuid>) -> api::ApiResult<Page<JobResource>> {
    match namespace {
        Some(id) => api::list_jobs(id).await,
        None => Ok(Page {
            items: Vec::new(),
            next_cursor: None,
        }),
    }
}
