//! One owner thread serializes SQLite operations. No database work runs in GPUI callbacks.
use std::{path::PathBuf, sync::mpsc, thread};

use chrono::{NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use futures_channel::oneshot;
use ultratasks_core::{
    CalendarQuery, Command, Engine, Event, HistoryStatus, Task, TaskQuery, TimeBlock,
};

pub const PAGE_SIZE: u32 = 1_000;

#[derive(Clone)]
pub struct ViewQuery {
    pub date: NaiveDate,
    pub timezone: Tz,
    pub search: String,
    pub offset: u32,
}

pub enum Operation {
    Refresh,
    Execute(Command),
    Undo,
    Redo,
}

pub struct ScheduledTask {
    pub block: TimeBlock,
    pub title: String,
}

pub struct Snapshot {
    pub tasks: Vec<Task>,
    pub events: Vec<Event>,
    pub blocks: Vec<ScheduledTask>,
    pub history: HistoryStatus,
}

struct Job {
    operation: Operation,
    query: ViewQuery,
    reply: oneshot::Sender<Result<Snapshot, String>>,
}

#[derive(Clone)]
pub struct Backend {
    sender: mpsc::Sender<Job>,
}

impl Backend {
    pub fn start(path: PathBuf) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("ultratasks-storage".into())
            .spawn(move || {
                let mut engine = Engine::open(path).map_err(|error| error.to_string());
                // Dropping all Backend handles closes this channel; accepted jobs finish first.
                while let Ok(job) = receiver.recv() {
                    let result = match &mut engine {
                        Ok(engine) => run(engine, job.operation, &job.query),
                        Err(error) => Err(error.clone()),
                    };
                    let _ = job.reply.send(result);
                }
            })?;
        Ok(Self { sender })
    }

    pub fn request(
        &self,
        operation: Operation,
        query: ViewQuery,
    ) -> oneshot::Receiver<Result<Snapshot, String>> {
        let (reply, result) = oneshot::channel();
        // If the worker stopped, dropping the failed job also cancels its reply.
        let _ = self.sender.send(Job {
            operation,
            query,
            reply,
        });
        result
    }
}

fn run(engine: &mut Engine, operation: Operation, query: &ViewQuery) -> Result<Snapshot, String> {
    match operation {
        Operation::Refresh => {}
        Operation::Execute(command) => {
            engine.execute(command).map_err(|error| error.to_string())?;
        }
        Operation::Undo => {
            engine.undo().map_err(|error| error.to_string())?;
        }
        Operation::Redo => {
            engine.redo().map_err(|error| error.to_string())?;
        }
    }
    snapshot(engine, query).map_err(|error| {
        // A query failure after commit must not be reported as a rolled-back command.
        format!("読み込みに失敗しました。更新後の場合、保存済みの可能性があります。再読み込みしてください: {error}")
    })
}

fn snapshot(engine: &Engine, query: &ViewQuery) -> Result<Snapshot, String> {
    let tasks = engine
        .tasks(&TaskQuery {
            search: (!query.search.is_empty()).then(|| query.search.clone()),
            limit: PAGE_SIZE,
            offset: query.offset,
            ..TaskQuery::default()
        })
        .map_err(|error| error.to_string())?;
    let start = date_boundary(query.date, query.timezone)?;
    let end = next_date_boundary(query.date, query.timezone)?;
    let calendar = CalendarQuery {
        start,
        end,
        timezone: query.timezone.to_string(),
        limit: PAGE_SIZE,
    };
    let events = engine
        .events(&calendar)
        .map_err(|error| error.to_string())?;
    let blocks = engine
        .time_blocks(&calendar)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|block| {
            let title = engine
                .task(block.task_id)
                .map_err(|error| error.to_string())?
                .title;
            Ok(ScheduledTask { block, title })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let history = engine.history_status().map_err(|error| error.to_string())?;
    Ok(Snapshot {
        tasks,
        events,
        blocks,
        history,
    })
}

fn date_boundary(date: NaiveDate, timezone: Tz) -> Result<chrono::DateTime<Utc>, String> {
    // Midnight may be skipped during a timezone transition. The day's first real minute
    // is the boundary. A whole skipped civil date is explicitly rejected.
    for minute in 0..1_440 {
        let local = date
            .and_hms_opt(minute / 60, minute % 60, 0)
            .ok_or("日付の範囲外です")?;
        if let Some(value) = timezone.from_local_datetime(&local).earliest() {
            return Ok(value.with_timezone(&Utc));
        }
    }
    Err("このタイムゾーンには指定の日付が存在しません".into())
}

fn next_date_boundary(date: NaiveDate, timezone: Tz) -> Result<chrono::DateTime<Utc>, String> {
    let mut next = date;
    // A date-line move can skip an entire civil day (e.g. Pacific/Apia, 2011-12-30).
    // The preceding real day's end is the beginning of the next real date.
    for _ in 0..3 {
        next = next.succ_opt().ok_or("日付の範囲外です")?;
        if let Ok(boundary) = date_boundary(next, timezone) {
            return Ok(boundary);
        }
    }
    Err("次の日付の境界を求められません".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_windows_follow_dst_instead_of_assuming_twenty_four_hours() {
        let timezone = chrono_tz::America::New_York;
        for (month, day, hours) in [(3, 8, 23), (11, 1, 25)] {
            let date = NaiveDate::from_ymd_opt(2026, month, day).unwrap();
            let start = date_boundary(date, timezone).unwrap();
            let end = next_date_boundary(date, timezone).unwrap();
            assert_eq!((end - start).num_hours(), hours);
        }
    }

    #[test]
    fn a_skipped_civil_date_does_not_break_the_preceding_day() {
        let timezone = chrono_tz::Pacific::Apia;
        let date = NaiveDate::from_ymd_opt(2011, 12, 29).unwrap();
        let start = date_boundary(date, timezone).unwrap();
        let end = next_date_boundary(date, timezone).unwrap();
        assert_eq!((end - start).num_hours(), 24);
        assert_eq!(
            end.with_timezone(&timezone).date_naive(),
            NaiveDate::from_ymd_opt(2011, 12, 31).unwrap()
        );
        assert!(date_boundary(date.succ_opt().unwrap(), timezone).is_err());
    }
}
