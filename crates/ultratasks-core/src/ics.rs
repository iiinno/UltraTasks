//! Deliberately strict iCalendar subset. Unsupported user data is rejected rather
//! than discarded. JSON is the complete UltraTasks interchange format.
use crate::{
    DueDate, Engine, Event, EventException, EventId, EventSchedule, Frequency, ImportMode,
    ImportReport, Recurrence, Task, TaskId, TransferResult,
    storage::{self, Entity, Snapshot},
    transfer::{ensure_input_size, invalid},
};
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct IcsExport {
    pub text: String,
    /// Explicitly reports application-only data excluded from calendar export.
    pub notices: Vec<String>,
}

impl Engine {
    /// Exports active VEVENT and non-recurring VTODO records. Calendar/list
    /// grouping, work blocks, reminders, revisions and tombstones use JSON.
    pub fn export_ics(&self) -> TransferResult<IcsExport> {
        let tx = self.connection.unchecked_transaction()?;
        storage::validate_consistency(&tx)?;
        let records = storage::all_snapshots(&tx)?;
        let mut lines = vec![
            "BEGIN:VCALENDAR".into(),
            "VERSION:2.0".into(),
            "PRODID:-//UltraTasks//Calendar 1//EN".into(),
            "CALSCALE:GREGORIAN".into(),
        ];
        let stamp = format!("DTSTAMP:{}", Utc::now().format("%Y%m%dT%H%M%SZ"));
        let mut omitted = 0usize;
        let mut grouping = false;
        for record in records {
            if record.deleted {
                omitted += 1;
                continue;
            }
            match record.entity {
                Entity::Event(event) => {
                    grouping |= event.calendar_id.is_some();
                    export_event(&event, &stamp, &mut lines)?;
                }
                Entity::Task(task) => {
                    grouping |= task.list_id.is_some();
                    if task.recurrence.is_some() || !task.exceptions.is_empty() {
                        return Err(invalid(
                            "recurring VTODO export is unsupported; use JSON to preserve due-only recurrence semantics",
                        ));
                    }
                    lines.push("BEGIN:VTODO".into());
                    lines.push(format!("UID:urn:uuid:{}", task.id));
                    lines.push(stamp.clone());
                    lines.push(format!("SUMMARY:{}", escape(&task.title)));
                    if let Some(due) = &task.due {
                        lines.push(format_due("DUE", due)?);
                    }
                    lines.push(format!(
                        "STATUS:{}",
                        if task.completed {
                            "COMPLETED"
                        } else {
                            "NEEDS-ACTION"
                        }
                    ));
                    lines.push("END:VTODO".into());
                }
                _ => omitted += 1,
            }
        }
        lines.push("END:VCALENDAR".into());
        let mut text = String::new();
        for line in lines {
            fold_line(&line, &mut text);
        }
        ensure_input_size(text.len())?;
        let mut notices = Vec::new();
        if omitted > 0 {
            notices.push(format!("ICS excludes {omitted} application-only records/tombstones; use JSON or a database backup for complete recovery."));
        }
        if grouping {
            notices.push("Calendar/list membership is not represented by this ICS subset; use JSON to preserve grouping.".into());
        }
        tx.commit()?;
        Ok(IcsExport { text, notices })
    }

    /// Imports a bounded RFC 5545 subset, with stable IDs derived from UID.
    /// Rejects unsupported properties/components before making any changes.
    pub fn import_ics(&mut self, input: &str, mode: ImportMode) -> TransferResult<ImportReport> {
        self.import_records(parse_calendar(input)?, mode)
    }
}

