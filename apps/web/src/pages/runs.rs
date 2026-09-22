//! Live Run creation and `JetStream` dispatch observation.

use crate::{api, components::PageHeader};
use leptos::{prelude::*, task::spawn_local};

/// Submit qualified Job and Target names and inspect durable dispatch state.
#[component]
pub fn RunsPage() -> impl IntoView {
    let job = RwSignal::new(String::new());
    let target = RwSignal::new(String::new());
    let feedback = RwSignal::new(None::<String>);
    let runs = LocalResource::new(api::list_runs);
    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let job_name = job.get_untracked();
        let target_name = target.get_untracked();
        if job_name.is_empty() || target_name.is_empty() {
            feedback.set(Some("Enter qualified Job and Target names.".to_string()));
            return;
        }
        feedback.set(Some("Committing Run and dispatch message…".to_string()));
        spawn_local(async move {
            match api::create_run(job_name, target_name).await {
                Ok(run) => {
                    feedback.set(Some(format!("Created Run {}.", run.id)));
                    runs.refetch();
                }
                Err(error) => feedback.set(Some(error)),
            }
        });
    };

    view! {
        <div class="space-y-8">
            <PageHeader title="Runs" description="Create a durable Run, then observe its transition from pending dispatch to JetStream acknowledged." />
            <section class="rounded-xl border border-crono-border bg-crono-surface p-5 sm:p-6">
                <h2 class="text-base font-semibold text-crono-text">"Run a Job"</h2>
                <form class="mt-4 grid gap-3 sm:grid-cols-[1fr_1fr_auto]" on:submit=submit>
                    <input class="rounded-md border border-crono-border px-3 py-2 text-sm" placeholder="namespace/job" prop:value=move || job.get() on:input=move |event| job.set(event_target_value(&event)) />
                    <input class="rounded-md border border-crono-border px-3 py-2 text-sm" placeholder="namespace/target" prop:value=move || target.get() on:input=move |event| target.set(event_target_value(&event)) />
                    <button class="rounded-md bg-crono-primary px-4 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover" type="submit">"Run"</button>
                </form>
                <div class="mt-3 flex flex-wrap items-center gap-3">
                    {move || feedback.get().map(|message| view! { <p class="text-sm text-crono-muted" role="status">{message}</p> })}
                    <button class="text-sm font-medium text-crono-primary hover:text-crono-primary-hover" type="button" on:click=move |_| runs.refetch()>"Refresh status"</button>
                </div>
            </section>
            <section class="overflow-hidden rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6"><h2 class="font-semibold text-crono-text">"Recent Runs"</h2></header>
                {move || runs.map(|result| match result {
                    Ok(page) if page.items.is_empty() => view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"No Runs yet."</p> }.into_any(),
                    Ok(page) => view! { <ul class="divide-y divide-crono-border">{page.items.iter().map(|run| {
                        let status = match run.status { crono_api::RunStatus::PendingDispatch => "pending dispatch", crono_api::RunStatus::Dispatched => "dispatched" };
                        view! {
                            <li class="grid gap-2 px-5 py-4 sm:grid-cols-[1fr_auto] sm:px-6">
                                <div><p class="font-medium text-crono-text">{format!("{} → {}", run.job, run.target)}</p><code class="text-xs text-crono-muted">{run.id.to_string()}</code></div>
                                <span class="self-center rounded-full bg-crono-primary-soft px-2.5 py-1 text-xs font-medium text-crono-primary">{status}</span>
                            </li>
                        }
                    }).collect_view()}</ul> }.into_any(),
                    Err(error) => view! { <p class="px-6 py-10 text-center text-sm text-crono-failed">{error.clone()}</p> }.into_any(),
                }).unwrap_or_else(|| view! { <p class="px-6 py-10 text-center text-sm text-crono-muted">"Loading Runs…"</p> }.into_any())}
            </section>
        </div>
    }
}
