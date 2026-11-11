use crate::calendar::*;

pub type Result<T> = std::result::Result<T, Error>;
pub const MAX_PAGE_SIZE: u16 = 200;
pub const MAX_QUERY_DAYS: u16 = 366;
pub const MAX_BLOCKS_PER_TASK: u16 = 128;
pub const MAX_REFRESH_TASKS: usize = 129;
pub const MAX_TITLE_CHARS: usize = 240;
pub const MAX_SEARCH_CHARS: usize = 128;
pub const MAX_OPERATION_BYTES: u64 = 1_048_576;
pub const MAX_HISTORY_BYTES: u64 = 16_777_216;
pub const MAX_HISTORY_OPERATIONS: u16 = 100;
pub const HISTORY_DAYS: u16 = 30;

pub(crate) fn invalid(field: Field, problem: InputProblem) -> Error {
    Error::InvalidInput { field, problem }
}

pub(crate) fn new_id() -> Result<[u8; 16]> {
    std::panic::catch_unwind(uuid::Uuid::new_v4)
        .map(|id| *id.as_bytes())
        .map_err(|_| Error::IdUnavailable)
}

macro_rules! entity_ids {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub struct $name(pub(crate) [u8; 16]);
        impl $name {
            pub fn new() -> Result<Self> { Ok(Self(new_id()?)) }
            pub fn from_bytes(value: [u8; 16]) -> Result<Self> {
                if value == [0; 16] { return Err(invalid(Field::Id, InputProblem::Malformed)); }
                Ok(Self(value))
            }
            pub fn parse(value: &str) -> Result<Self> {
                let id = uuid::Uuid::parse_str(value).map_err(|_| invalid(Field::Id, InputProblem::Malformed))?;
                Self::from_bytes(*id.as_bytes())
            }
            pub fn as_bytes(&self) -> &[u8; 16] { &self.0 }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                uuid::Uuid::from_bytes(self.0).fmt(f)
            }
        }
    )+ };
}
entity_ids!(TaskId, EventId, TimeBlockId, ListId, CalendarId, ActorId);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct DatabaseId(pub(crate) [u8; 16]);
impl DatabaseId {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct OperationId(pub(crate) [u8; 16]);
impl OperationId {
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Revision(pub(crate) u64);
impl Revision {
    pub fn get(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct DatabaseRevision(pub(crate) u64);
impl DatabaseRevision {
    pub fn get(self) -> u64 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Expected<I> {
    pub id: I,
    pub revision: Revision,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record<I, T> {
    pub key: Expected<I>,
    pub data: T,
}
#[derive(Clone, Debug)]
pub enum Save<I, T> {
    Create { id: I, data: T },
    Update { expected: Expected<I>, data: T },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Title(String);
fn text(text: &str, field: Field, max: usize, controls: bool) -> Result<String> {
    if controls
        && text
            .chars()
            .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
    {
        return Err(invalid(field, InputProblem::ControlCharacter));
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(invalid(field, InputProblem::Empty));
    }
    if text.chars().count() > max {
        return Err(invalid(field, InputProblem::TooLong));
    }
    Ok(text.to_owned())
}
impl Title {
    pub fn new(value: &str) -> Result<Self> {
        Ok(Self(text(value, Field::Title, MAX_TITLE_CHARS, true)?))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchText(String);
impl SearchText {
    pub fn new(value: &str) -> Result<Self> {
        Ok(Self(text(value, Field::Search, MAX_SEARCH_CHARS, false)?))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskStatus {
    Open,
    Done,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Due {
    Date(Date),
    At(ZonedInstant),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskData {
    pub title: Title,
    pub list: ListId,
    pub status: TaskStatus,
    pub due: Option<Due>,
}
pub type Task = Record<TaskId, TaskData>;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventWhen {
    AllDay(DateSpan),
    Timed(ZonedSpan),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventData {
    pub title: Title,
    pub calendar: CalendarId,
    pub when: EventWhen,
}
pub type Event = Record<EventId, EventData>;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockData {
    pub calendar: CalendarId,
    pub span: ZonedSpan,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeBlock {
    pub key: Expected<TimeBlockId>,
    pub task: TaskId,
    pub data: BlockData,
}
pub type List = Record<ListId, Title>;
pub type Calendar = Record<CalendarId, Title>;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Collection {
    List(List),
    Calendar(Calendar),
}
#[derive(Clone, Debug)]
pub enum CollectionSave {
    List(Save<ListId, Title>),
    Calendar(Save<CalendarId, Title>),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockSaved {
    pub task: Task,
    pub block: TimeBlock,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskSummary {
    pub key: Expected<TaskId>,
    pub title: Title,
    pub status: TaskStatus,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgendaItem {
    Event(Event),
    TimeBlock { block: TimeBlock, task: TaskSummary },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EntityId {
    Task(TaskId),
    Event(EventId),
    TimeBlock(TimeBlockId),
    List(ListId),
    Calendar(CalendarId),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Entity {
    Task(Task),
    Event(Event),
    TimeBlock(TimeBlock),
    List(List),
    Calendar(Calendar),
}
#[derive(Clone, Debug)]
pub enum DeleteTarget {
    Task(Expected<TaskId>),
    Event(Expected<EventId>),
    TimeBlock {
        task: Expected<TaskId>,
        block: Expected<TimeBlockId>,
    },
    List(Expected<ListId>),
    Calendar(Expected<CalendarId>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockSummary {
    pub total_for_task: u16,
    pub in_window: u16,
    pub allocated_micros_in_window: u64,
    pub first_in_window: Option<ZonedSpan>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskRow {
    pub task: Task,
    pub list_title: Title,
    pub blocks: BlockSummary,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDetail {
    pub task: Task,
    pub blocks: Vec<TimeBlock>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusFilter {
    Any,
    Open,
    Done,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DueFilter {
    Any,
    Undated,
    ThroughSelectedDay,
    Within(CalendarWindow),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockFilter {
    Any,
    InSelectedWindow,
    NotInSelectedWindow,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskQuery {
    pub day: CalendarWindow,
    pub list: Option<ListId>,
    pub status: StatusFilter,
    pub due: DueFilter,
    pub blocks: BlockFilter,
    pub title_contains: Option<SearchText>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgendaRange {
    AllDay(DateSpan),
    Timed(TimedWindow),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgendaQuery {
    pub range: AgendaRange,
    pub calendar: Option<CalendarId>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageSize(u16);
impl PageSize {
    pub fn new(value: u16) -> Result<Self> {
        if value == 0 {
            return Err(invalid(Field::Page, InputProblem::OutOfRange));
        }
        if value > MAX_PAGE_SIZE {
            return Err(Error::LimitExceeded {
                kind: LimitKind::PageSize,
                maximum: MAX_PAGE_SIZE.into(),
                observed_at_least: value.into(),
            });
        }
        Ok(Self(value))
    }
    pub fn get(self) -> u16 {
        self.0
    }
}
macro_rules! cursors {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name { pub(crate) opaque: Vec<u8> }
    )+ };
}
cursors!(TaskCursor, AgendaCursor, CollectionCursor);
#[derive(Clone, Debug)]
pub enum PagePosition<C> {
    First,
    After(C),
    Before(C),
}
#[derive(Clone, Debug)]
pub struct PageRequest<C> {
    pub size: PageSize,
    pub position: PagePosition<C>,
}
impl<C> PageRequest<C> {
    pub fn first(size: PageSize) -> Self {
        Self {
            size,
            position: PagePosition::First,
        }
    }
}
#[derive(Clone, Debug)]
pub enum TaskPosition {
    First,
    After(TaskCursor),
    Before(TaskCursor),
    Around(TaskId),
}
#[derive(Clone, Debug)]
pub struct TaskPageRequest {
    pub size: PageSize,
    pub position: TaskPosition,
}
#[derive(Clone, Debug)]
pub struct Page<T, C> {
    pub items: Vec<T>,
    pub previous: Option<C>,
    pub next: Option<C>,
    pub total: u64,
    pub generation: DatabaseRevision,
}
pub type TaskPage = Page<TaskRow, TaskCursor>;
#[derive(Clone, Debug)]
pub struct TaskRefresh {
    pub rows: Vec<TaskRow>,
    pub matching_ids: Vec<TaskId>,
    pub total: u64,
    pub generation: DatabaseRevision,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentVersion {
    Live(Revision),
    Deleted(Revision),
    Absent,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Change {
    Upsert(Entity),
    Deleted { id: EntityId, revision: Revision },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationKind {
    TaskCreated,
    TaskEdited,
    TaskCompleted,
    TaskReopened,
    TaskDeleted,
    EventCreated,
    EventEdited,
    EventDeleted,
    BlockCreated,
    BlockEdited,
    BlockDeleted,
    CollectionCreated,
    CollectionEdited,
    CollectionDeleted,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UndoTarget {
    pub(crate) database: DatabaseId,
    pub(crate) actor: ActorId,
    pub(crate) operation: OperationId,
    pub(crate) epoch: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedoTarget {
    pub(crate) database: DatabaseId,
    pub(crate) actor: ActorId,
    pub(crate) operation: OperationId,
    pub(crate) epoch: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry<T> {
    pub target: T,
    pub kind: OperationKind,
    pub focus: EntityId,
    pub affected: u16,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryState {
    pub undo: Option<HistoryEntry<UndoTarget>>,
    pub redo: Option<HistoryEntry<RedoTarget>>,
}
#[derive(Clone, Debug)]
pub struct Commit<T> {
    pub value: T,
    pub operation: Option<OperationId>,
    pub changed: Vec<Change>,
    pub generation: DatabaseRevision,
    pub history: HistoryState,
}
#[derive(Clone, Debug)]
pub struct StoreInfo {
    pub database: DatabaseId,
    pub actor: ActorId,
    pub default_list: ListId,
    pub default_calendar: CalendarId,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Id,
    Title,
    Search,
    Date,
    Instant,
    Zone,
    LocalTime,
    Range,
    Page,
    Query,
    Fixture,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputProblem {
    Empty,
    Malformed,
    TooLong,
    OutOfRange,
    ControlCharacter,
    NonIncreasingRange,
    UnsupportedPrecision,
    UnknownZone,
    OutsideWindow,
    RequiresSingleDay,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitKind {
    PageSize,
    QueryDays,
    BlocksPerTask,
    ChangeBytes,
    RefreshIds,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryProblem {
    Empty,
    Expired,
    NotOwn,
    NotCurrent,
    WrongDatabase,
    StaleTarget,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageStage {
    Open,
    Read,
    Begin,
    Write,
    Commit,
    Rollback,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageKind {
    Busy,
    NoSpace,
    PermissionDenied,
    Corrupt,
    Io,
    MultipleLinks,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitKnowledge {
    NotCommitted,
    Unknown,
}
#[derive(Clone, Debug)]
pub enum Error {
    InvalidInput {
        field: Field,
        problem: InputProblem,
    },
    LimitExceeded {
        kind: LimitKind,
        maximum: u64,
        observed_at_least: u64,
    },
    NotFound(EntityId),
    AlreadyExists(EntityId),
    NotEmpty(EntityId),
    ProtectedCollection(EntityId),
    AlreadyOpen,
    IdUnavailable,
    TimeBlockTaskMismatch {
        block: TimeBlockId,
        expected: TaskId,
        actual: TaskId,
    },
    Conflict {
        entity: EntityId,
        expected: Revision,
        current: CurrentVersion,
    },
    History(HistoryProblem),
    HistoryConflict {
        entity: EntityId,
    },
    StalePage,
    CursorMismatch,
    QueryBudgetExceeded,
    AmbiguousLocalTime {
        local: LocalDateTime,
        zone: ZoneId,
        earlier: Instant,
        later: Instant,
    },
    NonexistentLocalTime {
        local: LocalDateTime,
        zone: ZoneId,
    },
    TimeDataUnavailable,
    Storage {
        stage: StorageStage,
        kind: StorageKind,
        outcome: CommitKnowledge,
    },
    UnsupportedSchema {
        found: u32,
        supported: u32,
    },
    RecoveryRequired,
    FixtureRequiresEmptyDatabase,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyOpen => f.write_str("このデータベースは既に開かれています"),
            Self::Storage {
                stage: StorageStage::Open,
                kind: StorageKind::MultipleLinks,
                ..
            } => f.write_str("複数のハードリンクを持つデータベースは開けません"),
            Self::RecoveryRequired => {
                f.write_str("保存結果の確認のためデータベースを開き直してください")
            }
            Self::QueryBudgetExceeded => {
                f.write_str("検索の処理上限を超えました。範囲を絞ってください")
            }
            Self::FixtureRequiresEmptyDatabase => {
                f.write_str("デモデータは空のデータベースにだけ作成できます")
            }
            _ => write!(f, "{self:?}"),
        }
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validated_text_is_literal_and_bounded() {
        assert_eq!(Title::new(" 日本語_% ").unwrap().as_str(), "日本語_%");
        assert!(Title::new("\n日本語").is_err());
        assert!(Title::new(&"あ".repeat(241)).is_err());
        assert_eq!(SearchText::new(" %_ ").unwrap().as_str(), "%_");
        assert_ne!(Title::new("ｶﾅ").unwrap(), Title::new("カナ").unwrap());
        assert!(TaskId::from_bytes([0; 16]).is_err());
        let id = TaskId::new().unwrap();
        assert_eq!(TaskId::parse(&id.to_string()).unwrap(), id);
    }
}