fn export_event(event: &Event, stamp: &str, lines: &mut Vec<String>) -> TransferResult<()> {
    lines.push("BEGIN:VEVENT".into());
    lines.push(format!("UID:urn:uuid:{}", event.id));
    lines.push(stamp.into());
    lines.push(format!("SUMMARY:{}", escape(&event.title)));
    export_schedule(&event.schedule, lines)?;
    if let Some(recurrence) = &event.recurrence {
        lines.push(format!(
            "RRULE:{}",
            format_rule(recurrence, &event.schedule)?
        ));
    }
    for (date, exception) in &event.exceptions {
        if matches!(exception, EventException::Skip) {
            lines.push(format_occurrence_date("EXDATE", *date, &event.schedule)?);
        }
    }
    lines.push("END:VEVENT".into());
    for (date, exception) in &event.exceptions {
        if let EventException::Override { title, schedule } = exception {
            lines.push("BEGIN:VEVENT".into());
            lines.push(format!("UID:urn:uuid:{}", event.id));
            lines.push(stamp.into());
            lines.push(format_occurrence_date(
                "RECURRENCE-ID",
                *date,
                &event.schedule,
            )?);
            if let Some(title) = title {
                lines.push(format!("SUMMARY:{}", escape(title)));
            }
            export_schedule(schedule, lines)?;
            lines.push("END:VEVENT".into());
        }
    }
    Ok(())
}

