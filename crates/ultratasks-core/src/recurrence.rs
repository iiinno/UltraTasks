//! Bounded local-calendar recurrence calculations, independent of persistence.
use crate::{DueDate, Error, EventSchedule, Frequency, Recurrence, Result};
use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

fn invalid(message: &str) -> Error {
    Error::InvalidInput(message.into())
}

pub(crate) fn validate(rule: &Recurrence, anchor: NaiveDate) -> Result<()> {
    if rule.interval == 0
        || rule.interval > 10000
        || rule.count.is_some_and(|v| v == 0 || v > 10000)
    {
        return Err(invalid("recurrence interval/count is out of range"));
    }
    if rule
        .until
        .is_some_and(|v| v < anchor || !(1..=9999).contains(&v.year()))
    {
        return Err(invalid(
            "recurrence until must be a valid date on or after its start",
        ));
    }
    if let Frequency::Weekly { weekdays } = &rule.frequency {
        if weekdays.is_empty()
            || weekdays.len() > 7
            || weekdays.iter().any(|v| !(1..=7).contains(v))
        {
            return Err(invalid("weekly recurrence needs 1..=7 ISO weekdays"));
        }
        let mut sorted = weekdays.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() != weekdays.len()
            || !weekdays.contains(&(anchor.weekday().number_from_monday() as u8))
        {
            return Err(invalid(
                "weekly weekdays must be unique and include the anchor weekday",
            ));
        }
    }
    Ok(())
}

/// Produces at most 10,000 valid dates. Without COUNT this seeks directly to the
/// requested interval. COUNT requires a bounded scan from the anchor.
pub(crate) fn dates(
    anchor: NaiveDate,
    rule: &Recurrence,
    start: NaiveDate,
    end: NaiveDate,
    valid: impl Fn(NaiveDate) -> bool,
) -> Result<Vec<NaiveDate>> {
    validate(rule, anchor)?;
    if end <= start {
        return Ok(Vec::new());
    }
    let start = start.max(anchor);
    let end = rule
        .until
        .and_then(|d| d.succ_opt())
        .map_or(end, |until| end.min(until));
    if end <= start {
        return Ok(Vec::new());
    }
    let interval = i64::from(rule.interval);
    let seek = if rule.count.is_some() { anchor } else { start };
    let mut cycle: i64 = match rule.frequency {
        Frequency::Daily => (seek - anchor).num_days() / interval,
        Frequency::Weekly { .. } => (seek - anchor).num_days() / (7 * interval),
        Frequency::Monthly => ((i64::from(seek.year() - anchor.year()) * 12
            + i64::from(seek.month())
            - i64::from(anchor.month()))
            / interval)
            .max(0),
    };
    // One prior weekly cycle may contain weekdays after the anchor weekday.
    if matches!(rule.frequency, Frequency::Weekly { .. }) {
        cycle = cycle.saturating_sub(1);
    }
    let mut output = Vec::new();
    let mut generated = 0u32;
    for _ in 0..100_000 {
        let mut candidates = Vec::new();
        let cycle_start = match &rule.frequency {
            Frequency::Daily => {
                let Some(date) = anchor.checked_add_signed(Duration::days(cycle * interval)) else {
                    break;
                };
                candidates.push(date);
                date
            }
            Frequency::Weekly { weekdays } => {
                let Some(monday) = anchor.checked_sub_signed(Duration::days(i64::from(
                    anchor.weekday().num_days_from_monday(),
                ))) else {
                    break;
                };
                let Some(date) = monday.checked_add_signed(Duration::days(cycle * interval * 7))
                else {
                    break;
                };
                for weekday in weekdays {
                    if let Some(candidate) =
                        date.checked_add_signed(Duration::days(i64::from(*weekday) - 1))
                    {
                        candidates.push(candidate);
                    }
                }
                date
            }
            Frequency::Monthly => {
                let month =
                    i64::from(anchor.year()) * 12 + i64::from(anchor.month0()) + cycle * interval;
                let Ok(year) = i32::try_from(month.div_euclid(12)) else {
                    break;
                };
                let month = (month.rem_euclid(12) + 1) as u32;
                let Some(date) = NaiveDate::from_ymd_opt(year, month, 1) else {
                    break;
                };
                if let Some(candidate) = NaiveDate::from_ymd_opt(year, month, anchor.day()) {
                    candidates.push(candidate);
                }
                date
            }
        };
        if cycle_start >= end || cycle_start.year() > 9999 {
            return Ok(output);
        }
        candidates.sort_unstable();
        for candidate in candidates {
            if candidate < anchor || candidate >= end || !valid(candidate) {
                continue;
            }
            generated += 1;
            if rule.count.is_some_and(|count| generated > count) {
                return Ok(output);
            }
            if candidate >= start {
                output.push(candidate);
            }
            if output.len() > 10000 {
                return Err(invalid("recurrence result exceeds 10000 occurrences"));
            }
        }
        cycle += 1;
    }
    if cycle > 100_000 {
        return Err(invalid("recurrence expansion work limit exceeded"));
    }
    Ok(output)
}

