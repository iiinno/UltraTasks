//! An on-disk workload measurement, not a claim about all machines or the UI.
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use std::{error::Error, time::Instant};
use ultratasks_core::*;

fn statistics(samples: &[u128]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let percentile =
        |percent: usize| sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)];
    json!({
        "samples": sorted.len(), "unit": "microseconds",
        "min": sorted[0], "p50": percentile(50), "p95": percentile(95), "p99": percentile(99),
        "max": sorted[sorted.len()-1]
    })
}

fn main() -> std::result::Result<(), Box<dyn Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1000".into())
        .parse()?;
    if !(100..=99_900).contains(&count) {
        return Err("fixture count must be between 100 and 99900 (plus 100 time blocks)".into());
    }
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("measurement.sqlite3");
    let mut engine = Engine::open(&path)?;
    let start: DateTime<Utc> = "2026-11-09T00:00:00Z".parse()?;
    let mut ids = Vec::with_capacity(count);
    let begin = Instant::now();
    for index in 0..count {
        let Outcome::Task(task) = engine.execute(Command::CreateTask {
            title: format!("仕様確認 {index:06}"),
            due: None,
            list_id: None,
        })?
        else {
            return Err("unexpected create result".into());
        };
        ids.push(task.id);
    }
    let insert_total_ms = begin.elapsed().as_secs_f64() * 1000.0;
    for (index, task_id) in ids.iter().take(100).enumerate() {
        let task = engine.task(*task_id)?;
        let offset = Duration::minutes(index as i64 * 15);
        engine.execute(Command::ScheduleTask {
            task_id: *task_id,
            expected_task_revision: task.revision,
            start: start + offset,
            end: start + offset + Duration::minutes(30),
            timezone: "Asia/Tokyo".into(),
        })?;
    }
    let query = TaskQuery {
        search: Some("仕様確認".into()),
        limit: 50,
        ..TaskQuery::default()
    };
    let tail_query = TaskQuery {
        search: Some(format!("{:06}", count - 1)),
        limit: 50,
        ..TaskQuery::default()
    };
    let calendar_query = CalendarQuery {
        start,
        end: start + Duration::days(2),
        timezone: "Asia/Tokyo".into(),
        limit: 100,
    };
    // Warm caches before measuring the steady-state query/update workload.
    engine.tasks(&query)?;
    engine.tasks(&tail_query)?;
    engine.time_blocks(&calendar_query)?;
    let mut search_us = Vec::with_capacity(100);
    let mut tail_search_us = Vec::with_capacity(100);
    let mut schedule_us = Vec::with_capacity(100);
    let mut update_us = Vec::with_capacity(100);
    for task_id in ids.iter().take(100) {
        let begin = Instant::now();
        let found = engine.tasks(&query)?;
        search_us.push(begin.elapsed().as_micros());
        assert_eq!(found.len(), 50);
        let begin = Instant::now();
        let found = engine.tasks(&tail_query)?;
        tail_search_us.push(begin.elapsed().as_micros());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, ids[count - 1]);
        let begin = Instant::now();
        let found = engine.time_blocks(&calendar_query)?;
        schedule_us.push(begin.elapsed().as_micros());
        assert_eq!(found.len(), 100);
        let task = engine.task(*task_id)?;
        let begin = Instant::now();
        engine.execute(Command::SetTaskCompleted {
            id: *task_id,
            expected_revision: task.revision,
            completed: true,
        })?;
        update_us.push(begin.elapsed().as_micros());
    }
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let diagnostic_connection = rusqlite::Connection::open(&path)?;
    let sqlite_version: String =
        diagnostic_connection.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    let journal_mode: String =
        diagnostic_connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            info.lines().find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|line| line.split_once(':'))
                    .map(|(_, value)| value.trim().to_owned())
            })
        });
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope": "temporary on-disk SQLite, warm-cache sequential workload, includes transaction/history/sync-outbox/capacity persistence; excludes GUI/rendering and network",
            "environment": {
                "os": std::env::consts::OS, "architecture": std::env::consts::ARCH,
                "logical_cpu_count": std::thread::available_parallelism().map(usize::from).ok(),
                "cpu_model": cpu_model,
                "rustc": rustc, "debug_assertions": cfg!(debug_assertions),
                "sqlite_version": sqlite_version, "sqlite_journal_mode": journal_mode,
                "storage_directory": directory.path().parent().map(|path| path.display().to_string())
            },
            "dataset": {"tasks":count,"time_blocks":100,"sample_count":100,"query_limit":50},
            "insert_tasks_total_ms": insert_total_ms,
            "query_tasks_first_page": statistics(&search_us),
            "query_tasks_tail_match": statistics(&tail_search_us),
            "query_time_blocks": statistics(&schedule_us),
            "complete_task": statistics(&update_us),
            "caveat": "Numbers describe this process, filesystem and host only. Record hardware and OS separately for target-device comparisons."
        }))?
    );
    Ok(())
}
