//! Repeatable bounded recurrence workload, including a large exception set.
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::json;
use std::{collections::BTreeMap, error::Error, time::Instant};
use ultratasks_core::*;
fn stats(samples: &[u128]) -> serde_json::Value {
    let mut values = samples.to_vec();
    values.sort_unstable();
    let p = |percent: usize| values[(values.len() * percent).div_ceil(100).saturating_sub(1)];
    json!({"samples":values.len(),"unit":"microseconds","min":values[0],"p50":p(50),"p95":p(95),"p99":p(99),"max":values[values.len()-1]})
}
fn main() -> std::result::Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("recurrence.sqlite3");
    let mut engine = Engine::open(&path)?;
    let old: NaiveDate = "2020-01-01".parse()?;
    let anchor: NaiveDate = "2000-01-01".parse()?;
    let start: DateTime<Utc> = "2026-11-09T00:00:00Z".parse()?;
    let query = CalendarQuery {
        start,
        end: start + Duration::days(1),
        timezone: "Asia/Tokyo".into(),
        limit: 1000,
    };
    let mut records = Vec::new();
    for index in 0..10000 {
        records.push(RecordSnapshot {
            entity: Entity::Event(Event {
                id: EventId::new(),
                title: format!("過去の予定 {index:05}"),
                schedule: EventSchedule::AllDay {
                    start: old,
                    end: old.succ_opt().unwrap(),
                },
                calendar_id: None,
                recurrence: None,
                exceptions: BTreeMap::new(),
                revision: 1,
            }),
            deleted: false,
        });
    }
    for _ in 0..100 {
        records.push(RecordSnapshot {
            entity: Entity::Event(Event {
                id: EventId::new(),
                title: "長い日本語タイトルの繰り返し予定".repeat(20),
                schedule: EventSchedule::Timed {
                    start: anchor.and_hms_opt(3, 0, 0).unwrap().and_utc(),
                    end: anchor.and_hms_opt(4, 0, 0).unwrap().and_utc(),
                    timezone: "Asia/Tokyo".into(),
                },
                calendar_id: None,
                recurrence: Some(Recurrence {
                    frequency: Frequency::Daily,
                    interval: 1,
                    count: None,
                    until: None,
                }),
                exceptions: BTreeMap::new(),
                revision: 1,
            }),
            deleted: false,
        });
    }
    let heavy = Event {
        id: EventId::new(),
        title: "1000例外を持つ有限系列".into(),
        schedule: EventSchedule::Timed {
            start: anchor.and_hms_opt(5, 0, 0).unwrap().and_utc(),
            end: anchor.and_hms_opt(6, 0, 0).unwrap().and_utc(),
            timezone: "America/New_York".into(),
        },
        calendar_id: None,
        recurrence: Some(Recurrence {
            frequency: Frequency::Daily,
            interval: 1,
            count: Some(10000),
            until: None,
        }),
        exceptions: (0..1000)
            .map(|index| (anchor + Duration::days(index * 9), EventException::Skip))
            .collect(),
        revision: 1,
    };
    records.push(RecordSnapshot {
        entity: Entity::Event(heavy.clone()),
        deleted: false,
    });
    let input =
        serde_json::to_string(&json!({"format":"ultratasks","version":1,"records":records}))?;
    let begin = Instant::now();
    engine.import_json(&input, ImportMode::Empty)?;
    let import_ms = begin.elapsed().as_secs_f64() * 1000.;
    assert_eq!(engine.event_occurrences(&query, None)?.len(), 101);
    assert_eq!(expand_event(&heavy, &query)?.len(), 1);
    let mut calendar = Vec::new();
    let mut exception = Vec::new();
    for _ in 0..100 {
        let begin = Instant::now();
        assert_eq!(engine.event_occurrences(&query, None)?.len(), 101);
        calendar.push(begin.elapsed().as_micros());
        let begin = Instant::now();
        assert_eq!(expand_event(&heavy, &query)?.len(), 1);
        exception.push(begin.elapsed().as_micros());
    }
    let db = rusqlite::Connection::open(&path)?;
    let sqlite: String = db.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|line| line.split_once(':'))
                    .map(|(_, value)| value.trim().to_owned())
            })
        });
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope":"temporary on-disk SQLite, 100 warm-cache samples; recurrence includes validation, exception membership and bounded expansion; excludes GUI, notification OS delivery and network",
            "environment":{"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"cpu_model":cpu,"logical_cpu_count":std::thread::available_parallelism().map(usize::from).ok(),"rustc":rustc,"debug_assertions":cfg!(debug_assertions),"sqlite_version":sqlite,"sqlite_journal_mode":"wal","storage_directory":directory.path().parent().map(|p|p.display().to_string())},
            "dataset":{"past_single_events":10000,"unbounded_daily_series":100,"counted_daily_series":1,"counted_series_count":10000,"counted_series_exceptions":1000,"expected_window_occurrences":101,"query_days":1,"sample_count":100,"logical_json_bytes":input.len()},
            "import_total_ms":import_ms,"query_calendar":stats(&calendar),"expand_one_exception_heavy_series":stats(&exception),
            "caveat":"This host and storage only; no target-device or GUI performance guarantee."
        }))?
    );
    Ok(())
}