pub(crate) fn event_anchor(schedule: &EventSchedule) -> Result<NaiveDate> {
    match schedule {
        EventSchedule::AllDay { start, .. } => Ok(*start),
        EventSchedule::Timed {
            start, timezone, ..
        } => {
            let zone: Tz = timezone
                .parse()
                .map_err(|_| invalid("invalid recurrence timezone"))?;
            Ok(start.with_timezone(&zone).date_naive())
        }
    }
}
pub(crate) fn due_anchor(due: &DueDate) -> Result<NaiveDate> {
    match due {
        DueDate::Date(date) => Ok(*date),
        DueDate::At { at, timezone } => {
            let zone: Tz = timezone
                .parse()
                .map_err(|_| invalid("invalid recurrence timezone"))?;
            Ok(at.with_timezone(&zone).date_naive())
        }
    }
}
fn shifted_instant(at: DateTime<Utc>, timezone: &str, date: NaiveDate) -> Option<DateTime<Utc>> {
    let zone: Tz = timezone.parse().ok()?;
    let local = at.with_timezone(&zone);
    if local.date_naive() == date {
        return Some(at);
    }
    // A gap has no instance. A fold resolves to the first instant, deterministically.
    zone.from_local_datetime(&date.and_time(local.time()))
        .earliest()
        .map(|v| v.with_timezone(&Utc))
}
pub(crate) fn event_at(schedule: &EventSchedule, date: NaiveDate) -> Option<EventSchedule> {
    match schedule {
        EventSchedule::AllDay { start, end } => {
            let end = date.checked_add_signed(*end - *start)?;
            if !(1..=9999).contains(&date.year()) || !(1..=9999).contains(&end.year()) {
                return None;
            }
            Some(EventSchedule::AllDay { start: date, end })
        }
        EventSchedule::Timed {
            start,
            end,
            timezone,
        } => {
            let shifted = shifted_instant(*start, timezone, date)?;
            let end = shifted.checked_add_signed(*end - *start)?;
            let zone: Tz = timezone.parse().ok()?;
            if !(1..=9999).contains(&shifted.year())
                || !(1..=9999).contains(&end.year())
                || !(1..=9999).contains(&end.with_timezone(&zone).year())
            {
                return None;
            }
            Some(EventSchedule::Timed {
                start: shifted,
                end,
                timezone: timezone.clone(),
            })
        }
    }
}
pub(crate) fn due_at(due: &DueDate, date: NaiveDate) -> Option<DueDate> {
    match due {
        DueDate::Date(_) => Some(DueDate::Date(date)),
        DueDate::At { at, timezone } => Some(DueDate::At {
            at: shifted_instant(*at, timezone, date)?,
            timezone: timezone.clone(),
        }),
    }
}

fn event_overlaps(
    schedule: &EventSchedule,
    query: &crate::CalendarQuery,
    date_start: NaiveDate,
    date_end: NaiveDate,
) -> bool {
    match schedule {
        EventSchedule::Timed { start, end, .. } => *start < query.end && *end > query.start,
        EventSchedule::AllDay { start, end } => *start < date_end && *end > date_start,
    }
}
fn due_in_window(
    due: &DueDate,
    query: &crate::CalendarQuery,
    date_start: NaiveDate,
    date_end: NaiveDate,
) -> bool {
    match due {
        DueDate::Date(date) => *date >= date_start && *date < date_end,
        DueDate::At { at, .. } => *at >= query.start && *at < query.end,
    }
}

/// Expand one series. An error, rather than silent truncation, signals a result
/// above query.limit. Overrides moved into the window are included independently
/// of their original date. Durations are elapsed time for timed events.
pub fn expand_event(
    event: &crate::Event,
    query: &crate::CalendarQuery,
) -> Result<Vec<crate::EventOccurrence>> {
    use crate::{EventException, EventOccurrence};
    crate::engine::validate_snapshot(&crate::storage::Snapshot {
        entity: crate::storage::Entity::Event(event.clone()),
        deleted: false,
    })?;
    let (date_start, date_end) = crate::engine::calendar_window(query)?;
    let anchor = event_anchor(&event.schedule)?;
    let mut result = Vec::new();
    let mut add = |date, title: String, schedule: EventSchedule| {
        if event_overlaps(&schedule, query, date_start, date_end) {
            result.push(EventOccurrence {
                event_id: event.id,
                original_date: date,
                title,
                schedule,
                calendar_id: event.calendar_id,
                revision: event.revision,
            });
        }
    };
    if let Some(rule) = &event.recurrence {
        let duration_days = match &event.schedule {
            EventSchedule::AllDay { start, end } => (*end - *start).num_days(),
            EventSchedule::Timed { start, end, .. } => (*end - *start).num_days() + 2,
        };
        let (series_start, series_end) = match &event.schedule {
            EventSchedule::Timed { timezone, .. } => {
                let zone: Tz = timezone
                    .parse()
                    .map_err(|_| invalid("invalid series timezone"))?;
                (
                    query.start.with_timezone(&zone).date_naive(),
                    query.end.with_timezone(&zone).date_naive(),
                )
            }
            EventSchedule::AllDay { .. } => (date_start, date_end),
        };
        let scan_start = series_start
            .checked_sub_signed(Duration::days(duration_days))
            .unwrap_or(NaiveDate::MIN)
            .max(anchor);
        let scan_end = series_end.succ_opt().unwrap_or(series_end);
        for date in dates(anchor, rule, scan_start, scan_end, |d| {
            event_at(&event.schedule, d).is_some()
        })? {
            if !event.exceptions.contains_key(&date)
                && let Some(schedule) = event_at(&event.schedule, date)
            {
                add(date, event.title.clone(), schedule);
            }
        }
        for (date, exception) in &event.exceptions {
            if let EventException::Override { title, schedule } = exception {
                add(
                    *date,
                    title.clone().unwrap_or_else(|| event.title.clone()),
                    schedule.clone(),
                );
            }
        }
    } else {
        add(anchor, event.title.clone(), event.schedule.clone());
    }
    result.sort_by_key(|v| match &v.schedule {
        EventSchedule::AllDay { start, .. } => (
            start.and_hms_opt(0, 0, 0).unwrap().and_utc(),
            v.original_date,
        ),
        EventSchedule::Timed { start, .. } => (*start, v.original_date),
    });
    if result.len() > query.limit as usize {
        return Err(invalid(
            "occurrence query exceeds limit; narrow the window or increase the limit",
        ));
    }
    Ok(result)
}

