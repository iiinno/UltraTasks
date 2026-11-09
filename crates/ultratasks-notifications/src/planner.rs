use crate::{DesiredNotification, Error, Result};
use chrono::{DateTime, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ultratasks_core::{
    CalendarQuery, DueDate, Engine, Entity, EntityId, EventSchedule, ReminderTrigger, expand_event,
    expand_task,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePolicy {
    pub enabled: bool,
    /// Explicit interpretation of date-only deadlines and all-day reminders.
    pub timezone: String,
    /// Native queues are bounded. Reconciliation extends this rolling horizon.
    pub horizon_days: u16,
    /// Newly discovered reminders this late are shown once; older ones expire.
    pub missed_grace_minutes: u16,
}

impl Default for DevicePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            timezone: "UTC".into(),
            horizon_days: 30,
            missed_grace_minutes: 15,
        }
    }
}

pub fn plan_notifications(
    engine: &Engine,
    policy: &DevicePolicy,
    now: DateTime<Utc>,
) -> Result<Vec<DesiredNotification>> {
    if !policy.enabled {
        return Ok(Vec::new());
    }
    let zone: Tz = policy
        .timezone
        .parse()
        .map_err(|_| Error::Invalid("IANA timezone required".into()))?;
    if !(1..=90).contains(&policy.horizon_days) || policy.missed_grace_minutes > 1440 {
        return Err(Error::Invalid(
            "horizon must be 1..90 days; grace at most 1440 minutes".into(),
        ));
    }
    let low = now
        .checked_sub_signed(Duration::minutes(i64::from(policy.missed_grace_minutes)))
        .ok_or_else(|| Error::Invalid("planning start overflow".into()))?;
    let high = now
        .checked_add_signed(Duration::days(i64::from(policy.horizon_days)))
        .ok_or_else(|| Error::Invalid("planning end overflow".into()))?;
    let records = engine.snapshots()?;
    let active: HashMap<_, _> = records
        .iter()
        .filter(|r| !r.deleted)
        .map(|r| (r.id(), &r.entity))
        .collect();
    let mut result = Vec::new();
    for record in &records {
        let Entity::Reminder(reminder) = &record.entity else {
            continue;
        };
        if record.deleted || !reminder.enabled {
            continue;
        }
        let Some(target) = active.get(&reminder.target) else {
            continue;
        };
        if matches!(target,Entity::Task(task) if task.completed) {
            continue;
        }
        let title = match target {
            Entity::Task(v) => &v.title,
            Entity::Event(v) => &v.title,
            Entity::TimeBlock(v) => match active.get(&EntityId::Task(v.task_id)) {
                Some(Entity::Task(task)) if !task.completed => &task.title,
                _ => continue,
            },
            _ => return Err(Error::Invalid("unsupported reminder target".into())),
        };
        let mut push =
            |fire_at: DateTime<Utc>, suffix: Option<NaiveDate>, title: &str| -> Result<()> {
                if fire_at < low || fire_at >= high {
                    return Ok(());
                }
                let id = match suffix {
                    Some(date) => format!("{}:{date}", reminder.id),
                    None => reminder.id.to_string(),
                };
                result.push(DesiredNotification {
                    id,
                    fire_at,
                    title: title.to_owned(),
                    body: format!(
                        "UltraTasks · {}",
                        fire_at.with_timezone(&zone).format("%Y-%m-%d %H:%M %Z")
                    ),
                });
                if result.len() > 4000 {
                    return Err(Error::Invalid(
                        "more than 4000 reminders in planning horizon; shorten horizon".into(),
                    ));
                }
                Ok(())
            };
        match reminder.trigger {
            ReminderTrigger::At(at) => push(at, None, title)?,
            ReminderTrigger::Before { minutes } => {
                let offset = Duration::minutes(i64::from(minutes));
                let shift = |at: DateTime<Utc>| {
                    at.checked_add_signed(offset)
                        .ok_or_else(|| Error::Invalid("reminder offset overflow".into()))
                };
                let query = CalendarQuery {
                    start: shift(low)?,
                    end: shift(high)?,
                    timezone: policy.timezone.clone(),
                    limit: 1000,
                };
                let fire = |at: DateTime<Utc>| {
                    at.checked_sub_signed(offset)
                        .ok_or_else(|| Error::Invalid("reminder time overflow".into()))
                };
                match target {
                    Entity::Event(event) => {
                        for occurrence in expand_event(event, &query)? {
                            let at = match occurrence.schedule {
                                EventSchedule::Timed { start, .. } => start,
                                EventSchedule::AllDay { start, .. } => day_start(start, zone)?,
                            };
                            push(
                                fire(at)?,
                                event.recurrence.as_ref().map(|_| occurrence.original_date),
                                &occurrence.title,
                            )?;
                        }
                    }
                    Entity::Task(task) => {
                        for occurrence in expand_task(task, &query)? {
                            if occurrence.completed {
                                continue;
                            }
                            let at = match occurrence.due {
                                DueDate::At { at, .. } => at,
                                DueDate::Date(date) => day_start(date, zone)?,
                            };
                            push(
                                fire(at)?,
                                task.recurrence.as_ref().map(|_| occurrence.original_date),
                                &occurrence.title,
                            )?;
                        }
                    }
                    Entity::TimeBlock(block) => push(fire(block.start)?, None, title)?,
                    _ => return Err(Error::Invalid("unsupported reminder target".into())),
                }
            }
        }
    }
    result.sort_by(|a, b| a.fire_at.cmp(&b.fire_at).then(a.id.cmp(&b.id)));
    Ok(result)
}

/// Midnight gaps advance to the first valid minute of the same civil day;
/// folds choose the earlier instant. Completely skipped civil days are explicit.
fn day_start(date: NaiveDate, zone: Tz) -> Result<DateTime<Utc>> {
    let midnight = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| Error::Invalid("invalid date".into()))?;
    for minute in 0..1440 {
        let local = midnight + Duration::minutes(minute);
        match zone.from_local_datetime(&local) {
            LocalResult::Single(at) => return Ok(at.with_timezone(&Utc)),
            LocalResult::Ambiguous(a, b) => return Ok(a.min(b).with_timezone(&Utc)),
            LocalResult::None => {}
        }
    }
    Err(Error::Invalid(format!("{date} does not exist in {zone}")))
}
