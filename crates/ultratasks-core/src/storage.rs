use crate::{calendar::*, model::*};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) const SCHEMA: u32 = 1;
pub(crate) const QUERY_VM_BUDGET: u64 = 10_000_000;
pub(crate) const PROGRESS_INTERVAL: i32 = 1000;

pub(crate) fn sql_error(stage: StorageStage, error: rusqlite::Error) -> Error {
    use rusqlite::ErrorCode;
    if matches!(
        &error,
        rusqlite::Error::FromSqlConversionFailure(..)
            | rusqlite::Error::InvalidColumnType(..)
            | rusqlite::Error::IntegralValueOutOfRange(..)
    ) {
        return Error::Storage {
            stage,
            kind: StorageKind::Corrupt,
            outcome: CommitKnowledge::NotCommitted,
        };
    }
    let kind = match error.sqlite_error_code() {
        Some(ErrorCode::OperationInterrupted) if stage == StorageStage::Read => {
            return Error::QueryBudgetExceeded;
        }
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => StorageKind::Busy,
        Some(ErrorCode::DiskFull) => StorageKind::NoSpace,
        Some(ErrorCode::PermissionDenied | ErrorCode::ReadOnly) => StorageKind::PermissionDenied,
        Some(
            ErrorCode::DatabaseCorrupt
            | ErrorCode::NotADatabase
            | ErrorCode::SchemaChanged
            | ErrorCode::ConstraintViolation,
        ) => StorageKind::Corrupt,
        _ => StorageKind::Io,
    };
    Error::Storage {
        stage,
        kind,
        outcome: CommitKnowledge::NotCommitted,
    }
}
pub(crate) fn corrupt() -> Error {
    Error::Storage {
        stage: StorageStage::Read,
        kind: StorageKind::Corrupt,
        outcome: CommitKnowledge::NotCommitted,
    }
}
fn io_error(error: std::io::Error) -> Error {
    let kind = if error.kind() == std::io::ErrorKind::PermissionDenied {
        StorageKind::PermissionDenied
    } else {
        StorageKind::Io
    };
    Error::Storage {
        stage: StorageStage::Open,
        kind,
        outcome: CommitKnowledge::NotCommitted,
    }
}
fn normalized(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return path.canonicalize().map_err(io_error);
    }
    let absolute = std::path::absolute(path).map_err(io_error)?;
    let parent = absolute
        .parent()
        .ok_or_else(|| invalid(Field::Id, InputProblem::Malformed))?;
    std::fs::create_dir_all(parent).map_err(io_error)?;
    let parent = parent.canonicalize().map_err(io_error)?;
    Ok(parent.join(
        absolute
            .file_name()
            .ok_or_else(|| invalid(Field::Id, InputProblem::Malformed))?,
    ))
}
#[cfg(windows)]
fn link_count(file: &File) -> std::io::Result<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // fileの生存中だけhandleを借用し、初期化済みの出力領域を渡す。
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(u64::from(info.nNumberOfLinks))
}

#[cfg(unix)]
fn link_count(file: &File) -> std::io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(file.metadata()?.nlink())
}

#[cfg(not(any(windows, unix)))]
fn link_count(_file: &File) -> std::io::Result<u64> {
    Err(std::io::ErrorKind::Unsupported.into())
}

fn require_single_link(file: &File) -> Result<()> {
    match link_count(file).map_err(io_error)? {
        1 => Ok(()),
        0 => Err(io_error(std::io::ErrorKind::NotFound.into())),
        _ => Err(Error::Storage {
            stage: StorageStage::Open,
            kind: StorageKind::MultipleLinks,
            outcome: CommitKnowledge::NotCommitted,
        }),
    }
}

