//! Five-field `cron-parser` recurrence with IANA timezone evaluation.
//!
//! Expressions are parsed when schedules enter planner batches, not once per
//! clock tick. UTC is the persisted cursor. Converting each result back to UTC
//! makes repeated local times distinct across a fall-back transition. The
//! bounded UTC scan compensates for parsers choosing only the earlier instant
//! when a local wall-clock minute occurs twice.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Tz;
use cron_parser::{parse as parse_cron, parse_field};
use std::{collections::BTreeSet, error::Error, fmt};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecurrenceError {
    InvalidExpression,
    InvalidTimezone,
    OutOfRange,
}

impl fmt::Display for RecurrenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidExpression => "cron expression must contain five valid fields",
            Self::InvalidTimezone => "timezone must be a valid IANA identifier",
            Self::OutOfRange => "timestamp is outside the supported range",
        })
    }
}

impl Error for RecurrenceError {}

/// Validate a five-field expression and IANA timezone.
///
/// # Errors
///
/// Returns a typed validation error for an invalid expression or timezone.
pub fn validate_cron(expression: &str, timezone: &str) -> Result<(), RecurrenceError> {
    parsed_fields(expression)?;
    let zone = timezone
        .parse::<Tz>()
        .map_err(|_| RecurrenceError::InvalidTimezone)?;
    parse_cron(expression, &Utc::now().with_timezone(&zone))
        .map(|_| ())
        .map_err(|_| RecurrenceError::InvalidExpression)
}

/// Return the first cron occurrence strictly after the supplied UTC instant.
///
/// # Errors
///
/// Returns when parsing fails, the timestamp is out of range, or recurrence is exhausted.
pub fn next_cron_occurrence(
    expression: &str,
    timezone: &str,
    after: OffsetDateTime,
) -> Result<OffsetDateTime, RecurrenceError> {
    let fields = parsed_fields(expression)?;
    let zone = timezone
        .parse::<Tz>()
        .map_err(|_| RecurrenceError::InvalidTimezone)?;
    let utc = DateTime::<Utc>::from_timestamp(after.unix_timestamp(), after.nanosecond())
        .ok_or(RecurrenceError::OutOfRange)?;
    let library_next = parse_cron(expression, &utc.with_timezone(&zone))
        .map_err(|_| RecurrenceError::InvalidExpression)?
        .with_timezone(&Utc);
    let next = first_utc_match(&fields, zone, utc, library_next)?;
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(
        next.timestamp_nanos_opt()
            .ok_or(RecurrenceError::OutOfRange)?,
    ))
    .map_err(|_| RecurrenceError::OutOfRange)
}

/// Find a repeated wall-clock occurrence that `cron`'s local-time iterator can
/// skip after a backward UTC-offset transition. Searching is bounded to the
/// longest plausible civil-time overlap; the library result remains the fast
/// path and the upper bound for ordinary recurrence gaps.
fn first_utc_match(
    fields: &CronFields,
    zone: Tz,
    after: DateTime<Utc>,
    library_next: DateTime<Utc>,
) -> Result<DateTime<Utc>, RecurrenceError> {
    const OVERLAP_SEARCH_HOURS: i64 = 26;

    let search_end = after
        .checked_add_signed(Duration::hours(OVERLAP_SEARCH_HOURS))
        .map_or(library_next, |end| end.min(library_next));
    let next_minute = after.timestamp().div_euclid(60).saturating_add(1);
    let mut candidate = DateTime::<Utc>::from_timestamp(next_minute.saturating_mul(60), 0)
        .ok_or(RecurrenceError::OutOfRange)?;

    while candidate <= search_end {
        if fields.matches(candidate.with_timezone(&zone)) {
            return Ok(candidate);
        }
        candidate = candidate
            .checked_add_signed(Duration::minutes(1))
            .ok_or(RecurrenceError::OutOfRange)?;
    }

    Ok(library_next)
}

struct CronFields {
    minutes: BTreeSet<u32>,
    hours: BTreeSet<u32>,
    days_of_month: BTreeSet<u32>,
    months: BTreeSet<u32>,
    days_of_week: BTreeSet<u32>,
}

impl CronFields {
    fn matches(&self, candidate: DateTime<Tz>) -> bool {
        self.minutes.contains(&candidate.minute())
            && self.hours.contains(&candidate.hour())
            && self.days_of_month.contains(&candidate.day())
            && self.months.contains(&candidate.month())
            && self
                .days_of_week
                .contains(&candidate.weekday().num_days_from_sunday())
    }
}

fn parsed_fields(expression: &str) -> Result<CronFields, RecurrenceError> {
    let fields = expression.split_whitespace().collect::<Vec<_>>();
    let [minutes, hours, days_of_month, months, days_of_week] = fields.as_slice() else {
        return Err(RecurrenceError::InvalidExpression);
    };
    let invalid = |_| RecurrenceError::InvalidExpression;
    Ok(CronFields {
        minutes: parse_field(minutes, 0, 59).map_err(invalid)?,
        hours: parse_field(hours, 0, 23).map_err(invalid)?,
        days_of_month: parse_field(days_of_month, 1, 31).map_err(invalid)?,
        months: parse_field(months, 1, 12).map_err(invalid)?,
        days_of_week: parse_field(days_of_week, 0, 6).map_err(invalid)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{next_cron_occurrence, validate_cron};
    use anyhow::Result;
    use time::{Date, Month, OffsetDateTime, Time};

    fn utc(year: i32, month: Month, day: u8, hour: u8, minute: u8) -> Result<OffsetDateTime> {
        Ok(Date::from_calendar_date(year, month, day)?
            .with_time(Time::from_hms(hour, minute, 0)?)
            .assume_utc())
    }

    #[test]
    fn rejects_non_five_field_expressions() {
        assert!(validate_cron("0 0 1 1 * 2027", "UTC").is_err());
        assert!(validate_cron("*/5 * * * *", "UTC").is_ok());
    }

    #[test]
    fn accepts_cron_parser_ranges_steps_and_weekday_names() {
        assert!(validate_cron("0 12-18/3 * * Mon-Fri", "UTC").is_ok());
        assert!(validate_cron("*/0 * * * *", "UTC").is_err());
        assert!(validate_cron("0 0 31 2 *", "UTC").is_err());
    }

    #[test]
    fn calculates_in_the_stored_timezone() -> Result<()> {
        let next = next_cron_occurrence(
            "0 9 * * *",
            "America/New_York",
            utc(2026, Month::January, 1, 13, 30)?,
        )?;
        assert_eq!(next, utc(2026, Month::January, 1, 14, 0)?);
        Ok(())
    }

    #[test]
    fn spring_forward_nonexistent_time_is_not_invented() -> Result<()> {
        let next = next_cron_occurrence(
            "30 2 * * *",
            "America/New_York",
            utc(2026, Month::March, 8, 5, 0)?,
        )?;
        assert_eq!(next, utc(2026, Month::March, 9, 6, 30)?);
        Ok(())
    }

    #[test]
    fn fall_back_repeated_time_has_two_utc_occurrences() -> Result<()> {
        let first = next_cron_occurrence(
            "30 1 * * *",
            "America/New_York",
            utc(2026, Month::November, 1, 4, 0)?,
        )?;
        let second = next_cron_occurrence("30 1 * * *", "America/New_York", first)?;
        assert_eq!(first, utc(2026, Month::November, 1, 5, 30)?);
        assert_eq!(second, utc(2026, Month::November, 1, 6, 30)?);
        Ok(())
    }
}
