//! Read-only operator view of PostgreSQL and the execution pipeline.
//!
//! A bounded JSON snapshot is refreshed while this page is mounted. Shared
//! database counts are separated from API-instance poll and connection health,
//! so the browser never infers cluster-wide liveness from one process's state.

use crate::{api, components::PageHeader, navigation::AppRoute};
use crono_api::{DatabaseMonitorResource, MonitorResource, PipelineMonitorResource};
use leptos::prelude::*;
use leptos_router::components::A;
use std::time::Duration;

/// Show current system health and backlogs without retaining a metric history.
#[component]
pub fn MonitorPage() -> impl IntoView {
    let pending = RwSignal::new(false);
    let monitor = LocalResource::new(move || async move {
        pending.set(true);
        let result = api::monitor().await;
        pending.set(false);
        result
    });
    let interval = set_interval_with_handle(
        move || {
            if !pending.get_untracked() {
                monitor.refetch();
            }
        },
        Duration::from_secs(30),
    )
    .ok();
    let automatic = interval.is_some();
    on_cleanup(move || {
        if let Some(interval) = interval {
            interval.clear();
        }
    });

    view! {
        <div class="space-y-8">
            <PageHeader title="Monitor" description="Current PostgreSQL and execution-pipeline state for operators.">
                <button
                    type="button"
                    class="rounded-md bg-crono-primary px-3 py-2 text-sm font-medium text-white hover:bg-crono-primary-hover disabled:opacity-50"
                    disabled=move || pending.get()
                    on:click=move |_| {
                        if !pending.get_untracked() {
                            monitor.refetch();
                        }
                    }
                >{move || if pending.get() { "Refreshing…" } else { "Refresh" }}</button>
            </PageHeader>
            <p class="text-xs text-crono-muted">
                {if automatic { "Refreshes every 30 seconds while open. Values are snapshots, not historical trends." } else { "Automatic refresh is unavailable; use Refresh for a new snapshot." }}
            </p>
            {move || monitor.map(|result| match result {
                Ok(snapshot) => snapshot_view(snapshot.clone()).into_any(),
                Err(error) => view! {
                    <section role="alert" class="rounded-xl border border-crono-failed/30 bg-crono-surface px-6 py-8">
                        <h2 class="font-semibold text-crono-failed">"Monitor unavailable"</h2>
                        <p class="mt-2 text-sm text-crono-muted">{error.message.clone()}</p>
                    </section>
                }.into_any(),
            }).unwrap_or_else(|| view! {
                <section class="rounded-xl border border-crono-border bg-crono-surface px-6 py-8 text-sm text-crono-muted">"Loading system snapshot…"</section>
            }.into_any())}
        </div>
    }
}

/// Render one immutable API sample with source scope called out beside metrics.
fn snapshot_view(snapshot: MonitorResource) -> impl IntoView {
    let sampled_at = snapshot.sampled_at;
    let database_available = snapshot.database_available;
    let nats_available = snapshot.nats_available;
    let database = snapshot.database;
    let pipeline = snapshot.pipeline;
    let instance = snapshot.instance;
    view! {
        <div class="space-y-6">
            <p class="text-xs text-crono-muted">"Sampled at "<time datetime=sampled_at.clone()>{sampled_at.clone()}</time></p>
            <section aria-label="Service health" class="grid gap-4 sm:grid-cols-2">
                <StatusTile label="PostgreSQL" available=database_available detail="Control-plane durability" />
                <StatusTile label="NATS JetStream" available=nats_available detail="This API instance's dispatch transport" />
            </section>
            <section class="rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                    <h2 class="font-semibold text-crono-text">"PostgreSQL"</h2>
                    <p class="mt-1 text-xs text-crono-muted">"Database size includes all schemas. Pool usage belongs to this API instance."</p>
                </header>
                {database_view(database, database_available)}
            </section>
            <section class="rounded-xl border border-crono-border bg-crono-surface">
                <header class="border-b border-crono-border px-5 py-4 sm:px-6">
                    <h2 class="font-semibold text-crono-text">"Schedule and dispatch"</h2>
                    <p class="mt-1 text-xs text-crono-muted">"Database-backed counts are system-wide. Poll times belong to this API instance."</p>
                </header>
                {pipeline_view(pipeline)}
                <div class="grid gap-4 border-t border-crono-border px-5 py-5 sm:grid-cols-2 sm:px-6">
                    <MetricTile label="Scheduler DB poll" value=instance.scheduler_last_poll_at.unwrap_or_else(|| "Not observed yet".to_string()) detail="Last successful due-Schedule claim on this API instance" />
                    <MetricTile label="Publisher DB poll" value=instance.publisher_last_poll_at.unwrap_or_else(|| "Not observed yet".to_string()) detail="Last successful outbox claim on this API instance" />
                </div>
            </section>
            <nav aria-label="Investigate execution" class="flex flex-wrap gap-4 text-sm font-medium text-crono-primary">
                <A href=AppRoute::Schedules.path()>"View Schedules →"</A>
                <A href=AppRoute::Runs.path()>"View Runs →"</A>
                <A href=AppRoute::Workers.path()>"View Workers →"</A>
            </nav>
        </div>
    }
}

