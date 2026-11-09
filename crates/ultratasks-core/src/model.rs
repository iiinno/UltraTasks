use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, str::FromStr};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);
        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }
    };
}
id_type!(TaskId);
id_type!(ListId);
id_type!(EventId);
id_type!(TimeBlockId);
id_type!(CalendarId);
id_type!(ReminderId);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum EntityId {
    Task(TaskId),
    List(ListId),
    Event(EventId),
    TimeBlock(TimeBlockId),
    Calendar(CalendarId),
    Reminder(ReminderId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum DueDate {
    Date(NaiveDate),
    At { at: DateTime<Utc>, timezone: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub completed: bool,
    pub due: Option<DueDate>,
    pub list_id: Option<ListId>,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub exceptions: BTreeMap<NaiveDate, TaskException>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskList {
    pub id: ListId,
    pub title: String,
    pub revision: u64,
}

/// All ranges are half-open: the end is excluded. All-day dates are never UTC midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum EventSchedule {
    Timed {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timezone: String,
    },
    AllDay {
        start: NaiveDate,
        end: NaiveDate,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: EventId,
    pub title: String,
    pub schedule: EventSchedule,
    #[serde(default)]
    pub calendar_id: Option<CalendarId>,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub exceptions: BTreeMap<NaiveDate, EventException>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeBlock {
    pub id: TimeBlockId,
    pub task_id: TaskId,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: String,
    pub revision: u64,
}

/// Frequencies use the original local date as their anchor. Missing month days
/// are skipped; weekly weekdays use ISO Monday=1 ... Sunday=7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum Frequency {
    Daily,
    Weekly { weekdays: Vec<u8> },
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub frequency: Frequency,
    pub interval: u16,
    /// Number of valid generated instances; missing dates and DST gaps do not count.
    pub count: Option<u32>,
    /// Last included local date.
    pub until: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum EventException {
    Skip,
    Override {
        title: Option<String>,
        schedule: EventSchedule,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum TaskException {
    Skip,
    Override {
        title: Option<String>,
        due: DueDate,
        completed: bool,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Calendar {
    pub id: CalendarId,
    pub title: String,
    pub revision: u64,
}

/// Durable reminder policy. Each device decides whether to deliver it.
/// At fires once; Before resolves against each current target occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Reminder {
    pub id: ReminderId,
    pub target: EntityId,
    pub trigger: ReminderTrigger,
    pub enabled: bool,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum ReminderTrigger {
    At(DateTime<Utc>),
    Before { minutes: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventOccurrence {
    pub event_id: EventId,
    pub original_date: NaiveDate,
    pub title: String,
    pub schedule: EventSchedule,
    pub calendar_id: Option<CalendarId>,
    pub revision: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskOccurrence {
    pub task_id: TaskId,
    pub original_date: NaiveDate,
    pub title: String,
    pub due: DueDate,
    pub completed: bool,
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum DueFilter {
    None,
    /// Date-only deadlines compare with local dates. Timed deadlines compare instants.
    Range {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timezone: String,
    },
    Before {
        at: DateTime<Utc>,
        timezone: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum Command {
    CreateCalendar {
        title: String,
    },
    RenameCalendar {
        id: CalendarId,
        expected_revision: u64,
        title: String,
    },
    DeleteCalendar {
        id: CalendarId,
        expected_revision: u64,
    },
    AssignEventCalendar {
        id: EventId,
        expected_revision: u64,
        calendar_id: Option<CalendarId>,
    },
    SetEventRecurrence {
        id: EventId,
        expected_revision: u64,
        recurrence: Option<Recurrence>,
    },
    SetEventException {
        id: EventId,
        expected_revision: u64,
        date: NaiveDate,
        exception: Option<EventException>,
    },
    SetTaskRecurrence {
        id: TaskId,
        expected_revision: u64,
        recurrence: Option<Recurrence>,
    },
    SetTaskException {
        id: TaskId,
        expected_revision: u64,
        date: NaiveDate,
        exception: Option<TaskException>,
    },
    CreateReminder {
        target: EntityId,
        trigger: ReminderTrigger,
    },
    UpdateReminder {
        id: ReminderId,
        expected_revision: u64,
        trigger: ReminderTrigger,
        enabled: bool,
    },
    DeleteReminder {
        id: ReminderId,
        expected_revision: u64,
    },
    CreateList {
        title: String,
    },
    RenameList {
        id: ListId,
        expected_revision: u64,
        title: String,
    },
    DeleteList {
        id: ListId,
        expected_revision: u64,
    },
    CreateTask {
        title: String,
        due: Option<DueDate>,
        list_id: Option<ListId>,
    },
    UpdateTask {
        id: TaskId,
        expected_revision: u64,
        title: String,
        completed: bool,
        due: Option<DueDate>,
        list_id: Option<ListId>,
    },
    SetTaskCompleted {
        id: TaskId,
        expected_revision: u64,
        completed: bool,
    },
    DeleteTask {
        id: TaskId,
        expected_revision: u64,
    },
    CreateEvent {
        title: String,
        schedule: EventSchedule,
    },
    UpdateEvent {
        id: EventId,
        expected_revision: u64,
        title: String,
        schedule: EventSchedule,
    },
    DeleteEvent {
        id: EventId,
        expected_revision: u64,
    },
    ScheduleTask {
        task_id: TaskId,
        expected_task_revision: u64,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timezone: String,
    },
    UpdateTimeBlock {
        id: TimeBlockId,
        expected_revision: u64,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        timezone: String,
    },
    DeleteTimeBlock {
        id: TimeBlockId,
        expected_revision: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub enum Outcome {
    Calendar(Calendar),
    Reminder(Reminder),
    Task(Task),
    List(TaskList),
    Event(Event),
    TimeBlock(TimeBlock),
    Deleted(EntityId),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskQuery {
    pub search: Option<String>,
    pub completed: Option<bool>,
    pub list_id: Option<ListId>,
    pub limit: u32,
    pub offset: u32,
}
impl Default for TaskQuery {
    fn default() -> Self {
        Self {
            search: None,
            completed: None,
            list_id: None,
            limit: 100,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarQuery {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: String,
    pub limit: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoryStatus {
    pub can_undo: bool,
    pub can_redo: bool,
}
