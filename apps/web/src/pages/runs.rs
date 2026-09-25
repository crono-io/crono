//! Run history, manual invocation, and authorized details pages.
//!
//! History queries are server-filtered and paginated. Re-run requests copy the
//! server's immutable execution snapshot, so the browser never reconstructs
//! secret-bearing invocation data. Attempt output is fetched only on demand.

mod details;
mod history;
mod new;
mod output;

pub use details::RunDetailsPage;
pub use history::RunsPage;
pub use new::RunJobPage;

use crono_api::{RunResource, RunStatus, RunTriggerSource};

pub(super) const ACTION_CLASS: &str = "inline-flex items-center justify-center rounded-md bg-crono-primary px-4 py-2.5 text-sm font-medium text-white shadow-sm hover:bg-crono-primary-hover focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2";
pub(super) const RUN_ACTION_CLASS: &str = "inline-flex min-h-9 items-center gap-1.5 rounded-md px-2 py-1.5 text-sm font-medium text-crono-primary hover:bg-crono-primary-soft focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-crono-primary focus-visible:ring-offset-2";

/// Text remains present alongside status color for accessible scanning.
pub(super) const fn status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::PendingDispatch => "Pending dispatch",
        RunStatus::Queued => "Queued",
        RunStatus::Running => "Running",
        RunStatus::RetryWait => "Retry wait",
        RunStatus::Succeeded => "Succeeded",
        RunStatus::Failed => "Failed",
        RunStatus::Dead => "Dead",
        RunStatus::Skipped => "Skipped",
        RunStatus::Cancelled => "Cancelled",
        RunStatus::Unknown => "Unknown",
    }
}

pub(super) const fn run_status(status: RunStatus) -> &'static str {
    status_label(status)
}

pub(super) const fn status_class(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Succeeded => "bg-emerald-100 text-emerald-800",
        RunStatus::Failed | RunStatus::Dead => "bg-red-100 text-red-800",
        RunStatus::Running => "bg-blue-100 text-blue-800",
        RunStatus::RetryWait => "bg-amber-100 text-amber-800",
        RunStatus::Cancelled | RunStatus::Skipped => "bg-zinc-200 text-zinc-700",
        RunStatus::PendingDispatch | RunStatus::Queued | RunStatus::Unknown => {
            "bg-crono-primary-soft text-crono-primary"
        }
    }
}

/// Only nonterminal Runs can transition; terminal outcomes are immutable in the server.
pub(super) const fn status_can_change(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::PendingDispatch | RunStatus::Queued | RunStatus::Running | RunStatus::RetryWait
    )
}

pub(super) fn trigger_label(run: &RunResource) -> String {
    if let Some(actor) = &run.trigger_actor {
        return actor.clone();
    }
    match run.trigger_source {
        RunTriggerSource::Unknown => "Unknown".to_string(),
        RunTriggerSource::Api => "API".to_string(),
        RunTriggerSource::Scheduler => "Scheduler".to_string(),
        RunTriggerSource::Rerun => "Re-run".to_string(),
    }
}

pub(super) fn run_namespace(run: &RunResource) -> &str {
    if run.namespace.is_empty() {
        run.job
            .split_once('/')
            .map_or("—", |(namespace, _)| namespace)
    } else {
        &run.namespace
    }
}

pub(super) fn run_job_name(run: &RunResource) -> &str {
    if run.job_name.is_empty() {
        run.job.rsplit('/').next().unwrap_or(&run.job)
    } else {
        &run.job_name
    }
}

pub(super) fn run_target_name(run: &RunResource) -> &str {
    if run.target_name.is_empty() {
        run.target.rsplit('/').next().unwrap_or(&run.target)
    } else {
        &run.target_name
    }
}

pub(super) fn run_triggered_at(run: &RunResource) -> &str {
    if run.triggered_at.is_empty() {
        &run.created_at
    } else {
        &run.triggered_at
    }
}

/// Present the API's exact UTC timestamp in a compact, sortable form.
pub(super) fn display_time(value: &str) -> String {
    match (value.get(..10), value.as_bytes().get(10), value.get(11..16)) {
        (Some(date), Some(b'T'), Some(clock)) => format!("{date} {clock} UTC"),
        _ => value.to_string(),
    }
}

/// Keep sub-second server ordering visible in the lifecycle timeline.
pub(super) fn display_event_time(value: &str) -> String {
    let clock = value.get(11..23).or_else(|| value.get(11..19));
    clock.map_or_else(|| value.to_string(), |clock| format!("{clock} UTC"))
}

pub(super) fn display_duration(milliseconds: Option<u64>) -> String {
    match milliseconds {
        None => "—".to_string(),
        Some(ms) if ms < 1_000 => format!("{ms}ms"),
        Some(ms) if ms < 60_000 => format!("{}.{:01}s", ms / 1_000, ms % 1_000 / 100),
        Some(ms) => format!("{}m {}s", ms / 60_000, ms % 60_000 / 1_000),
    }
}

/// Split a UTC API timestamp into a prominent clock and quieter date for history rows.
/// Unexpected timestamp formats remain visible verbatim instead of being mislabeled UTC.
pub(super) fn history_time_parts(value: &str) -> (String, String) {
    match (value.get(..10), value.as_bytes().get(10), value.get(11..19)) {
        (Some(date), Some(b'T'), Some(clock)) if value.ends_with('Z') => {
            (format!("{clock} UTC"), date.to_string())
        }
        _ => (value.to_string(), String::new()),
    }
}

#[cfg(test)]
mod history_time_tests {
    use super::{history_time_parts, status_can_change};
    use crono_api::RunStatus;
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn history_time_keeps_seconds_and_utc_date() {
        assert_eq!(
            history_time_parts("2026-09-25T12:28:01.123456Z"),
            ("12:28:01 UTC".to_string(), "2026-09-25".to_string())
        );
    }

    #[wasm_bindgen_test]
    fn history_time_does_not_mislabel_unknown_timezone() {
        assert_eq!(
            history_time_parts("2026-09-25T12:28:01+02:00"),
            ("2026-09-25T12:28:01+02:00".to_string(), String::new())
        );
    }

    #[wasm_bindgen_test]
    fn status_refresh_is_available_only_while_run_can_change() {
        for status in [
            RunStatus::PendingDispatch,
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::RetryWait,
        ] {
            assert!(status_can_change(status));
        }
        for status in [
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::Dead,
            RunStatus::Skipped,
            RunStatus::Cancelled,
            RunStatus::Unknown,
        ] {
            assert!(!status_can_change(status));
        }
    }
}
