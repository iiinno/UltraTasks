use crate::{
    history::Delta,
    storage::{self, Data},
    *,
};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Small,
    Thousand,
    TenThousand,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Distribution {
    ThirtyDays,
    DenseDay,
}
#[derive(Clone, Debug)]
pub struct Config {
    pub profile: Profile,
    pub distribution: Distribution,
    pub seed: u64,
    pub day: Date,
    pub zone: ZoneId,
}
#[derive(Clone, Debug)]
pub struct Report {
    pub tasks: u64,
    pub events: u64,
    pub time_blocks: u64,
    pub seed: u64,
}

fn id(seed: u64, kind: u8, index: u64) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&seed.to_be_bytes());
    bytes[8..].copy_from_slice(&index.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (kind & 0x3f) | 0x80;
    bytes
}
fn mixed(seed: u64, index: u64) -> u64 {
    let mut x = seed.wrapping_add(index.wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn at(day: Date, hour: u8, minute: u8, zone: &ZoneId) -> Result<ZonedInstant> {
    resolve_local(
        LocalDateTime::parse(&format!("{}T{hour:02}:{minute:02}", day.iso()))?,
        zone,
        OverlapChoice::Earlier,
    )
}
fn span(day: Date, hour: u8, minute: u8, minutes: i64, zone: &ZoneId) -> Result<ZonedSpan> {
    let start = at(day, hour, minute, zone)?.instant;
    ZonedSpan::new(
        start,
        Instant::from_unix_micros(start.unix_micros() + minutes * 60_000_000)?,
        zone.clone(),
    )
}
fn create(store: &mut Store, id: EntityId, data: Data, kind: OperationKind) -> Result<()> {
    let mut changes = Vec::new();
    if let Data::Block { task, .. } = &data {
        let parent = storage::require_live(
            &store.connection,
            EntityId::Task(TaskId::from_bytes(*task)?),
        )?;
        changes.push(Delta::replace(parent.image.clone(), parent.image.data));
    }
    changes.push(store.edited(id, None, data)?);
    store.apply(changes, kind, id)?;
    Ok(())
}
pub fn seed_empty(path: &Path, config: Config) -> Result<Report> {
    let mut store = Store::open(path)?;
    store.transaction(|s| {
        let pristine:bool=s.connection.query_row("SELECT generation=0 AND sequence=2 AND (SELECT count(*) FROM tasks)=0 AND (SELECT count(*) FROM events)=0 AND (SELECT count(*) FROM blocks)=0 AND (SELECT count(*) FROM lists)=1 AND (SELECT count(*) FROM calendars)=1 FROM meta WHERE singleton=1",[],|r| r.get(0)).map_err(|e| storage::sql_error(StorageStage::Read,e))?;
        if !pristine { return Err(Error::FixtureRequiresEmptyDatabase); }
        let work=s.info.default_list;
        let calendar=s.info.default_calendar;
        let life=ListId::from_bytes(id(config.seed,3,1))?;
        let list=storage::require_live(&s.connection,EntityId::List(work))?;
        let edit=s.edited(EntityId::List(work),Some(list.revision),Data::List("仕事".to_owned()))?;
        s.apply(vec![edit],OperationKind::CollectionEdited,EntityId::List(work))?;
        create(s,EntityId::List(life),Data::List("生活".to_owned()),OperationKind::CollectionCreated)?;
        let count=match config.profile { Profile::Small=>5,Profile::Thousand=>1000,Profile::TenThousand=>10000 };
        let event_count=if config.profile==Profile::Small { 3 } else { count/10 };
        let block_count=if config.profile==Profile::Small { 2 } else { count };
        let titles=["カレンダーの時刻指定と取消動作を見直す","日本語入力の変換確定とタスク追加を確認する","デザインレビューで確認する画面と操作をまとめる","今週の支払い予定を確認する","机の書類を片付ける"];
        for i in 0..count {
            let random=mixed(config.seed,i);
            let day=if config.profile==Profile::Small || config.distribution==Distribution::DenseDay { config.day } else { config.day.add_days((random%30) as i32)? };
            let task_id=TaskId::from_bytes(id(config.seed,0,i+1))?;
            let title=if config.profile==Profile::Small { titles[i as usize].to_owned() } else if i%37==0 {
                format!("日本語の長いタイトル {i:05} {}", "画面と入力の確認を続ける。".repeat(17))
            } else if i%13==0 { "日本語入力の確認_%".to_owned() } else { format!("資料と予定の確認 {i:05}") };
            let due=if config.profile==Profile::Small {
                match i { 0=>Some(Due::At(at(day,17,0,&config.zone)?)),1|2=>Some(Due::Date(day)),3=>Some(Due::Date(day.add_days(2)?)),_=>None }
            } else {
                match i%3 { 0=>None,1=>Some(Due::Date(day)),_=>Some(Due::At(at(day,17,0,&config.zone)?)) }
            };
            let data=TaskData { title:Title::new(&title)?,list:if (config.profile==Profile::Small && i<3)||(config.profile!=Profile::Small && i%2==0) { work } else { life },status:if config.profile!=Profile::Small && i%5==0 { TaskStatus::Done } else { TaskStatus::Open },due };
            create(s,EntityId::Task(task_id),Data::task(data),OperationKind::TaskCreated)?;
            if i<block_count {
                let (hour,minute,duration)=if config.profile==Profile::Small { if i==0 { (10,0,90) } else { (14,0,45) } } else if i%17==0 { (23,0,120) } else { (9+(random%9) as u8,((random/9)%4) as u8*15,30+((random/36)%4) as i64*15) };
                let block=BlockData { calendar,span:span(day,hour,minute,duration,&config.zone)? };
                create(s,EntityId::TimeBlock(TimeBlockId::from_bytes(id(config.seed,2,i+1))?),Data::block(task_id,block),OperationKind::BlockCreated)?;
            }
        }
        for i in 0..event_count {
            let random=mixed(config.seed,i+count);
            let day=if config.profile==Profile::Small || config.distribution==Distribution::DenseDay { config.day } else { config.day.add_days((random%30) as i32)? };
            let (title,when)=if config.profile==Profile::Small {
                let (title,hour,minute)=match i { 0=>("今日の予定を確認",9,0),1=>("昼休み",12,30),_=>("デザインレビュー",15,0) };
                (title.to_owned(),EventWhen::Timed(span(day,hour,minute,60,&config.zone)?))
            } else if i%5==0 {
                (format!("終日の予定 {i:04}"),EventWhen::AllDay(DateSpan::new(day,day.add_days(if i%10==0 { 2 } else { 1 })?)?))
            } else {
                (format!("打ち合わせ {i:04}"),EventWhen::Timed(span(day,if i%7==0 { 23 } else { 9+(random%8) as u8 },0,if i%7==0 { 120 } else { 60 },&config.zone)?))
            };
            create(s,EntityId::Event(EventId::from_bytes(id(config.seed,1,i+1))?),Data::event(EventData { title:Title::new(&title)?,calendar,when }),OperationKind::EventCreated)?;
        }
        Ok(Report { tasks:count,events:event_count,time_blocks:block_count,seed:config.seed })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(profile: Profile, distribution: Distribution) -> Config {
        Config {
            profile,
            distribution,
            seed: 42,
            day: Date::parse("2026-11-11").unwrap(),
            zone: ZoneId::parse("Asia/Tokyo").unwrap(),
        }
    }
    #[test]
    fn fixture_small_preserves_nonempty_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.sqlite");
        let c = config(Profile::Small, Distribution::ThirtyDays);
        let report = seed_empty(&path, c.clone()).unwrap();
        assert_eq!((report.tasks, report.events, report.time_blocks), (5, 3, 2));
        assert!(matches!(
            seed_empty(&path, c),
            Err(Error::FixtureRequiresEmptyDatabase)
        ));
        let mut store = Store::open(&path).unwrap();
        let day = config(Profile::Small, Distribution::ThirtyDays).day;
        let window = CalendarWindow::new(
            DateSpan::new(day, day.add_days(1).unwrap()).unwrap(),
            ZoneId::parse("Asia/Tokyo").unwrap(),
        )
        .unwrap();
        let agenda = store
            .agenda(
                AgendaQuery {
                    range: AgendaRange::Timed(window.timed()),
                    calendar: None,
                },
                PageRequest::first(PageSize::new(200).unwrap()),
            )
            .unwrap();
        assert_eq!(agenda.total, 5);
    }
    #[test]
    fn fixture_thousand_queries_use_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thousand.sqlite");
        let c = config(Profile::Thousand, Distribution::DenseDay);
        seed_empty(&path, c.clone()).unwrap();
        let mut store = Store::open(&path).unwrap();
        let day = CalendarWindow::new(
            DateSpan::new(c.day, c.day.add_days(1).unwrap()).unwrap(),
            c.zone,
        )
        .unwrap();
        let query = TaskQuery {
            day: day.clone(),
            list: None,
            status: StatusFilter::Any,
            due: DueFilter::Any,
            blocks: BlockFilter::InSelectedWindow,
            title_contains: None,
        };
        let page = store
            .tasks(
                query,
                TaskPageRequest {
                    size: PageSize::new(200).unwrap(),
                    position: TaskPosition::First,
                },
            )
            .unwrap();
        assert_eq!((page.total, page.items.len()), (1000, 200));
        assert!(page.next.is_some());
        let agenda = store
            .agenda(
                AgendaQuery {
                    range: AgendaRange::Timed(day.timed()),
                    calendar: None,
                },
                PageRequest::first(PageSize::new(200).unwrap()),
            )
            .unwrap();
        assert_eq!((agenda.total, agenda.items.len()), (1080, 200));
        let all_day = store
            .agenda(
                AgendaQuery {
                    range: AgendaRange::AllDay(day.days()),
                    calendar: None,
                },
                PageRequest::first(PageSize::new(200).unwrap()),
            )
            .unwrap();
        assert_eq!(all_day.total, 20);
        for (sql, id, expected) in [
            (
                "EXPLAIN QUERY PLAN SELECT id FROM tasks WHERE deleted=0 AND list_id=?1 AND status=0 ORDER BY created_seq,id LIMIT 100",
                store.info.default_list.0,
                "tasks_list_status_created",
            ),
            (
                "EXPLAIN QUERY PLAN SELECT id FROM blocks WHERE deleted=0 AND task_id=?1 AND start_us<9999999999999999 AND end_us>0 ORDER BY start_us,id LIMIT 100",
                page.items[0].task.key.id.0,
                "blocks_task_start",
            ),
            (
                "EXPLAIN QUERY PLAN SELECT id FROM events WHERE deleted=0 AND calendar_id=?1 AND start_us IS NOT NULL AND start_us<9999999999999999 AND end_us>0 ORDER BY start_us,id LIMIT 100",
                store.info.default_calendar.0,
                "events_calendar_start",
            ),
        ] {
            let mut stmt = store.connection.prepare(sql).unwrap();
            let plans = stmt
                .query_map([id], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            eprintln!("{sql}: {plans:?}");
            assert!(plans.iter().any(|plan| plan.contains(expected)));
        }
    }
    #[test]
    fn fixture_ten_thousand_has_exact_counts_in_both_distributions() {
        let dir = tempfile::tempdir().unwrap();
        for distribution in [Distribution::ThirtyDays, Distribution::DenseDay] {
            let path = dir.path().join(format!("{distribution:?}.sqlite"));
            let report = seed_empty(&path, config(Profile::TenThousand, distribution)).unwrap();
            assert_eq!(
                (report.tasks, report.events, report.time_blocks),
                (10000, 1000, 10000)
            );
            let store = Store::open(&path).unwrap();
            let counts:(u64,u64,u64)=store.connection.query_row("SELECT (SELECT count(*) FROM tasks WHERE deleted=0),(SELECT count(*) FROM events WHERE deleted=0),(SELECT count(*) FROM blocks WHERE deleted=0)",[],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
            assert_eq!(counts, (10000, 1000, 10000));
        }
    }
}