pub(crate) fn open(path: &Path) -> Result<(Connection, File, StoreInfo)> {
    let path = normalized(path)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
        // SQLiteのhandleへ引き継ぐまで、検査済みの実体の削除・差し替えを許可しない。
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    let database = options.open(&path).map_err(io_error)?;
    // 未作成DBを指すsymlinkも、実体の作成後にlockとWALのパスを揃える。
    let path = path.canonicalize().map_err(io_error)?;
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(".lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(PathBuf::from(sidecar))
        .map_err(io_error)?;
    match lock.try_lock() {
        Ok(()) => (),
        Err(std::fs::TryLockError::WouldBlock) => return Err(Error::AlreadyOpen),
        Err(std::fs::TryLockError::Error(error)) => return Err(io_error(error)),
    }
    // WALはDB名から決まる。SQLiteが別名のWALを読む前に、実ファイルの別名を拒否する。
    require_single_link(&database)?;
    let connection = Connection::open(path).map_err(|e| sql_error(StorageStage::Open, e))?;
    require_single_link(&database)?;
    connection
        .busy_timeout(Duration::from_millis(250))
        .map_err(|e| sql_error(StorageStage::Open, e))?;
    let found: u32 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| sql_error(StorageStage::Open, e))?;
    if found > SCHEMA {
        return Err(Error::UnsupportedSchema {
            found,
            supported: SCHEMA,
        });
    }
    connection
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
        .map_err(|e| sql_error(StorageStage::Open, e))?;
    if found == 0 {
        let objects: u64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| sql_error(StorageStage::Open, e))?;
        if objects != 0 {
            return Err(corrupt());
        }
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| sql_error(StorageStage::Begin, e))?;
        let initialize = (|| {
            connection
                .execute_batch(include_str!("../migrations/001_initial.sql"))
                .map_err(|e| sql_error(StorageStage::Write, e))?;
            let db = new_id()?;
            let actor = ActorId::new()?;
            let list = ListId::new()?;
            let calendar = CalendarId::new()?;
            connection
                .execute(
                    "INSERT INTO meta VALUES(1,?1,?2,?3,?4,0,0,2)",
                    params![db, actor.0, list.0, calendar.0],
                )
                .map_err(|e| sql_error(StorageStage::Write, e))?;
            connection
                .execute("INSERT INTO lists VALUES(?1,1,1,NULL,0,'タスク')", [list.0])
                .map_err(|e| sql_error(StorageStage::Write, e))?;
            connection
                .execute(
                    "INSERT INTO calendars VALUES(?1,1,2,NULL,0,'個人')",
                    [calendar.0],
                )
                .map_err(|e| sql_error(StorageStage::Write, e))?;
            connection
                .execute_batch("COMMIT")
                .map_err(|e| sql_error(StorageStage::Commit, e))
        })();
        if initialize.is_err() {
            let _ = connection.execute_batch("ROLLBACK");
        }
        initialize?;
    }
    let check: String = connection
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(|e| sql_error(StorageStage::Open, e))?;
    if check != "ok" {
        return Err(corrupt());
    }
    let info = connection.query_row("SELECT database_id, actor_id, default_list, default_calendar FROM meta WHERE singleton=1", [], |r| {
        Ok(StoreInfo { database: DatabaseId(r.get(0)?), actor: ActorId(r.get(1)?), default_list: ListId(r.get(2)?), default_calendar: CalendarId(r.get(3)?) })
    }).map_err(|e| sql_error(StorageStage::Open, e))?;
    if info.database.0 == [0; 16] || ActorId::from_bytes(info.actor.0).is_err() {
        return Err(corrupt());
    }
    require_live(&connection, EntityId::List(info.default_list))?;
    require_live(&connection, EntityId::Calendar(info.default_calendar))?;
    Ok((connection, lock, info))
}

