use crate::model::*;
use jiff::tz::{AmbiguousOffset, TimeZone, TimeZoneDatabase};
use std::sync::OnceLock;

pub(crate) const DAY_US: i64 = 86_400_000_000;
const MIN_DAY: i32 = -719_162;
const MAX_DAY: i32 = 2_932_896;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Date(pub(crate) i32);
impl Date {
    pub fn parse(value: &str) -> Result<Self> {
        let bad = || invalid(Field::Date, InputProblem::Malformed);
        if value.len() != 10
            || value.as_bytes()[4] != b'-'
            || value.as_bytes()[7] != b'-'
            || !value
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
        {
            return Err(bad());
        }
        let y: i32 = value[..4].parse().map_err(|_| bad())?;
        let m: i32 = value[5..7].parse().map_err(|_| bad())?;
        let d: i32 = value[8..].parse().map_err(|_| bad())?;
        if y == 0 {
            return Err(invalid(Field::Date, InputProblem::OutOfRange));
        }
        jiff::civil::Date::new(y as i16, m as i8, d as i8).map_err(|_| bad())?;
        let y = y - i32::from(m <= 2);
        let era = y / 400;
        let yoe = y - era * 400;
        let mp = m + if m > 2 { -3 } else { 9 };
        let doy = (153 * mp + 2) / 5 + d - 1;
        Ok(Self(
            era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468,
        ))
    }
    pub fn ymd(self) -> (i32, u8, u8) {
        let z = self.0 + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = mp + if mp < 10 { 3 } else { -9 };
        (y + i32::from(m <= 2), m as u8, d as u8)
    }
    pub fn add_days(self, days: i32) -> Result<Self> {
        let value = self
            .0
            .checked_add(days)
            .filter(|d| (MIN_DAY..=MAX_DAY).contains(d))
            .ok_or_else(|| invalid(Field::Date, InputProblem::OutOfRange))?;
        Ok(Self(value))
    }
    pub fn iso(self) -> String {
        let (y, m, d) = self.ymd();
        format!("{y:04}-{m:02}-{d:02}")
    }
    pub fn today(zone: &ZoneId) -> Result<Self> {
        let now = Instant::from_unix_micros(jiff::Timestamp::now().as_microsecond())?;
        Ok(local_at(now, zone)?.datetime.date)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Instant(i64);
impl Instant {
    pub fn parse(value: &str) -> Result<Self> {
        let bad = || invalid(Field::Instant, InputProblem::Malformed);
        if !value.is_ascii() {
            return Err(bad());
        }
        let (local, offset) =
            if let Some(local) = value.strip_suffix('Z').or_else(|| value.strip_suffix('z')) {
                (local, 0i64)
            } else {
                if value.len() < 22 {
                    return Err(bad());
                }
                let i = value.len() - 6;
                let suffix = &value[i..];
                let bytes = suffix.as_bytes();
                if !matches!(bytes[0], b'+' | b'-')
                    || bytes[3] != b':'
                    || !bytes[1..3]
                        .iter()
                        .chain(&bytes[4..])
                        .all(u8::is_ascii_digit)
                {
                    return Err(bad());
                }
                let h: i64 = suffix[1..3].parse().map_err(|_| bad())?;
                let m: i64 = suffix[4..].parse().map_err(|_| bad())?;
                if h > 23 || m > 59 {
                    return Err(bad());
                }
                (
                    &value[..i],
                    (h * 60 + m) * 60 * if bytes[0] == b'-' { -1 } else { 1 },
                )
            };
        if local.len() < 19 {
            return Err(bad());
        }
        let dt = LocalDateTime::parse(local).map_err(|e| match e {
            Error::InvalidInput { problem, .. } => invalid(Field::Instant, problem),
            other => other,
        })?;
        Self::from_unix_micros(dt.naive_micros() - offset * 1_000_000)
    }
    pub fn from_unix_micros(value: i64) -> Result<Self> {
        if !(i64::from(MIN_DAY) * DAY_US..(i64::from(MAX_DAY) + 1) * DAY_US).contains(&value) {
            return Err(invalid(Field::Instant, InputProblem::OutOfRange));
        }
        Ok(Self(value))
    }
    pub fn unix_micros(self) -> i64 {
        self.0
    }
    pub fn rfc3339(self) -> String {
        let local = LocalDateTime {
            date: Date(self.0.div_euclid(DAY_US) as i32),
            micros_of_day: self.0.rem_euclid(DAY_US) as u64,
        };
        format!("{}Z", local.iso())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZoneId(String);
fn tzdb() -> &'static TimeZoneDatabase {
    static DB: OnceLock<TimeZoneDatabase> = OnceLock::new();
    DB.get_or_init(TimeZoneDatabase::bundled)
}
impl ZoneId {
    pub fn parse(value: &str) -> Result<Self> {
        if value.is_empty() {
            return Err(invalid(Field::Zone, InputProblem::Empty));
        }
        if !tzdb().is_definitively_empty() {
            tzdb()
                .get(value)
                .map_err(|_| invalid(Field::Zone, InputProblem::UnknownZone))?;
            Ok(Self(value.to_owned()))
        } else {
            Err(Error::TimeDataUnavailable)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn timezone(&self) -> Result<TimeZone> {
        tzdb().get(&self.0).map_err(|_| Error::TimeDataUnavailable)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalDateTime {
    date: Date,
    micros_of_day: u64,
}
impl LocalDateTime {
    pub fn parse(value: &str) -> Result<Self> {
        let bad = || invalid(Field::LocalTime, InputProblem::Malformed);
        if !value.is_ascii() || value.len() < 16 || !matches!(value.as_bytes()[10], b'T' | b't') {
            return Err(bad());
        }
        let date = Date::parse(&value[..10])?;
        let clock = &value[11..];
        let (base, fraction) = clock
            .split_once('.')
            .map_or((clock, None), |(a, b)| (a, Some(b)));
        if !matches!(base.len(), 5 | 8)
            || base.as_bytes()[2] != b':'
            || (base.len() == 8 && base.as_bytes()[5] != b':')
            || !base
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 2 || i == 5 || b.is_ascii_digit())
        {
            return Err(bad());
        }
        let h: u64 = base[..2].parse().map_err(|_| bad())?;
        let m: u64 = base[3..5].parse().map_err(|_| bad())?;
        let s: u64 = if base.len() == 8 {
            base[6..8].parse().map_err(|_| bad())?
        } else {
            0
        };
        if h > 23 || m > 59 || s > 59 {
            return Err(invalid(Field::LocalTime, InputProblem::OutOfRange));
        }
        let us = if let Some(fraction) = fraction {
            if base.len() != 8
                || fraction.is_empty()
                || !fraction.bytes().all(|c| c.is_ascii_digit())
            {
                return Err(bad());
            }
            if fraction.len() > 6 {
                return Err(invalid(
                    Field::LocalTime,
                    InputProblem::UnsupportedPrecision,
                ));
            }
            fraction.parse::<u64>().map_err(|_| bad())? * 10u64.pow(6 - fraction.len() as u32)
        } else {
            0
        };
        Ok(Self {
            date,
            micros_of_day: ((h * 60 + m) * 60 + s) * 1_000_000 + us,
        })
    }
    pub fn date(self) -> Date {
        self.date
    }
    pub fn clock(self) -> (u8, u8, u8, u32) {
        let seconds = self.micros_of_day / 1_000_000;
        (
            (seconds / 3600) as u8,
            (seconds / 60 % 60) as u8,
            (seconds % 60) as u8,
            (self.micros_of_day % 1_000_000) as u32,
        )
    }
    pub fn iso(self) -> String {
        let (h, m, s, us) = self.clock();
        let base = format!("{}T{h:02}:{m:02}:{s:02}", self.date.iso());
        if us == 0 {
            base
        } else {
            format!("{base}.{us:06}").trim_end_matches('0').to_owned()
        }
    }
    fn naive_micros(self) -> i64 {
        i64::from(self.date.0) * DAY_US + self.micros_of_day as i64
    }
    fn civil(self) -> Result<jiff::civil::DateTime> {
        self.iso()
            .parse()
            .map_err(|_| invalid(Field::LocalTime, InputProblem::OutOfRange))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlapChoice {
    Reject,
    Earlier,
    Later,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZonedInstant {
    pub instant: Instant,
    pub zone: ZoneId,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalView {
    pub datetime: LocalDateTime,
    pub offset_seconds: i32,
}

fn with_offset(local: LocalDateTime, offset: jiff::tz::Offset) -> Result<Instant> {
    Instant::from_unix_micros(local.naive_micros() - i64::from(offset.seconds()) * 1_000_000)
}
pub fn resolve_local(
    local: LocalDateTime,
    zone: &ZoneId,
    choice: OverlapChoice,
) -> Result<ZonedInstant> {
    let offset = zone
        .timezone()?
        .to_ambiguous_timestamp(local.civil()?)
        .offset();
    let instant = match offset {
        AmbiguousOffset::Unambiguous { offset } => with_offset(local, offset)?,
        AmbiguousOffset::Gap { .. } => {
            return Err(Error::NonexistentLocalTime {
                local,
                zone: zone.clone(),
            });
        }
        AmbiguousOffset::Fold { before, after } => {
            let earlier = with_offset(local, before)?;
            let later = with_offset(local, after)?;
            match choice {
                OverlapChoice::Earlier => earlier,
                OverlapChoice::Later => later,
                OverlapChoice::Reject => {
                    return Err(Error::AmbiguousLocalTime {
                        local,
                        zone: zone.clone(),
                        earlier,
                        later,
                    });
                }
            }
        }
    };
    Ok(ZonedInstant {
        instant,
        zone: zone.clone(),
    })
}
pub fn local_at(instant: Instant, zone: &ZoneId) -> Result<LocalView> {
    // JiffのTimestamp上限だけはUTCの9999年末より狭い。同梱tzdbの将来規則は400年周期。
    let probe = if instant.0 > jiff::Timestamp::MAX.as_microsecond() {
        instant.0 - 146097 * DAY_US
    } else {
        instant.0
    };
    let timestamp =
        jiff::Timestamp::from_microsecond(probe).map_err(|_| Error::TimeDataUnavailable)?;
    let offset_seconds = zone.timezone()?.to_offset(timestamp).seconds();
    let local = instant.0 + i64::from(offset_seconds) * 1_000_000;
    let day = local.div_euclid(DAY_US);
    if !(i64::from(MIN_DAY)..=i64::from(MAX_DAY)).contains(&day) {
        return Err(invalid(Field::LocalTime, InputProblem::OutOfRange));
    }
    Ok(LocalView {
        datetime: LocalDateTime {
            date: Date(day as i32),
            micros_of_day: local.rem_euclid(DAY_US) as u64,
        },
        offset_seconds,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DateSpan {
    start: Date,
    end: Date,
}
impl DateSpan {
    pub fn new(start_inclusive: Date, end_exclusive: Date) -> Result<Self> {
        if start_inclusive >= end_exclusive {
            return Err(invalid(Field::Range, InputProblem::NonIncreasingRange));
        }
        Ok(Self {
            start: start_inclusive,
            end: end_exclusive,
        })
    }
    pub fn bounds(self) -> (Date, Date) {
        (self.start, self.end)
    }
    pub(crate) fn query_days(self) -> Result<()> {
        let days = self.end.0 - self.start.0;
        if days > i32::from(MAX_QUERY_DAYS) {
            return Err(Error::LimitExceeded {
                kind: LimitKind::QueryDays,
                maximum: MAX_QUERY_DAYS.into(),
                observed_at_least: days as u64,
            });
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZonedSpan {
    start: Instant,
    end: Instant,
    zone: ZoneId,
}
impl ZonedSpan {
    pub fn new(start: Instant, end_exclusive: Instant, zone: ZoneId) -> Result<Self> {
        if start >= end_exclusive {
            return Err(invalid(Field::Range, InputProblem::NonIncreasingRange));
        }
        Ok(Self {
            start,
            end: end_exclusive,
            zone,
        })
    }
    pub fn bounds(&self) -> (Instant, Instant) {
        (self.start, self.end)
    }
    pub fn zone(&self) -> &ZoneId {
        &self.zone
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarWindow {
    days: DateSpan,
    zone: ZoneId,
    start: Instant,
    end: Instant,
}
fn day_boundary(day: Date, zone: &ZoneId) -> Result<Instant> {
    let local = LocalDateTime {
        date: day,
        micros_of_day: 0,
    };
    let offset = zone
        .timezone()?
        .to_ambiguous_timestamp(local.civil()?)
        .offset();
    match offset {
        AmbiguousOffset::Unambiguous { offset } => with_offset(local, offset),
        AmbiguousOffset::Fold { before, .. } => with_offset(local, before),
        AmbiguousOffset::Gap { before, after } => {
            let mut low = with_offset(local, after)?.0;
            let mut high = with_offset(local, before)?.0;
            while low < high {
                let mid = low + (high - low) / 2;
                if local_at(Instant(mid), zone)?.datetime.naive_micros() < local.naive_micros() {
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            Instant::from_unix_micros(low)
        }
    }
}
impl CalendarWindow {
    pub fn new(days: DateSpan, zone: ZoneId) -> Result<Self> {
        days.query_days()?;
        let start = day_boundary(days.start, &zone)?;
        let end = day_boundary(days.end, &zone)?;
        if start > end {
            return Err(invalid(Field::Range, InputProblem::NonIncreasingRange));
        }
        Ok(Self {
            days,
            zone,
            start,
            end,
        })
    }
    pub fn timed(&self) -> TimedWindow {
        TimedWindow {
            calendar: self.clone(),
            start: self.start,
            end: self.end,
        }
    }
    pub fn clip(&self, start: Instant, end_exclusive: Instant) -> Result<TimedWindow> {
        if start >= end_exclusive {
            return Err(invalid(Field::Range, InputProblem::NonIncreasingRange));
        }
        let start = start.max(self.start);
        let end = end_exclusive.min(self.end);
        if start > end || (start == end && self.start != self.end) {
            return Err(invalid(Field::Range, InputProblem::OutsideWindow));
        }
        Ok(TimedWindow {
            calendar: self.clone(),
            start,
            end,
        })
    }
    pub fn days(&self) -> DateSpan {
        self.days
    }
    pub fn zone(&self) -> &ZoneId {
        &self.zone
    }
    pub fn instant_bounds(&self) -> (Instant, Instant) {
        (self.start, self.end)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimedWindow {
    calendar: CalendarWindow,
    start: Instant,
    end: Instant,
}
impl TimedWindow {
    pub fn calendar(&self) -> &CalendarWindow {
        &self.calendar
    }
    pub fn bounds(&self) -> (Instant, Instant) {
        (self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window(day: &str, zone: &str) -> CalendarWindow {
        let day = Date::parse(day).unwrap();
        CalendarWindow::new(
            DateSpan::new(day, day.add_days(1).unwrap()).unwrap(),
            ZoneId::parse(zone).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn dst_and_missing_day_are_distinct_from_input() {
        let ny = ZoneId::parse("America/New_York").unwrap();
        let gap = LocalDateTime::parse("2026-03-08T02:30").unwrap();
        assert!(matches!(
            resolve_local(gap, &ny, OverlapChoice::Later),
            Err(Error::NonexistentLocalTime { .. })
        ));
        let fold = LocalDateTime::parse("2026-11-01T01:30").unwrap();
        assert!(matches!(
            resolve_local(fold, &ny, OverlapChoice::Reject),
            Err(Error::AmbiguousLocalTime { .. })
        ));
        let a = resolve_local(fold, &ny, OverlapChoice::Earlier).unwrap();
        let b = resolve_local(fold, &ny, OverlapChoice::Later).unwrap();
        assert_eq!(b.instant.0 - a.instant.0, 3_600_000_000);
        for (date, hours) in [("2026-03-08", 23), ("2026-11-01", 25)] {
            let w = window(date, "America/New_York");
            assert_eq!(w.end.0 - w.start.0, hours * 3_600_000_000);
        }
        let apia = window("2011-12-30", "Pacific/Apia");
        assert_eq!(apia.start, apia.end);
        assert!(
            resolve_local(
                LocalDateTime::parse("2011-12-30T12:00").unwrap(),
                apia.zone(),
                OverlapChoice::Earlier
            )
            .is_err()
        );
        let tokyo = window("2026-11-11", "Asia/Tokyo");
        assert_eq!(tokyo.start.rfc3339(), "2026-11-10T15:00:00Z");
    }
    #[test]
    fn calendar_bounds_and_precision_do_not_round() {
        assert_eq!(
            Date::parse("2024-02-28")
                .unwrap()
                .add_days(2)
                .unwrap()
                .iso(),
            "2024-03-01"
        );
        assert!(Date::parse("0000-01-01").is_err());
        assert!(Date::parse("9999-12-31").unwrap().add_days(1).is_err());
        for s in [
            "0001-01-01T00:00:00Z",
            "9999-12-31T23:59:59.999999Z",
            "1969-12-31T23:59:59.999999Z",
        ] {
            assert_eq!(Instant::parse(s).unwrap().rfc3339(), s);
        }
        assert!(Instant::parse("2026-01-01T00:00:00.0000001Z").is_err());
        assert!(Instant::parse("2016-12-31T23:59:60Z").is_err());
    }
    #[test]
    fn upper_year_zone_roundtrip_and_strict_clock_syntax() {
        for name in ["UTC", "Asia/Tokyo", "America/New_York"] {
            let zone = ZoneId::parse(name).unwrap();
            let local = LocalDateTime::parse("9999-12-31T12:00:00.123456").unwrap();
            let resolved = resolve_local(local, &zone, OverlapChoice::Reject).unwrap();
            assert_eq!(local_at(resolved.instant, &zone).unwrap().datetime, local);
        }
        for value in [
            "2026-01-01T+1:00",
            "2026-01-01T00:+1",
            "2026-01-01T00:00:+1",
        ] {
            assert!(LocalDateTime::parse(value).is_err());
        }
        assert!(Instant::parse("2026-01-01T00:00:00++1:00").is_err());
        assert!(Instant::parse("2026-01-01T00:00:00+00:+1").is_err());
    }
}
