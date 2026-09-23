//! Bounded scheduler loop shared safely by multiple server instances.
//!
//! A database lease reserves a small set of due Schedules. Each Schedule is
//! committed independently so recurrence calculation never creates a large
//! transaction and uniqueness still protects an occurrence after lease races.

use super::{MisfireDecision, decide_misfire, next_cron_occurrence};
use crate::{
    application::{ControlPlaneStore, PlannedOccurrence, SchedulePlan},
    domain::{CatchupPolicy, MisfirePolicy, Schedule, ScheduleTiming},
};
use std::{sync::Arc, time::Duration};
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::time::{self as tokio_time, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

const BATCH_SIZE: u16 = 100;
const CLAIM_LEASE: Duration = Duration::from_secs(30);

/// Claim and plan due Schedules until shutdown.
pub async fn run_scheduler(store: Arc<dyn ControlPlaneStore>, cancellation: CancellationToken) {
    let owner = Uuid::now_v7();
    let mut interval = tokio_time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                info!("scheduler stopped");
                return;
            }
            _ = interval.tick() => {
                let schedules = match store
                    .claim_due_schedules(owner, BATCH_SIZE, CLAIM_LEASE)
                    .await
                {
                    Ok(schedules) => schedules,
                    Err(error) => {
                        warn!(%error, "failed to claim due Schedules");
                        continue;
                    }
                };
                for schedule in schedules {
                    match plan(&schedule, owner, OffsetDateTime::now_utc()) {
                        Ok(plan) => {
                            for occurrence in &plan.occurrences {
                                crate::metrics::global().scheduler_due.inc();
                                if occurrence.lateness_seconds > 0 {
                                    crate::metrics::global().scheduler_misfire.inc();
                                }
                                let observed = u32::try_from(occurrence.lateness_seconds)
                                    .unwrap_or(u32::MAX);
                                crate::metrics::global()
                                    .execution_lateness
                                    .observe(f64::from(observed));
                                if !occurrence.execute {
                                    crate::metrics::global().scheduler_skipped.inc();
                                }
                            }
                            if let Err(error) = store.commit_schedule_plan(&plan).await {
                                warn!(%error, schedule_id = %schedule.id.get(), "failed to commit Schedule plan");
                            }
                        }
                        Err(error) => {
                            warn!(%error, schedule_id = %schedule.id.get(), "failed to calculate Schedule recurrence");
                        }
                    }
                }
            }
        }
    }
}

fn plan(
    schedule: &Schedule,
    owner: Uuid,
    now: OffsetDateTime,
) -> Result<SchedulePlan, crate::scheduler::RecurrenceError> {
    let Some(first_due) = schedule.next_run_at else {
        return Ok(SchedulePlan {
            schedule_id: schedule.id,
            owner,
            occurrences: Vec::new(),
            next_run_at: None,
            disable: true,
        });
    };
    match &schedule.timing {
        ScheduleTiming::Once { .. } => Ok(SchedulePlan {
            schedule_id: schedule.id,
            owner,
            occurrences: vec![occurrence(schedule, first_due, now, None)],
            next_run_at: None,
            disable: true,
        }),
        ScheduleTiming::Cron {
            expression,
            timezone,
        } => plan_cron(schedule, owner, now, first_due, expression, timezone),
    }
}

fn plan_cron(
    schedule: &Schedule,
    owner: Uuid,
    now: OffsetDateTime,
    first_due: OffsetDateTime,
    expression: &str,
    timezone: &str,
) -> Result<SchedulePlan, crate::scheduler::RecurrenceError> {
    let mut occurrences = Vec::new();
    let next_run_at = match schedule.catchup_policy {
        CatchupPolicy::Skip => {
            occurrences.push(skipped(
                first_due,
                now,
                "catch-up policy skipped missed occurrences",
            ));
            next_cron_occurrence(expression, timezone, now)?
        }
        CatchupPolicy::RunOnce => {
            occurrences.push(occurrence(
                schedule,
                first_due,
                now,
                Some("coalesced missed occurrences"),
            ));
            next_cron_occurrence(expression, timezone, now)?
        }
        CatchupPolicy::CatchUp => {
            let oldest = now - TimeDuration::seconds(i64::from(schedule.max_catchup_age_seconds));
            let mut cursor = first_due;
            if cursor < oldest {
                occurrences.push(skipped(
                    cursor,
                    now,
                    "occurrences older than max_catchup_age were summarized",
                ));
                cursor = next_cron_occurrence(expression, timezone, oldest)?;
            }
            let limit = usize::from(schedule.max_catchup_runs);
            while cursor <= now && occurrences.len() < limit {
                occurrences.push(occurrence(schedule, cursor, now, None));
                cursor = next_cron_occurrence(expression, timezone, cursor)?;
            }
            if cursor <= now {
                occurrences.push(skipped(
                    cursor,
                    now,
                    "occurrences exceeding max_catchup_runs were summarized",
                ));
                next_cron_occurrence(expression, timezone, now)?
            } else {
                cursor
            }
        }
    };
    Ok(SchedulePlan {
        schedule_id: schedule.id,
        owner,
        occurrences,
        next_run_at: Some(next_run_at),
        disable: false,
    })
}

fn occurrence(
    schedule: &Schedule,
    scheduled_at: OffsetDateTime,
    now: OffsetDateTime,
    reason: Option<&str>,
) -> PlannedOccurrence {
    let decision = decide_misfire(
        schedule.misfire_policy,
        schedule.misfire_grace_seconds,
        scheduled_at,
        now,
    );
    match decision {
        MisfireDecision::Execute { lateness_seconds } => PlannedOccurrence {
            scheduled_at,
            execute: true,
            lateness_seconds,
            reason: reason.map(str::to_string),
            dispatch_deadline: dispatch_deadline(
                schedule.misfire_policy,
                schedule.misfire_grace_seconds,
                scheduled_at,
            ),
        },
        MisfireDecision::Skip { lateness_seconds } => PlannedOccurrence {
            scheduled_at,
            execute: false,
            lateness_seconds,
            reason: Some("misfire policy rejected the late occurrence".to_string()),
            dispatch_deadline: None,
        },
    }
}

fn skipped(scheduled_at: OffsetDateTime, now: OffsetDateTime, reason: &str) -> PlannedOccurrence {
    let lateness = (now - scheduled_at).whole_seconds().max(0);
    PlannedOccurrence {
        scheduled_at,
        execute: false,
        lateness_seconds: u64::try_from(lateness).unwrap_or(u64::MAX),
        reason: Some(reason.to_string()),
        dispatch_deadline: None,
    }
}

fn dispatch_deadline(
    policy: MisfirePolicy,
    grace_seconds: Option<u32>,
    scheduled_at: OffsetDateTime,
) -> Option<OffsetDateTime> {
    match policy {
        MisfirePolicy::RunLate => None,
        MisfirePolicy::Skip => Some(scheduled_at + TimeDuration::seconds(30)),
        MisfirePolicy::GracePeriod => {
            grace_seconds.map(|seconds| scheduled_at + TimeDuration::seconds(i64::from(seconds)))
        }
    }
}
