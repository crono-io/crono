//! Explicit late-occurrence policy.

use crate::domain::MisfirePolicy;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MisfireDecision {
    Execute { lateness_seconds: u64 },
    Skip { lateness_seconds: u64 },
}

/// Decide whether a due occurrence remains eligible at the supplied database time.
#[must_use]
pub fn decide_misfire(
    policy: MisfirePolicy,
    grace_seconds: Option<u32>,
    scheduled_at: OffsetDateTime,
    now: OffsetDateTime,
) -> MisfireDecision {
    let lateness = (now - scheduled_at).whole_seconds().max(0);
    let lateness_seconds = u64::try_from(lateness).unwrap_or(u64::MAX);
    let execute = match policy {
        MisfirePolicy::RunLate => true,
        MisfirePolicy::Skip => lateness_seconds <= 30,
        MisfirePolicy::GracePeriod => {
            grace_seconds.is_some_and(|grace| lateness_seconds <= u64::from(grace))
        }
    };
    if execute {
        MisfireDecision::Execute { lateness_seconds }
    } else {
        MisfireDecision::Skip { lateness_seconds }
    }
}

#[cfg(test)]
mod tests {
    use super::{MisfireDecision, decide_misfire};
    use crate::domain::MisfirePolicy;
    use time::{Duration, OffsetDateTime};

    #[test]
    fn run_late_never_discards_execution_intent() {
        let scheduled = OffsetDateTime::UNIX_EPOCH;
        assert_eq!(
            decide_misfire(
                MisfirePolicy::RunLate,
                None,
                scheduled,
                scheduled + Duration::hours(12),
            ),
            MisfireDecision::Execute {
                lateness_seconds: 43_200
            }
        );
    }

    #[test]
    fn grace_period_has_inclusive_boundary() {
        let scheduled = OffsetDateTime::UNIX_EPOCH;
        assert!(matches!(
            decide_misfire(
                MisfirePolicy::GracePeriod,
                Some(15),
                scheduled,
                scheduled + Duration::seconds(15),
            ),
            MisfireDecision::Execute { .. }
        ));
        assert!(matches!(
            decide_misfire(
                MisfirePolicy::GracePeriod,
                Some(15),
                scheduled,
                scheduled + Duration::seconds(16),
            ),
            MisfireDecision::Skip { .. }
        ));
    }

    #[test]
    fn skip_allows_only_small_planner_jitter() {
        let scheduled = OffsetDateTime::UNIX_EPOCH;
        assert!(matches!(
            decide_misfire(
                MisfirePolicy::Skip,
                None,
                scheduled,
                scheduled + Duration::seconds(31),
            ),
            MisfireDecision::Skip { .. }
        ));
    }
}