fn format_due(name: &str, due: &DueDate) -> TransferResult<String> {
    match due {
        DueDate::Date(date) => Ok(format!("{name};VALUE=DATE:{}", date.format("%Y%m%d"))),
        DueDate::At { at, timezone } => format_instant(name, *at, timezone),
    }
}
fn format_instant(name: &str, instant: DateTime<Utc>, zone: &str) -> TransferResult<String> {
    if instant.timestamp_subsec_nanos() != 0 {
        return Err(invalid("ICS cannot preserve fractional seconds; use JSON"));
    }
    if zone == "UTC" {
        Ok(format!("{name}:{}", instant.format("%Y%m%dT%H%M%SZ")))
    } else {
        let zone: Tz = zone
            .parse()
            .map_err(|_| invalid("invalid stored IANA timezone"))?;
        let local = instant.with_timezone(&zone);
        if zone
            .from_local_datetime(&local.naive_local())
            .earliest()
            .map(|value| value.with_timezone(&Utc))
            != Some(instant)
        {
            return Err(invalid(
                "ICS local datetime cannot preserve the second instance of an ambiguous wall time; use JSON",
            ));
        }
        Ok(format!(
            "{name};TZID={zone}:{}",
            local.format("%Y%m%dT%H%M%S")
        ))
    }
}
fn export_schedule(schedule: &EventSchedule, lines: &mut Vec<String>) -> TransferResult<()> {
    match schedule {
        EventSchedule::AllDay { start, end } => {
            lines.push(format!("DTSTART;VALUE=DATE:{}", start.format("%Y%m%d")));
            lines.push(format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")));
        }
        EventSchedule::Timed {
            start,
            end,
            timezone,
        } => {
            lines.push(format_instant("DTSTART", *start, timezone)?);
            lines.push(format_instant("DTEND", *end, timezone)?);
        }
    }
    Ok(())
}
fn format_occurrence_date(
    name: &str,
    date: NaiveDate,
    schedule: &EventSchedule,
) -> TransferResult<String> {
    match schedule {
        EventSchedule::AllDay { .. } => Ok(format!("{name};VALUE=DATE:{}", date.format("%Y%m%d"))),
        EventSchedule::Timed {
            start, timezone, ..
        } => {
            let zone: Tz = timezone
                .parse()
                .map_err(|_| invalid("invalid stored timezone"))?;
            Ok(format!(
                "{name};TZID={zone}:{}",
                date.and_time(start.with_timezone(&zone).time())
                    .format("%Y%m%dT%H%M%S")
            ))
        }
    }
}
fn format_rule(rule: &Recurrence, schedule: &EventSchedule) -> TransferResult<String> {
    if rule.count.is_some() && rule.until.is_some() {
        return Err(invalid(
            "ICS cannot represent COUNT and UNTIL together; use JSON",
        ));
    }
    let frequency = match &rule.frequency {
        Frequency::Daily => "DAILY",
        Frequency::Weekly { .. } => "WEEKLY",
        Frequency::Monthly => "MONTHLY",
    };
    let mut value = format!("FREQ={frequency};INTERVAL={}", rule.interval);
    if let Frequency::Weekly { weekdays } = &rule.frequency {
        value.push_str(";BYDAY=");
        value.push_str(
            &weekdays
                .iter()
                .map(|day| weekday_name(*day))
                .collect::<TransferResult<Vec<_>>>()?
                .join(","),
        );
    }
    if let Some(count) = rule.count {
        value.push_str(&format!(";COUNT={count}"));
    }
    if let Some(until) = rule.until {
        match schedule {
            EventSchedule::AllDay { .. } => {
                value.push_str(&format!(";UNTIL={}", until.format("%Y%m%d")))
            }
            EventSchedule::Timed { timezone, .. } => {
                let zone: Tz = timezone.parse().map_err(|_| invalid("invalid timezone"))?;
                // A local end-of-day bound includes precisely every recurrence
                // start on the domain's inclusive final date, including DST days.
                let next = until
                    .succ_opt()
                    .ok_or_else(|| invalid("UNTIL date overflow"))?;
                if next.year() > 9999 {
                    return Err(invalid(
                        "ICS UNTIL boundary exceeds supported year; use JSON",
                    ));
                }
                let midnight = next.and_hms_opt(0, 0, 0).expect("valid midnight");
                let boundary = zone
                    .from_local_datetime(&midnight)
                    .earliest()
                    .ok_or_else(|| {
                        invalid("UNTIL falls before a timezone midnight gap; use JSON")
                    })?;
                let end = boundary.with_timezone(&Utc) - Duration::seconds(1);
                value.push_str(&format!(";UNTIL={}", end.format("%Y%m%dT%H%M%SZ")));
            }
        }
    }
    Ok(value)
}
fn weekday_name(day: u8) -> TransferResult<&'static str> {
    ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
        .get(usize::from(day).wrapping_sub(1))
        .copied()
        .ok_or_else(|| invalid("invalid weekday"))
}
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}
fn unescape(value: &str) -> TransferResult<String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            out.push(match chars.next() {
                Some('n' | 'N') => '\n',
                Some(ch @ ('\\' | ';' | ',')) => ch,
                _ => return Err(invalid("invalid text escape")),
            });
        } else if ch == '\r' || ch == '\n' {
            return Err(invalid("unescaped newline"));
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}
fn fold_line(line: &str, output: &mut String) {
    let mut remaining = line;
    let mut first = true;
    loop {
        let mut boundary = remaining.len().min(if first { 75 } else { 74 });
        while !remaining.is_char_boundary(boundary) {
            boundary -= 1;
        }
        if !first {
            output.push(' ');
        }
        output.push_str(&remaining[..boundary]);
        output.push_str("\r\n");
        remaining = &remaining[boundary..];
        if remaining.is_empty() {
            break;
        }
        first = false;
    }
}

#[derive(Debug, Clone)]
struct Property {
    name: String,
    params: BTreeMap<String, String>,
    value: String,
}
type Component = Vec<Property>;