pub fn expand_task(
    task: &crate::Task,
    query: &crate::CalendarQuery,
) -> Result<Vec<crate::TaskOccurrence>> {
    use crate::{TaskException, TaskOccurrence};
    crate::engine::validate_snapshot(&crate::storage::Snapshot {
        entity: crate::storage::Entity::Task(task.clone()),
        deleted: false,
    })?;
    let (date_start, date_end) = crate::engine::calendar_window(query)?;
    let Some(due) = &task.due else {
        return Ok(Vec::new());
    };
    let anchor = due_anchor(due)?;
    let mut result = Vec::new();
    let mut add = |date, title: String, due: DueDate, completed: bool| {
        if due_in_window(&due, query, date_start, date_end) {
            result.push(TaskOccurrence {
                task_id: task.id,
                original_date: date,
                title,
                due,
                completed,
                revision: task.revision,
            });
        }
    };
    if let Some(rule) = &task.recurrence {
        let (series_start, series_end) = match due {
            DueDate::At { timezone, .. } => {
                let zone: Tz = timezone
                    .parse()
                    .map_err(|_| invalid("invalid series timezone"))?;
                (
                    query.start.with_timezone(&zone).date_naive(),
                    query.end.with_timezone(&zone).date_naive(),
                )
            }
            DueDate::Date(_) => (date_start, date_end),
        };
        let scan_start = series_start.pred_opt().unwrap_or(series_start);
        let scan_end = series_end.succ_opt().unwrap_or(series_end);
        for date in dates(anchor, rule, scan_start, scan_end, |d| {
            due_at(due, d).is_some()
        })? {
            if !task.exceptions.contains_key(&date)
                && let Some(due) = due_at(due, date)
            {
                add(date, task.title.clone(), due, task.completed);
            }
        }
        for (date, exception) in &task.exceptions {
            if let TaskException::Override {
                title,
                due,
                completed,
            } = exception
            {
                add(
                    *date,
                    title.clone().unwrap_or_else(|| task.title.clone()),
                    due.clone(),
                    *completed,
                );
            }
        }
    } else {
        add(anchor, task.title.clone(), due.clone(), task.completed);
    }
    result.sort_by_key(|v| match &v.due {
        DueDate::Date(date) => date.and_hms_opt(0, 0, 0).unwrap().and_utc(),
        DueDate::At { at, .. } => *at,
    });
    if result.len() > query.limit as usize {
        return Err(invalid(
            "occurrence query exceeds limit; narrow the window or increase the limit",
        ));
    }
    Ok(result)
}

/// COUNT rules scan from the anchor only once for all exceptions. Without COUNT,
/// each key seeks directly to its cycle, so widely separated keys remain bounded.
pub(crate) fn validate_exception_dates(
    anchor: NaiveDate,
    rule: &Recurrence,
    keys: &[NaiveDate],
    valid: impl Fn(NaiveDate) -> bool,
) -> Result<()> {
    let Some(first) = keys.first().copied() else {
        return Ok(());
    };
    let last = keys.last().copied().unwrap();
    let end = last
        .succ_opt()
        .ok_or_else(|| invalid("exception date overflow"))?;
    if rule.count.is_some() {
        let generated = dates(anchor, rule, first, end, &valid)?;
        if keys.iter().any(|key| generated.binary_search(key).is_err()) {
            return Err(invalid("exception date is not an occurrence in the series"));
        }
    } else {
        for key in keys {
            let end = key
                .succ_opt()
                .ok_or_else(|| invalid("exception date overflow"))?;
            if dates(anchor, rule, *key, end, &valid)?.is_empty() {
                return Err(invalid("exception date is not an occurrence in the series"));
            }
        }
    }
    Ok(())
}