pub(crate) fn split(id: EntityId) -> (u8, [u8; 16]) {
    match id {
        EntityId::Task(id) => (0, id.0),
        EntityId::Event(id) => (1, id.0),
        EntityId::TimeBlock(id) => (2, id.0),
        EntityId::List(id) => (3, id.0),
        EntityId::Calendar(id) => (4, id.0),
    }
}
pub(crate) fn join(kind: u8, id: [u8; 16]) -> Result<EntityId> {
    Ok(match kind {
        0 => EntityId::Task(TaskId::from_bytes(id)?),
        1 => EntityId::Event(EventId::from_bytes(id)?),
        2 => EntityId::TimeBlock(TimeBlockId::from_bytes(id)?),
        3 => EntityId::List(ListId::from_bytes(id)?),
        4 => EntityId::Calendar(CalendarId::from_bytes(id)?),
        _ => return Err(corrupt()),
    })
}
pub(crate) fn table(kind: u8) -> Result<&'static str> {
    match kind {
        0 => Ok("tasks"),
        1 => Ok("events"),
        2 => Ok("blocks"),
        3 => Ok("lists"),
        4 => Ok("calendars"),
        _ => Err(corrupt()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum DueImage {
    Date(String),
    At(i64, String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum WhenImage {
    AllDay(String, String),
    Timed(i64, i64, String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Data {
    Task {
        title: String,
        list: [u8; 16],
        done: bool,
        due: Option<DueImage>,
    },
    Event {
        title: String,
        calendar: [u8; 16],
        when: WhenImage,
    },
    Block {
        task: [u8; 16],
        calendar: [u8; 16],
        start: i64,
        end: i64,
        zone: String,
    },
    List(String),
    Calendar(String),
}
impl Data {
    pub(crate) fn task(data: TaskData) -> Self {
        Self::Task {
            title: data.title.as_str().to_owned(),
            list: data.list.0,
            done: data.status == TaskStatus::Done,
            due: data.due.map(|due| match due {
                Due::Date(d) => DueImage::Date(d.iso()),
                Due::At(z) => DueImage::At(z.instant.unix_micros(), z.zone.as_str().to_owned()),
            }),
        }
    }
    pub(crate) fn event(data: EventData) -> Self {
        Self::Event {
            title: data.title.as_str().to_owned(),
            calendar: data.calendar.0,
            when: match data.when {
                EventWhen::AllDay(days) => {
                    let (a, b) = days.bounds();
                    WhenImage::AllDay(a.iso(), b.iso())
                }
                EventWhen::Timed(span) => {
                    let (a, b) = span.bounds();
                    WhenImage::Timed(
                        a.unix_micros(),
                        b.unix_micros(),
                        span.zone().as_str().to_owned(),
                    )
                }
            },
        }
    }
    pub(crate) fn block(task: TaskId, data: BlockData) -> Self {
        let (a, b) = data.span.bounds();
        Self::Block {
            task: task.0,
            calendar: data.calendar.0,
            start: a.unix_micros(),
            end: b.unix_micros(),
            zone: data.span.zone().as_str().to_owned(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Image {
    pub kind: u8,
    pub id: [u8; 16],
    pub position: Option<[u8; 16]>,
    pub data: Option<Data>,
}
impl Image {
    pub(crate) fn absent(id: EntityId) -> Self {
        let (kind, id) = split(id);
        Self {
            kind,
            id,
            position: None,
            data: None,
        }
    }
    pub(crate) fn entity_id(&self) -> Result<EntityId> {
        join(self.kind, self.id)
    }
    pub(crate) fn entity(&self, revision: Revision) -> Result<Entity> {
        let id = self.entity_id()?;
        let Some(data) = &self.data else {
            return Err(Error::NotFound(id));
        };
        Ok(match (id, data) {
            (
                EntityId::Task(id),
                Data::Task {
                    title,
                    list,
                    done,
                    due,
                },
            ) => Entity::Task(Record {
                key: Expected { id, revision },
                data: TaskData {
                    title: Title::new(title)?,
                    list: ListId::from_bytes(*list)?,
                    status: if *done {
                        TaskStatus::Done
                    } else {
                        TaskStatus::Open
                    },
                    due: due
                        .as_ref()
                        .map(|due| {
                            Ok(match due {
                                DueImage::Date(date) => Due::Date(Date::parse(date)?),
                                DueImage::At(us, zone) => Due::At(ZonedInstant {
                                    instant: Instant::from_unix_micros(*us)?,
                                    zone: ZoneId::parse(zone)?,
                                }),
                            })
                        })
                        .transpose()?,
                },
            }),
            (
                EntityId::Event(id),
                Data::Event {
                    title,
                    calendar,
                    when,
                },
            ) => Entity::Event(Record {
                key: Expected { id, revision },
                data: EventData {
                    title: Title::new(title)?,
                    calendar: CalendarId::from_bytes(*calendar)?,
                    when: match when {
                        WhenImage::AllDay(a, b) => {
                            EventWhen::AllDay(DateSpan::new(Date::parse(a)?, Date::parse(b)?)?)
                        }
                        WhenImage::Timed(a, b, zone) => EventWhen::Timed(span(*a, *b, zone)?),
                    },
                },
            }),
            (
                EntityId::TimeBlock(id),
                Data::Block {
                    task,
                    calendar,
                    start,
                    end,
                    zone,
                },
            ) => Entity::TimeBlock(TimeBlock {
                key: Expected { id, revision },
                task: TaskId::from_bytes(*task)?,
                data: BlockData {
                    calendar: CalendarId::from_bytes(*calendar)?,
                    span: span(*start, *end, zone)?,
                },
            }),
            (EntityId::List(id), Data::List(title)) => Entity::List(Record {
                key: Expected { id, revision },
                data: Title::new(title)?,
            }),
            (EntityId::Calendar(id), Data::Calendar(title)) => Entity::Calendar(Record {
                key: Expected { id, revision },
                data: Title::new(title)?,
            }),
            _ => return Err(corrupt()),
        })
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.entity_id()?;
        if self.position == Some([0; 16]) {
            return Err(corrupt());
        }
        if self.data.is_some() {
            self.entity(Revision(1))?;
        }
        Ok(())
    }
}
pub(crate) fn span(start: i64, end: i64, zone: &str) -> Result<ZonedSpan> {
    ZonedSpan::new(
        Instant::from_unix_micros(start)?,
        Instant::from_unix_micros(end)?,
        ZoneId::parse(zone)?,
    )
}
pub(crate) struct StoredRow {
    pub image: Image,
    pub revision: Revision,
    pub created_seq: i64,
}
impl StoredRow {
    pub(crate) fn entity(&self) -> Result<Entity> {
        self.image.entity(self.revision)
    }
}
pub(crate) fn load(connection: &Connection, id: EntityId) -> Result<Option<StoredRow>> {
    let (kind, bytes) = split(id);
    let fields = match kind {
        0 => "title,list_id,status,due_date,due_us,due_zone",
        1 => "title,calendar_id,date_start,date_end,start_us,end_us,zone",
        2 => "task_id,calendar_id,start_us,end_us,zone",
        _ => "title",
    };
    let sql = format!(
        "SELECT revision,created_seq,position,deleted,{fields} FROM {} WHERE id=?1",
        table(kind)?
    );
    let result = connection
        .prepare_cached(&sql)
        .and_then(|mut s| {
            s.query_row([bytes], |r| {
                let deleted: bool = r.get(3)?;
                let data = if deleted {
                    None
                } else {
                    Some(match kind {
                        0 => Data::Task {
                            title: r.get(4)?,
                            list: r.get(5)?,
                            done: r.get(6)?,
                            due: if let Some(date) = r.get::<_, Option<String>>(7)? {
                                Some(DueImage::Date(date))
                            } else if let Some(us) = r.get::<_, Option<i64>>(8)? {
                                Some(DueImage::At(us, r.get(9)?))
                            } else {
                                None
                            },
                        },
                        1 => Data::Event {
                            title: r.get(4)?,
                            calendar: r.get(5)?,
                            when: if let Some(date) = r.get::<_, Option<String>>(6)? {
                                WhenImage::AllDay(date, r.get(7)?)
                            } else {
                                WhenImage::Timed(r.get(8)?, r.get(9)?, r.get(10)?)
                            },
                        },
                        2 => Data::Block {
                            task: r.get(4)?,
                            calendar: r.get(5)?,
                            start: r.get(6)?,
                            end: r.get(7)?,
                            zone: r.get(8)?,
                        },
                        3 => Data::List(r.get(4)?),
                        _ => Data::Calendar(r.get(4)?),
                    })
                };
                Ok(StoredRow {
                    image: Image {
                        kind,
                        id: bytes,
                        position: r.get(2)?,
                        data,
                    },
                    revision: Revision(r.get(0)?),
                    created_seq: r.get(1)?,
                })
            })
            .optional()
        })
        .map_err(|e| sql_error(StorageStage::Read, e))?;
    if let Some(row) = &result {
        row.image.validate().map_err(|_| corrupt())?;
    }
    Ok(result)
}
pub(crate) fn require_live(connection: &Connection, id: EntityId) -> Result<StoredRow> {
    load(connection, id)?
        .filter(|r| r.image.data.is_some())
        .ok_or(Error::NotFound(id))
}
pub(crate) fn expected(
    connection: &Connection,
    id: EntityId,
    revision: Revision,
) -> Result<StoredRow> {
    let row = load(connection, id)?;
    let current = match &row {
        Some(row) if row.image.data.is_some() => CurrentVersion::Live(row.revision),
        Some(row) => CurrentVersion::Deleted(row.revision),
        None => CurrentVersion::Absent,
    };
    if current != CurrentVersion::Live(revision) {
        return Err(Error::Conflict {
            entity: id,
            expected: revision,
            current,
        });
    }
    row.ok_or_else(corrupt)
}
pub(crate) fn write_image(connection: &Connection, image: &Image) -> Result<Change> {
    image.validate()?;
    let id = image.entity_id()?;
    let old = load(connection, id)?;
    let (revision, seq) = if let Some(row) = old {
        (
            row.revision
                .0
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or_else(corrupt)?,
            row.created_seq,
        )
    } else {
        let seq: i64 = connection
            .query_row(
                "UPDATE meta SET sequence=sequence+1 WHERE singleton=1 RETURNING sequence",
                [],
                |r| r.get(0),
            )
            .map_err(|e| sql_error(StorageStage::Write, e))?;
        (1, seq)
    };
    let table = table(image.kind)?;
    let deleted = image.data.is_none();
    connection.prepare_cached(&format!("INSERT INTO {table}(id,revision,created_seq,position,deleted) VALUES(?1,?2,?3,?4,1) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,position=excluded.position,deleted=1"))
        .and_then(|mut s| s.execute(params![image.id,revision,seq,image.position])).map_err(|e| sql_error(StorageStage::Write, e))?;
    let result = (|| -> rusqlite::Result<usize> {
        match &image.data {
        Some(Data::Task { title, list, done, due }) => {
            let (date, us, zone) = match due { Some(DueImage::Date(d)) => (Some(d.as_str()),None,None), Some(DueImage::At(us,z)) => (None,Some(*us),Some(z.as_str())), None => (None,None,None) };
            connection.prepare_cached("UPDATE tasks SET deleted=0,title=?2,list_id=?3,status=?4,due_date=?5,due_us=?6,due_zone=?7 WHERE id=?1")?.execute(params![image.id,title,list,done,date,us,zone])
        }
        Some(Data::Event { title, calendar, when }) => {
            let (date_a,date_b,a,b,zone) = match when { WhenImage::AllDay(a,b) => (Some(a.as_str()),Some(b.as_str()),None,None,None), WhenImage::Timed(a,b,z) => (None,None,Some(*a),Some(*b),Some(z.as_str())) };
            connection.prepare_cached("UPDATE events SET deleted=0,title=?2,calendar_id=?3,date_start=?4,date_end=?5,start_us=?6,end_us=?7,zone=?8 WHERE id=?1")?.execute(params![image.id,title,calendar,date_a,date_b,a,b,zone])
        }
        Some(Data::Block { task, calendar, start, end, zone }) => connection.prepare_cached("UPDATE blocks SET deleted=0,task_id=?2,calendar_id=?3,start_us=?4,end_us=?5,zone=?6 WHERE id=?1")?.execute(params![image.id,task,calendar,start,end,zone]),
        Some(Data::List(title)) | Some(Data::Calendar(title)) => connection.prepare_cached(&format!("UPDATE {table} SET deleted=0,title=?2 WHERE id=?1"))?.execute(params![image.id,title]),
        None => {
            let fields = match image.kind { 0 => "title=NULL,list_id=NULL,status=NULL,due_date=NULL,due_us=NULL,due_zone=NULL", 1 => "title=NULL,calendar_id=NULL,date_start=NULL,date_end=NULL,start_us=NULL,end_us=NULL,zone=NULL", 2 => "task_id=NULL,calendar_id=NULL,start_us=NULL,end_us=NULL,zone=NULL", _ => "title=NULL" };
            connection.prepare_cached(&format!("UPDATE {table} SET {fields} WHERE id=?1"))?.execute([image.id])
        }
    }
    })();
    result.map_err(|e| sql_error(StorageStage::Write, e))?;
    if deleted {
        Ok(Change::Deleted {
            id,
            revision: Revision(revision),
        })
    } else {
        Ok(Change::Upsert(image.entity(Revision(revision))?))
    }
}

pub(crate) fn task_blocks(connection: &Connection, task: TaskId) -> Result<Vec<TimeBlock>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT id FROM blocks WHERE deleted=0 AND task_id=?1 ORDER BY start_us,id LIMIT ?2",
        )
        .map_err(|e| sql_error(StorageStage::Read, e))?;
    let ids = statement
        .query_map(params![task.0, MAX_BLOCKS_PER_TASK + 1], |r| {
            r.get::<_, [u8; 16]>(0)
        })
        .map_err(|e| sql_error(StorageStage::Read, e))?;
    let mut blocks = Vec::new();
    for id in ids {
        let id = TimeBlockId::from_bytes(id.map_err(|e| sql_error(StorageStage::Read, e))?)?;
        let Entity::TimeBlock(block) =
            require_live(connection, EntityId::TimeBlock(id))?.entity()?
        else {
            return Err(corrupt());
        };
        blocks.push(block);
    }
    if blocks.len() > usize::from(MAX_BLOCKS_PER_TASK) {
        return Err(corrupt());
    }
    Ok(blocks)
}
pub(crate) fn references(connection: &Connection, image: &Image, info: &StoreInfo) -> Result<()> {
    let id = image.entity_id()?;
    match &image.data {
        Some(Data::Task { list, .. }) => {
            require_live(connection, EntityId::List(ListId::from_bytes(*list)?))?;
        }
        Some(Data::Event { calendar, .. }) => {
            require_live(
                connection,
                EntityId::Calendar(CalendarId::from_bytes(*calendar)?),
            )?;
        }
        Some(Data::Block { task, calendar, .. }) => {
            require_live(connection, EntityId::Task(TaskId::from_bytes(*task)?))?;
            require_live(
                connection,
                EntityId::Calendar(CalendarId::from_bytes(*calendar)?),
            )?;
            let count: u64 = connection
                .query_row(
                    "SELECT count(*) FROM blocks WHERE deleted=0 AND task_id=?1",
                    [task],
                    |r| r.get(0),
                )
                .map_err(|e| sql_error(StorageStage::Read, e))?;
            if count > MAX_BLOCKS_PER_TASK.into() {
                return Err(Error::LimitExceeded {
                    kind: LimitKind::BlocksPerTask,
                    maximum: MAX_BLOCKS_PER_TASK.into(),
                    observed_at_least: count,
                });
            }
        }
        None => {
            if id == EntityId::List(info.default_list)
                || id == EntityId::Calendar(info.default_calendar)
            {
                return Err(Error::ProtectedCollection(id));
            }
            let sql = match image.kind {
                0 => Some("SELECT EXISTS(SELECT 1 FROM blocks WHERE deleted=0 AND task_id=?1)"),
                3 => Some("SELECT EXISTS(SELECT 1 FROM tasks WHERE deleted=0 AND list_id=?1)"),
                4 => Some(
                    "SELECT EXISTS(SELECT 1 FROM events WHERE deleted=0 AND calendar_id=?1) OR EXISTS(SELECT 1 FROM blocks WHERE deleted=0 AND calendar_id=?1)",
                ),
                _ => None,
            };
            if let Some(sql) = sql {
                let has_refs: bool = connection
                    .query_row(sql, [image.id], |r| r.get(0))
                    .map_err(|e| sql_error(StorageStage::Read, e))?;
                if has_refs {
                    return Err(Error::NotEmpty(id));
                }
            }
        }
        _ => (),
    }
    Ok(())
}
pub(crate) fn generation(connection: &Connection) -> Result<DatabaseRevision> {
    connection
        .query_row("SELECT generation FROM meta WHERE singleton=1", [], |r| {
            r.get(0)
        })
        .map(DatabaseRevision)
        .map_err(|e| sql_error(StorageStage::Read, e))
}
pub(crate) fn epoch(connection: &Connection) -> Result<u64> {
    connection
        .query_row("SELECT epoch FROM meta WHERE singleton=1", [], |r| r.get(0))
        .map_err(|e| sql_error(StorageStage::Read, e))
}