fn parse_calendar(input: &str) -> TransferResult<Vec<Snapshot>> {
    ensure_input_size(input.len())?;
    let lines = unfold(input)?;
    if lines.first().map(String::as_str) != Some("BEGIN:VCALENDAR")
        || lines.last().map(String::as_str) != Some("END:VCALENDAR")
    {
        return Err(invalid("expected one VCALENDAR"));
    }
    let mut header = Vec::new();
    let mut current: Option<(String, Component)> = None;
    let mut events = Vec::new();
    let mut tasks = Vec::new();
    for line in &lines[1..lines.len() - 1] {
        let property = parse_property(line)?;
        match property.name.as_str() {
            "BEGIN" => {
                if current.is_some()
                    || !property.params.is_empty()
                    || !matches!(property.value.as_str(), "VEVENT" | "VTODO")
                {
                    return Err(invalid(
                        "unsupported or nested iCalendar component (including VALARM/VTIMEZONE)",
                    ));
                }
                current = Some((property.value, Vec::new()));
            }
            "END" => {
                let (kind, component) = current
                    .take()
                    .ok_or_else(|| invalid("unexpected component end"))?;
                if !property.params.is_empty() || kind != property.value {
                    return Err(invalid("mismatched component end"));
                }
                if kind == "VEVENT" {
                    events.push(component);
                } else {
                    tasks.push(component);
                }
                if events.len() + tasks.len() > crate::MAX_TRANSFER_RECORDS {
                    return Err(invalid("too many calendar components"));
                }
            }
            _ => match &mut current {
                Some((_, component)) => component.push(property),
                None => header.push(property),
            },
        }
    }
    if current.is_some() {
        return Err(invalid("unterminated calendar component"));
    }
    validate_properties(&header, &["VERSION", "PRODID", "CALSCALE", "METHOD"])?;
    if required(&header, "VERSION")?.value != "2.0" {
        return Err(invalid("only iCalendar 2.0 is supported"));
    }
    required(&header, "PRODID")?;
    if optional(&header, "CALSCALE")?.is_some_and(|p| p.value != "GREGORIAN")
        || optional(&header, "METHOD")?.is_some_and(|p| p.value != "PUBLISH")
    {
        return Err(invalid("unsupported calendar scale or scheduling method"));
    }
    let mut masters = HashMap::<String, Event>::new();
    let mut overrides = Vec::new();
    let mut records = Vec::new();
    for component in events {
        validate_properties(
            &component,
            &[
                "UID",
                "SUMMARY",
                "DTSTART",
                "DTEND",
                "DURATION",
                "DTSTAMP",
                "CREATED",
                "LAST-MODIFIED",
                "SEQUENCE",
                "RRULE",
                "EXDATE",
                "RECURRENCE-ID",
                "STATUS",
            ],
        )?;
        let uid = uid(&component)?;
        if optional(&component, "RECURRENCE-ID")?.is_some() {
            overrides.push((uid, component));
            continue;
        }
        if optional(&component, "STATUS")?.is_some_and(|p| p.value != "CONFIRMED") {
            return Err(invalid(
                "cancelled/nonstandard master events are unsupported",
            ));
        }
        let schedule = parse_schedule(&component)?;
        let recurrence = optional(&component, "RRULE")?
            .map(|p| parse_rule(&p.value, &schedule))
            .transpose()?;
        let mut exceptions = BTreeMap::new();
        for property in component.iter().filter(|p| p.name == "EXDATE") {
            if recurrence.is_none() {
                return Err(invalid("EXDATE requires RRULE"));
            }
            for value in property.value.split(',') {
                let mut property = property.clone();
                property.value = value.into();
                let date = occurrence_date(&property, &schedule)?;
                if exceptions.insert(date, EventException::Skip).is_some() {
                    return Err(invalid("duplicate EXDATE"));
                }
            }
        }
        let event = Event {
            id: EventId(stable_uid("event", &uid)?),
            title: unescape(&required(&component, "SUMMARY")?.value)?,
            schedule,
            calendar_id: None,
            recurrence,
            exceptions,
            revision: 1,
        };
        if masters.insert(uid, event).is_some() {
            return Err(invalid("duplicate master event UID"));
        }
    }
    for (uid, component) in overrides {
        let event = masters
            .get_mut(&uid)
            .ok_or_else(|| invalid("recurrence override has no master in this file"))?;
        if event.recurrence.is_none()
            || optional(&component, "RRULE")?.is_some()
            || component.iter().any(|p| p.name == "EXDATE")
        {
            return Err(invalid("invalid recurrence override"));
        }
        let date = occurrence_date(required(&component, "RECURRENCE-ID")?, &event.schedule)?;
        let exception = match optional(&component, "STATUS")?.map(|p| p.value.as_str()) {
            Some("CANCELLED") => {
                if component.iter().any(|p| {
                    matches!(
                        p.name.as_str(),
                        "DTSTART" | "DTEND" | "DURATION" | "SUMMARY"
                    )
                }) {
                    return Err(invalid(
                        "cancelled override must contain only its identity and metadata",
                    ));
                }
                EventException::Skip
            }
            None | Some("CONFIRMED") => EventException::Override {
                title: optional(&component, "SUMMARY")?
                    .map(|p| unescape(&p.value))
                    .transpose()?,
                schedule: parse_schedule(&component)?,
            },
            _ => return Err(invalid("unsupported override status")),
        };
        if event.exceptions.insert(date, exception).is_some() {
            return Err(invalid("duplicate or conflicting recurrence exception"));
        }
    }
    for event in masters.into_values() {
        records.push(Snapshot {
            entity: Entity::Event(event),
            deleted: false,
        });
    }
    let mut task_uids = HashSet::new();
    for component in tasks {
        validate_properties(
            &component,
            &[
                "UID",
                "SUMMARY",
                "DUE",
                "STATUS",
                "DTSTAMP",
                "CREATED",
                "LAST-MODIFIED",
                "SEQUENCE",
                "PERCENT-COMPLETE",
            ],
        )?;
        let uid = uid(&component)?;
        if !task_uids.insert(uid.clone()) {
            return Err(invalid("duplicate VTODO UID"));
        }
        let completed = match optional(&component, "STATUS")?.map(|p| p.value.as_str()) {
            None | Some("NEEDS-ACTION") => false,
            Some("COMPLETED") => true,
            _ => return Err(invalid("unsupported VTODO status")),
        };
        if let Some(value) = optional(&component, "PERCENT-COMPLETE")?
            && value.value != if completed { "100" } else { "0" }
        {
            return Err(invalid("partial task progress is unsupported"));
        }
        let due = optional(&component, "DUE")?
            .map(parse_date_value)
            .transpose()?;
        records.push(Snapshot {
            entity: Entity::Task(Task {
                id: TaskId(stable_uid("task", &uid)?),
                title: unescape(&required(&component, "SUMMARY")?.value)?,
                completed,
                due,
                list_id: None,
                recurrence: None,
                exceptions: BTreeMap::new(),
                revision: 1,
            }),
            deleted: false,
        });
    }
    records.sort_by_key(|value| format!("{:?}", value.id()));
    Ok(records)
}

