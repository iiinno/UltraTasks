// 各統合テストは別バイナリになるため、共用の準備関数の未使用を許す。
#![allow(dead_code)]

use std::path::PathBuf;
use tempfile::TempDir;
use ultratasks_core::{test_support::*, *};

pub struct Database {
    pub path: PathBuf,
    _directory: TempDir,
}

impl Database {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        Self {
            path: directory.path().join("acceptance.sqlite"),
            _directory: directory,
        }
    }

    pub fn open(&self) -> Store {
        Store::open(&self.path).unwrap()
    }

    pub fn open_as(&self, actor: ActorId, at: &str) -> Store {
        open_as_actor(&self.path, actor, instant(at)).unwrap()
    }
}

pub fn date(value: &str) -> Date {
    Date::parse(value).unwrap()
}

pub fn instant(value: &str) -> Instant {
    Instant::parse(value).unwrap()
}

pub fn zone(value: &str) -> ZoneId {
    ZoneId::parse(value).unwrap()
}

pub fn days(start: &str, end: &str) -> DateSpan {
    DateSpan::new(date(start), date(end)).unwrap()
}

pub fn window(day: &str, timezone: &str) -> CalendarWindow {
    let start = date(day);
    CalendarWindow::new(
        DateSpan::new(start, start.add_days(1).unwrap()).unwrap(),
        zone(timezone),
    )
    .unwrap()
}

pub fn span(start: &str, end: &str) -> ZonedSpan {
    ZonedSpan::new(instant(start), instant(end), zone("Asia/Tokyo")).unwrap()
}

pub fn query() -> TaskQuery {
    TaskQuery {
        day: window("2026-11-11", "Asia/Tokyo"),
        list: None,
        status: StatusFilter::Any,
        due: DueFilter::Any,
        blocks: BlockFilter::Any,
        title_contains: None,
    }
}

pub fn request(size: u16, position: TaskPosition) -> TaskPageRequest {
    TaskPageRequest {
        size: PageSize::new(size).unwrap(),
        position,
    }
}

pub fn first<C>(size: u16) -> PageRequest<C> {
    PageRequest::first(PageSize::new(size).unwrap())
}

pub fn data(store: &Store, title: &str) -> TaskData {
    TaskData {
        title: Title::new(title).unwrap(),
        list: store.info().default_list,
        status: TaskStatus::Open,
        due: None,
    }
}

pub fn create_task(store: &mut Store, title: &str) -> Task {
    let data = data(store, title);
    store
        .save_task(Save::Create {
            id: TaskId::new().unwrap(),
            data,
        })
        .unwrap()
        .value
}

pub fn task(store: &mut Store, id: TaskId) -> Task {
    match store.get(EntityId::Task(id)).unwrap() {
        Entity::Task(task) => task,
        other => panic!("Taskの取得結果が異なる: {other:?}"),
    }
}

pub fn block_data(store: &Store, start: &str, end: &str) -> BlockData {
    BlockData {
        calendar: store.info().default_calendar,
        span: span(start, end),
    }
}

pub fn create_block(store: &mut Store, task: &Task, start: &str, end: &str) -> BlockSaved {
    let data = block_data(store, start, end);
    store
        .save_time_block(
            task.key,
            Save::Create {
                id: TimeBlockId::new().unwrap(),
                data,
            },
        )
        .unwrap()
        .value
}

pub fn create_event(store: &mut Store, title: &str, when: EventWhen) -> Event {
    store
        .save_event(Save::Create {
            id: EventId::new().unwrap(),
            data: EventData {
                title: Title::new(title).unwrap(),
                calendar: store.info().default_calendar,
                when,
            },
        })
        .unwrap()
        .value
}

pub fn create_list(store: &mut Store, title: &str) -> List {
    match store
        .save_collection(CollectionSave::List(Save::Create {
            id: ListId::new().unwrap(),
            data: Title::new(title).unwrap(),
        }))
        .unwrap()
        .value
    {
        Collection::List(list) => list,
        other => panic!("Listの作成結果が異なる: {other:?}"),
    }
}

pub fn create_calendar(store: &mut Store, title: &str) -> Calendar {
    match store
        .save_collection(CollectionSave::Calendar(Save::Create {
            id: CalendarId::new().unwrap(),
            data: Title::new(title).unwrap(),
        }))
        .unwrap()
        .value
    {
        Collection::Calendar(calendar) => calendar,
        other => panic!("Calendarの作成結果が異なる: {other:?}"),
    }
}

pub fn task_ids(page: &TaskPage) -> Vec<TaskId> {
    page.items.iter().map(|row| row.task.key.id).collect()
}

pub fn agenda_id(item: &AgendaItem) -> EntityId {
    match item {
        AgendaItem::Event(event) => EntityId::Event(event.key.id),
        AgendaItem::TimeBlock { block, .. } => EntityId::TimeBlock(block.key.id),
    }
}

pub fn timed_query(day: &str) -> AgendaQuery {
    AgendaQuery {
        range: AgendaRange::Timed(window(day, "Asia/Tokyo").timed()),
        calendar: None,
    }
}

#[derive(Debug, PartialEq)]
pub struct Snapshot {
    entities: Vec<Option<Entity>>,
    history: HistoryState,
    generation: DatabaseRevision,
    counts: (u64, u64, u64, u64),
}

pub fn snapshot(store: &mut Store, ids: &[EntityId]) -> Snapshot {
    let entities = ids
        .iter()
        .map(|id| match store.get(*id) {
            Ok(entity) => Some(entity),
            Err(Error::NotFound(missing)) if missing == *id => None,
            other => panic!("状態の取得に失敗: {other:?}"),
        })
        .collect();
    let history = store.history().unwrap();
    let counts = inspect(store).unwrap();
    let refresh = store.task_rows(query(), &[]).unwrap();
    assert_eq!(refresh.total, counts.live_tasks);
    Snapshot {
        entities,
        history,
        generation: refresh.generation,
        counts: (
            counts.live_tasks,
            counts.live_blocks,
            counts.retained_operations,
            counts.retained_image_bytes,
        ),
    }
}

pub fn assert_rollback<T: std::fmt::Debug>(
    store: &mut Store,
    ids: &[EntityId],
    point: FailPoint,
    action: impl FnOnce(&mut Store) -> Result<T>,
) {
    let before = snapshot(store, ids);
    fail_next(store, point);
    let error = action(store).unwrap_err();
    assert!(
        matches!(
            error,
            Error::Storage {
                outcome: CommitKnowledge::NotCommitted,
                ..
            }
        ),
        "{point:?}: {error:?}"
    );
    assert_eq!(snapshot(store, ids), before, "{point:?}");
}

pub const PRECOMMIT_FAILURES: [FailPoint; 3] = [
    FailPoint::AfterStateWrite,
    FailPoint::AfterHistoryWrite,
    FailPoint::BeforeCommit,
];
