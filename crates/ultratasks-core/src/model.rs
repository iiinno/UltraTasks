use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntityId {
    Task(TaskId),
    List(ListId),
    Event(EventId),
    TimeBlock(TimeBlockId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DueDate {
    Date(NaiveDate),
    At { at: DateTime<Utc>, timezone: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub completed: bool,
    pub due: Option<DueDate>,
    pub list_id: Option<ListId>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    pub id: ListId,
    pub title: String,
    pub revision: u64,
}

/// All ranges are half-open: the end is excluded. All-day dates are never UTC midnight.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub title: String,
    pub schedule: EventSchedule,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeBlock {
    pub id: TimeBlockId,
    pub task_id: TaskId,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
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

#[derive(Debug, Clone)]
pub enum Outcome {
    Task(Task),
    List(TaskList),
    Event(Event),
    TimeBlock(TimeBlock),
    Deleted(EntityId),
}

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct CalendarQuery {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub timezone: String,
    pub limit: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryStatus {
    pub can_undo: bool,
    pub can_redo: bool,
}