fn unfold(input: &str) -> TransferResult<Vec<String>> {
    let mut lines: Vec<String> = Vec::new();
    for raw in input.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            continue;
        }
        if line
            .chars()
            .any(|ch| ch == '\r' || (ch.is_control() && ch != '\t'))
        {
            return Err(invalid("invalid control character in calendar"));
        }
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            let previous = lines
                .last_mut()
                .ok_or_else(|| invalid("orphan folded line"))?;
            previous.push_str(rest);
            if previous.len() > 16384 {
                return Err(invalid("unfolded line exceeds 16 KiB"));
            }
        } else {
            if line.len() > 16384 {
                return Err(invalid("line exceeds 16 KiB"));
            }
            lines.push(line.into());
        }
        if lines.len() > 1_000_000 {
            return Err(invalid("too many content lines"));
        }
    }
    Ok(lines)
}
fn parse_property(line: &str) -> TransferResult<Property> {
    let (left, value) = line
        .split_once(':')
        .ok_or_else(|| invalid("content line is missing ':'"))?;
    let mut pieces = left.split(';');
    let name = pieces.next().unwrap_or_default().to_ascii_uppercase();
    if name.is_empty()
        || !name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
    {
        return Err(invalid("invalid property name"));
    }
    let mut params = BTreeMap::new();
    for piece in pieces {
        let (key, value) = piece
            .split_once('=')
            .ok_or_else(|| invalid("invalid property parameter"))?;
        let key = key.to_ascii_uppercase();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value)
            .to_owned();
        if params.insert(key, value).is_some() {
            return Err(invalid("duplicate property parameter"));
        }
    }
    Ok(Property {
        name,
        params,
        value: value.into(),
    })
}
fn validate_properties(component: &Component, allowed: &[&str]) -> TransferResult<()> {
    let mut names = HashSet::new();
    for property in component {
        if !allowed.contains(&property.name.as_str()) {
            return Err(invalid(format!("unsupported property {}", property.name)));
        }
        if property.name != "EXDATE" && !names.insert(&property.name) {
            return Err(invalid(format!("duplicate property {}", property.name)));
        }
        if !matches!(
            property.name.as_str(),
            "DTSTART" | "DTEND" | "DUE" | "EXDATE" | "RECURRENCE-ID"
        ) && !property.params.is_empty()
        {
            return Err(invalid(format!(
                "unsupported parameters on {}",
                property.name
            )));
        }
        if matches!(
            property.name.as_str(),
            "DTSTAMP" | "CREATED" | "LAST-MODIFIED" | "COMPLETED"
        ) {
            parse_utc(&property.value)?;
        }
        if property.name == "SEQUENCE" {
            property
                .value
                .parse::<u32>()
                .map_err(|_| invalid("invalid SEQUENCE"))?;
        }
    }
    Ok(())
}
fn optional<'a>(component: &'a Component, name: &str) -> TransferResult<Option<&'a Property>> {
    let mut found = component.iter().filter(|p| p.name == name);
    let first = found.next();
    if found.next().is_some() {
        return Err(invalid(format!("duplicate {name}")));
    }
    Ok(first)
}
fn required<'a>(component: &'a Component, name: &str) -> TransferResult<&'a Property> {
    optional(component, name)?.ok_or_else(|| invalid(format!("missing {name}")))
}
fn uid(component: &Component) -> TransferResult<String> {
    let uid = required(component, "UID")?.value.clone();
    if uid.is_empty() || uid.len() > 2048 {
        return Err(invalid("UID must contain 1..=2048 bytes"));
    }
    Ok(uid)
}
fn stable_uid(kind: &str, uid: &str) -> TransferResult<uuid::Uuid> {
    if let Some(uuid) = uid.strip_prefix("urn:uuid:") {
        return uuid.parse().map_err(|_| invalid("invalid urn:uuid UID"));
    }
    Ok(uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("https://ultratasks.invalid/ics/{kind}/{uid}").as_bytes(),
    ))
}
fn parse_utc(value: &str) -> TransferResult<DateTime<Utc>> {
    if value.len() != 16 || !value.ends_with('Z') {
        return Err(invalid("UTC datetime must be YYYYMMDDTHHMMSSZ"));
    }
    let local = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
        .map_err(|_| invalid("invalid UTC datetime"))?;
    Ok(Utc.from_utc_datetime(&local))
}
fn parse_date_value(property: &Property) -> TransferResult<DueDate> {
    for key in property.params.keys() {
        if !matches!(key.as_str(), "TZID" | "VALUE") {
            return Err(invalid("unsupported datetime parameter"));
        }
    }
    match property.params.get("VALUE").map(String::as_str) {
        Some("DATE") => {
            if property.params.contains_key("TZID") || property.value.len() != 8 {
                return Err(invalid("DATE cannot have TZID and must be YYYYMMDD"));
            }
            Ok(DueDate::Date(
                NaiveDate::parse_from_str(&property.value, "%Y%m%d")
                    .map_err(|_| invalid("invalid DATE"))?,
            ))
        }
        None | Some("DATE-TIME") => {
            if property.value.ends_with('Z') {
                if property.params.contains_key("TZID") {
                    return Err(invalid("UTC datetime cannot have TZID"));
                }
                return Ok(DueDate::At {
                    at: parse_utc(&property.value)?,
                    timezone: "UTC".into(),
                });
            }
            let zone = property.params.get("TZID").ok_or_else(|| {
                invalid("floating datetimes are unsupported; specify an IANA TZID or UTC")
            })?;
            let tz: Tz = zone.parse().map_err(|_| {
                invalid("TZID must be an installed IANA timezone (custom VTIMEZONE is unsupported)")
            })?;
            if property.value.len() != 15 {
                return Err(invalid("local datetime must be YYYYMMDDTHHMMSS"));
            }
            let local = NaiveDateTime::parse_from_str(&property.value, "%Y%m%dT%H%M%S")
                .map_err(|_| invalid("invalid local datetime"))?;
            let instant = match tz.from_local_datetime(&local) {
                LocalResult::Single(value) => value,
                LocalResult::Ambiguous(first, second) => first.min(second),
                LocalResult::None => {
                    return Err(invalid("explicit datetime falls in a timezone gap"));
                }
            };
            Ok(DueDate::At {
                at: instant.with_timezone(&Utc),
                timezone: zone.clone(),
            })
        }
        _ => Err(invalid("unsupported VALUE type")),
    }
}
fn parse_schedule(component: &Component) -> TransferResult<EventSchedule> {
    let start = parse_date_value(required(component, "DTSTART")?)?;
    let end = optional(component, "DTEND")?;
    let duration = optional(component, "DURATION")?;
    if end.is_some() && duration.is_some() {
        return Err(invalid("DTEND and DURATION are mutually exclusive"));
    }
    let end = match (end, duration) {
        (Some(value), _) => parse_date_value(value)?,
        (_, Some(value)) => {
            let days = value
                .value
                .strip_prefix('P')
                .and_then(|s| s.strip_suffix('D'))
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|n| *n > 0 && *n <= 36600)
                .ok_or_else(|| {
                    invalid("only positive whole-day DURATION on DATE events is supported")
                })?;
            match &start {
                DueDate::Date(date) => DueDate::Date(
                    date.checked_add_signed(Duration::days(days))
                        .ok_or_else(|| invalid("date duration overflow"))?,
                ),
                _ => return Err(invalid("timed DURATION is unsupported; use explicit DTEND")),
            }
        }
        _ => match &start {
            DueDate::Date(date) => {
                DueDate::Date(date.succ_opt().ok_or_else(|| invalid("date overflow"))?)
            }
            _ => return Err(invalid("timed events require DTEND")),
        },
    };
    match (start, end) {
        (DueDate::Date(start), DueDate::Date(end)) => Ok(EventSchedule::AllDay { start, end }),
        (
            DueDate::At {
                at: start,
                timezone,
            },
            DueDate::At { at: end, .. },
        ) => Ok(EventSchedule::Timed {
            start,
            end,
            timezone,
        }),
        _ => Err(invalid("DTSTART and DTEND must have the same VALUE type")),
    }
}
fn occurrence_date(property: &Property, schedule: &EventSchedule) -> TransferResult<NaiveDate> {
    match (parse_date_value(property)?, schedule) {
        (DueDate::Date(date), EventSchedule::AllDay { .. }) => Ok(date),
        (DueDate::At { at, .. }, EventSchedule::Timed { timezone, .. }) => {
            let zone: Tz = timezone
                .parse()
                .map_err(|_| invalid("invalid master timezone"))?;
            let date = at.with_timezone(&zone).date_naive();
            let occurrence = crate::recurrence::event_at(schedule, date);
            if !matches!(occurrence, Some(EventSchedule::Timed { start, .. }) if start == at) {
                return Err(invalid(
                    "recurrence identifier must identify the exact generated start instant",
                ));
            }
            Ok(date)
        }
        _ => Err(invalid("recurrence identifier type must match DTSTART")),
    }
}
fn parse_rule(value: &str, schedule: &EventSchedule) -> TransferResult<Recurrence> {
    let mut fields = HashMap::new();
    for field in value.split(';') {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| invalid("invalid RRULE field"))?;
        if !matches!(
            key,
            "FREQ" | "INTERVAL" | "COUNT" | "UNTIL" | "BYDAY" | "WKST"
        ) || fields.insert(key, value).is_some()
        {
            return Err(invalid(format!(
                "unsupported or duplicate RRULE field {key}"
            )));
        }
    }
    if fields.get("WKST").is_some_and(|value| *value != "MO") {
        return Err(invalid("only Monday WKST is supported"));
    }
    let anchor = match schedule {
        EventSchedule::AllDay { start, .. } => *start,
        EventSchedule::Timed {
            start, timezone, ..
        } => start
            .with_timezone(
                &timezone
                    .parse::<Tz>()
                    .map_err(|_| invalid("invalid timezone"))?,
            )
            .date_naive(),
    };
    let frequency = match fields.get("FREQ").copied() {
        Some("DAILY") => Frequency::Daily,
        Some("MONTHLY") => Frequency::Monthly,
        Some("WEEKLY") => {
            let weekdays = match fields.get("BYDAY") {
                Some(days) => days
                    .split(',')
                    .map(|day| {
                        ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
                            .iter()
                            .position(|v| *v == day)
                            .map(|index| index as u8 + 1)
                            .ok_or_else(|| invalid("unsupported BYDAY value"))
                    })
                    .collect::<TransferResult<Vec<_>>>()?,
                None => vec![anchor.weekday().number_from_monday() as u8],
            };
            if !weekdays.contains(&(anchor.weekday().number_from_monday() as u8)) {
                return Err(invalid("DTSTART must match weekly BYDAY"));
            }
            Frequency::Weekly { weekdays }
        }
        _ => {
            return Err(invalid(
                "only DAILY, WEEKLY and MONTHLY RRULE are supported",
            ));
        }
    };
    if fields.contains_key("BYDAY") && !matches!(frequency, Frequency::Weekly { .. }) {
        return Err(invalid("BYDAY is supported only for weekly recurrence"));
    }
    if fields.contains_key("COUNT") && fields.contains_key("UNTIL") {
        return Err(invalid("RRULE COUNT and UNTIL are mutually exclusive"));
    }
    let interval = fields
        .get("INTERVAL")
        .map(|value| {
            value
                .parse::<u16>()
                .map_err(|_| invalid("invalid INTERVAL"))
        })
        .transpose()?
        .unwrap_or(1);
    let count = fields
        .get("COUNT")
        .map(|value| value.parse::<u32>().map_err(|_| invalid("invalid COUNT")))
        .transpose()?;
    let until = fields
        .get("UNTIL")
        .map(|value| -> TransferResult<NaiveDate> {
            match schedule {
                EventSchedule::AllDay { .. } => {
                    if value.len() != 8 {
                        return Err(invalid("DATE recurrence requires DATE UNTIL"));
                    }
                    NaiveDate::parse_from_str(value, "%Y%m%d")
                        .map_err(|_| invalid("invalid UNTIL date"))
                }
                EventSchedule::Timed {
                    start, timezone, ..
                } => {
                    let bound = parse_utc(value)?;
                    let zone: Tz = timezone.parse().map_err(|_| invalid("invalid timezone"))?;
                    let date = bound.with_timezone(&zone).date_naive();
                    let candidate = zone
                        .from_local_datetime(&date.and_time(start.with_timezone(&zone).time()))
                        .earliest();
                    if candidate.is_some_and(|candidate| candidate.with_timezone(&Utc) > bound) {
                        date.pred_opt().ok_or_else(|| invalid("UNTIL overflow"))
                    } else {
                        Ok(date)
                    }
                }
            }
        })
        .transpose()?;
    Ok(Recurrence {
        frequency,
        interval,
        count,
        until,
    })
}
