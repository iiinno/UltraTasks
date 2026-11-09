//! Presentation-only parsing; model mutations stay in ultratasks-core.
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

pub fn schedule_range(
    date: NaiveDate,
    time: &str,
    minutes: &str,
    timezone: Tz,
) -> Result<(DateTime<Utc>, DateTime<Utc>), String> {
    let time = NaiveTime::parse_from_str(time.trim(), "%H:%M")
        .map_err(|_| "開始時刻を HH:MM 形式で入力してください")?;
    let minutes: i64 = minutes
        .trim()
        .parse()
        .map_err(|_| "所要時間は分数で入力してください")?;
    if !(1..=1_440).contains(&minutes) {
        return Err("所要時間は1〜1440分で入力してください".into());
    }
    let start = timezone.from_local_datetime(&date.and_time(time)).single()
        .ok_or("夏時間の切り替えで存在しない、または二通りに解釈される時刻です。別の時刻を指定してください")?
        .with_timezone(&Utc);
    let end = start
        .checked_add_signed(Duration::minutes(minutes))
        .ok_or("時刻の範囲外です")?;
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduling_rejects_nonexistent_and_ambiguous_wall_times() {
        let tz = chrono_tz::America::New_York;
        assert!(
            schedule_range(
                NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(),
                "02:30",
                "30",
                tz
            )
            .is_err()
        );
        assert!(
            schedule_range(
                NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
                "01:30",
                "30",
                tz
            )
            .is_err()
        );
    }

    #[test]
    fn duration_is_elapsed_time_and_can_cross_midnight() {
        let date = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        let (start, end) = schedule_range(date, "23:30", "60", chrono_tz::Asia::Tokyo).unwrap();
        assert_eq!((end - start).num_minutes(), 60);
        assert_eq!(
            end.with_timezone(&chrono_tz::Asia::Tokyo).date_naive(),
            date.succ_opt().unwrap()
        );
        assert!(schedule_range(date, "25:30", "60", chrono_tz::UTC).is_err());
        assert!(schedule_range(date, "09:00", "0", chrono_tz::UTC).is_err());
    }
}