/// Do not reuse old database values after a failed sample.
fn database_view(database: Option<DatabaseMonitorResource>, available: bool) -> impl IntoView {
    match database {
        Some(database) => view! {
            <div class="grid gap-4 px-5 py-5 sm:grid-cols-2 xl:grid-cols-4 sm:px-6">
                <MetricTile label="Database size" value=format_bytes(database.size_bytes) detail="Current PostgreSQL database" />
                <MetricTile label="Database connections" value=database.connections.to_string() detail="Sessions connected to this database" />
                <MetricTile label="API pool open" value=format!("{} / {}", database.pool_connections, database.pool_max_connections) detail="This API instance's connection pool" />
                <MetricTile label="API pool idle" value=database.pool_idle_connections.to_string() detail="Open connections currently idle" />
            </div>
        }.into_any(),
        None => view! {
            <p role="status" class="px-5 py-8 text-sm text-crono-muted sm:px-6">
                {if available { "Database metrics are unavailable. Retry shortly." } else { "PostgreSQL is unavailable; database metrics cannot be sampled." }}
            </p>
        }.into_any(),
    }
}

/// Show durable backlogs or an explicit unavailable state.
fn pipeline_view(pipeline: Option<PipelineMonitorResource>) -> impl IntoView {
    match pipeline {
        Some(pipeline) => view! {
            <div class="grid gap-4 px-5 py-5 sm:grid-cols-2 xl:grid-cols-4 sm:px-6">
                <MetricTile label="Enabled Schedules" value=pipeline.enabled_schedules.to_string() detail="Schedules eligible for planning" />
                <MetricTile label="Due Schedules" value=pipeline.due_schedules.to_string() detail="Enabled Schedules past next_run_at" />
                <MetricTile label="Earliest occurrence" value=pipeline.earliest_next_run_at.unwrap_or_else(|| "None scheduled".to_string()) detail="May be overdue" />
                <MetricTile label="Pending dispatch" value=pipeline.outbox_pending.to_string() detail="Durable outbox rows not yet published" />
                <MetricTile label="Oldest dispatch age" value=format!("{} s", pipeline.outbox_oldest_seconds) detail="Zero when no dispatch is pending" />
                <MetricTile label="Queued Runs" value=pipeline.runs_queued.to_string() detail="JetStream acknowledged; waiting for execution" />
                <MetricTile label="Running Runs" value=pipeline.runs_running.to_string() detail="Claimed for execution" />
                <MetricTile label="Online workers" value=pipeline.online_workers.to_string() detail="Presence heartbeat within 30 seconds" />
                <MetricTile label="Active worker leases" value=pipeline.active_worker_leases.to_string() detail="Unexpired running Attempt leases" />
            </div>
        }.into_any(),
        None => view! {
            <p role="status" class="px-5 py-8 text-sm text-crono-muted sm:px-6">"Pipeline counts are unavailable until PostgreSQL can be sampled."</p>
        }.into_any(),
    }
}

/// Distinguish dependency availability from workload backlog numbers.
#[component]
fn StatusTile(label: &'static str, available: bool, detail: &'static str) -> impl IntoView {
    let (status, class) = if available {
        ("Available", "bg-emerald-50 text-emerald-700")
    } else {
        ("Unavailable", "bg-red-50 text-crono-failed")
    };
    view! {
        <div class="rounded-xl border border-crono-border bg-crono-surface p-5">
            <div class="flex items-center justify-between gap-3"><h2 class="font-semibold text-crono-text">{label}</h2><span class=format!("rounded-full px-2.5 py-1 text-xs font-medium {class}")>{status}</span></div>
            <p class="mt-2 text-xs text-crono-muted">{detail}</p>
        </div>
    }
}

/// Present a single labeled value in the existing Crono card style.
#[component]
fn MetricTile(label: &'static str, value: String, detail: &'static str) -> impl IntoView {
    view! {
        <div class="min-w-0 rounded-lg bg-crono-bg px-4 py-3">
            <p class="text-xs font-medium text-crono-muted">{label}</p>
            <p class="mt-1 break-all text-xl font-semibold text-crono-text">{value}</p>
            <p class="mt-1 text-xs text-crono-muted">{detail}</p>
        </div>
    }
}

/// Format binary bytes without floating-point precision loss for large databases.
fn format_bytes(value: u64) -> String {
    let units = [
        ("TiB", 1_u64 << 40),
        ("GiB", 1_u64 << 30),
        ("MiB", 1_u64 << 20),
        ("KiB", 1_u64 << 10),
    ];
    for (unit, divisor) in units {
        if value >= divisor {
            let tenths = (u128::from(value % divisor) * 10) / u128::from(divisor);
            return format!("{}.{} {unit}", value / divisor, tenths);
        }
    }
    format!("{value} B")
}
