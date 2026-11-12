use crate::worker::Work;
use ultratasks_core::*;

#[derive(Clone)]
pub enum Target {
    Task {
        id: TaskId,
        expected: Option<Expected<TaskId>>,
        blocks: Vec<TimeBlock>,
    },
    Event {
        id: EventId,
        expected: Option<Expected<EventId>>,
    },
    Block {
        id: TimeBlockId,
        expected: Option<Expected<TimeBlockId>>,
        task: TaskSummary,
    },
    List {
        id: ListId,
        expected: Option<Expected<ListId>>,
    },
    Calendar {
        id: CalendarId,
        expected: Option<Expected<CalendarId>>,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DueKind {
    None,
    Date,
    At,
}
#[derive(Clone)]
pub struct Draft {
    pub target: Target,
    pub title: String,
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub list: ListId,
    pub calendar: CalendarId,
    pub status: TaskStatus,
    pub due: DueKind,
    pub all_day: bool,
    pub start_overlap: OverlapChoice,
    pub end_overlap: OverlapChoice,
}
impl Target {
    pub fn id(&self) -> EntityId {
        match self {
            Self::Task { id, .. } => EntityId::Task(*id),
            Self::Event { id, .. } => EntityId::Event(*id),
            Self::Block { id, .. } => EntityId::TimeBlock(*id),
            Self::List { id, .. } => EntityId::List(*id),
            Self::Calendar { id, .. } => EntityId::Calendar(*id),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Task { .. } => "タスク",
            Self::Event { .. } => "予定",
            Self::Block { .. } => "作業時間",
            Self::List { .. } => "リスト",
            Self::Calendar { .. } => "カレンダー",
        }
    }
    pub fn delete(&self) -> Option<DeleteTarget> {
        match self {
            Self::Task { expected, .. } => expected.map(DeleteTarget::Task),
            Self::Event { expected, .. } => expected.map(DeleteTarget::Event),
            Self::Block { expected, task, .. } => expected.map(|block| DeleteTarget::TimeBlock {
                task: task.key,
                block,
            }),
            Self::List { expected, .. } => expected.map(DeleteTarget::List),
            Self::Calendar { expected, .. } => expected.map(DeleteTarget::Calendar),
        }
    }
}
impl Draft {
    pub fn blank(target: Target, day: Date, info: &StoreInfo) -> Self {
        Self {
            target,
            title: String::new(),
            start_date: day.iso(),
            start_time: "09:00".into(),
            end_date: day.iso(),
            end_time: "09:30".into(),
            list: info.default_list,
            calendar: info.default_calendar,
            status: TaskStatus::Open,
            due: DueKind::None,
            all_day: false,
            start_overlap: OverlapChoice::Reject,
            end_overlap: OverlapChoice::Reject,
        }
    }
    pub fn task(detail: TaskDetail, day: Date, zone: &ZoneId, info: &StoreInfo) -> Self {
        let mut draft = Self::blank(
            Target::Task {
                id: detail.task.key.id,
                expected: Some(detail.task.key),
                blocks: detail.blocks,
            },
            day,
            info,
        );
        draft.title = detail.task.data.title.as_str().into();
        draft.list = detail.task.data.list;
        draft.status = detail.task.data.status;
        match detail.task.data.due {
            None => {}
            Some(Due::Date(date)) => {
                draft.due = DueKind::Date;
                draft.start_date = date.iso();
            }
            Some(Due::At(at)) => {
                draft.due = DueKind::At;
                draft.start_overlap = existing_choice(at.instant, zone);
                if let Ok(view) = local_at(at.instant, zone) {
                    draft.start_date = view.datetime.date().iso();
                    let (h, m, _, _) = view.datetime.clock();
                    draft.start_time = format!("{h:02}:{m:02}");
                }
            }
        }
        draft
    }
    pub fn event(event: Event, day: Date, zone: &ZoneId, info: &StoreInfo) -> Self {
        let mut draft = Self::blank(
            Target::Event {
                id: event.key.id,
                expected: Some(event.key),
            },
            day,
            info,
        );
        draft.title = event.data.title.as_str().into();
        draft.calendar = event.data.calendar;
        match event.data.when {
            EventWhen::AllDay(span) => {
                draft.all_day = true;
                let (a, b) = span.bounds();
                draft.start_date = a.iso();
                draft.end_date = b.add_days(-1).unwrap_or(a).iso();
            }
            EventWhen::Timed(span) => draft.set_span(&span, zone),
        }
        draft
    }
    pub fn block(
        block: TimeBlock,
        task: TaskSummary,
        day: Date,
        zone: &ZoneId,
        info: &StoreInfo,
    ) -> Self {
        let mut draft = Self::blank(
            Target::Block {
                id: block.key.id,
                expected: Some(block.key),
                task: task.clone(),
            },
            day,
            info,
        );
        draft.title = task.title.as_str().into();
        draft.calendar = block.data.calendar;
        draft.set_span(&block.data.span, zone);
        draft
    }
    pub fn existing_block(
        detail: TaskDetail,
        id: TimeBlockId,
        day: Date,
        zone: &ZoneId,
        info: &StoreInfo,
    ) -> Result<Self> {
        let block = detail
            .blocks
            .into_iter()
            .find(|block| block.key.id == id)
            .ok_or(Error::NotFound(EntityId::TimeBlock(id)))?;
        let task = TaskSummary {
            key: detail.task.key,
            title: detail.task.data.title,
            status: detail.task.data.status,
        };
        Ok(Self::block(block, task, day, zone, info))
    }
    pub fn set_span(&mut self, span: &ZonedSpan, zone: &ZoneId) {
        let (a, b) = span.bounds();
        self.start_overlap = existing_choice(a, zone);
        self.end_overlap = existing_choice(b, zone);
        for (instant, date, time) in [
            (a, &mut self.start_date, &mut self.start_time),
            (b, &mut self.end_date, &mut self.end_time),
        ] {
            if let Ok(view) = local_at(instant, zone) {
                *date = view.datetime.date().iso();
                let (h, m, _, _) = view.datetime.clock();
                *time = format!("{h:02}:{m:02}");
            }
        }
    }
    fn instant(
        &self,
        date: &str,
        time: &str,
        zone: &ZoneId,
        choice: OverlapChoice,
    ) -> Result<ZonedInstant> {
        let time = normalize_clock(time).ok_or(Error::InvalidInput {
            field: Field::LocalTime,
            problem: InputProblem::Malformed,
        })?;
        resolve_local(
            LocalDateTime::parse(&format!("{date}T{time}:00"))?,
            zone,
            choice,
        )
    }
    pub fn span(&self, zone: &ZoneId) -> Result<ZonedSpan> {
        ZonedSpan::new(
            self.instant(&self.start_date, &self.start_time, zone, self.start_overlap)?
                .instant,
            self.instant(&self.end_date, &self.end_time, zone, self.end_overlap)?
                .instant,
            zone.clone(),
        )
    }
    pub fn command(&self, zone: &ZoneId) -> Result<Work> {
        Ok(match &self.target {
            Target::Task { id, expected, .. } => Work::SaveTask(save(
                *id,
                *expected,
                TaskData {
                    title: Title::new(&self.title)?,
                    list: self.list,
                    status: self.status,
                    due: match self.due {
                        DueKind::None => None,
                        DueKind::Date => Some(Due::Date(Date::parse(&self.start_date)?)),
                        DueKind::At => Some(Due::At(self.instant(
                            &self.start_date,
                            &self.start_time,
                            zone,
                            self.start_overlap,
                        )?)),
                    },
                },
            )),
            Target::Event { id, expected } => Work::SaveEvent(save(
                *id,
                *expected,
                EventData {
                    title: Title::new(&self.title)?,
                    calendar: self.calendar,
                    when: if self.all_day {
                        EventWhen::AllDay(DateSpan::new(
                            Date::parse(&self.start_date)?,
                            Date::parse(&self.end_date)?.add_days(1)?,
                        )?)
                    } else {
                        EventWhen::Timed(self.span(zone)?)
                    },
                },
            )),
            Target::Block { id, expected, task } => Work::SaveBlock(
                task.key,
                save(
                    *id,
                    *expected,
                    BlockData {
                        calendar: self.calendar,
                        span: self.span(zone)?,
                    },
                ),
            ),
            Target::List { id, expected } => Work::SaveCollection(CollectionSave::List(save(
                *id,
                *expected,
                Title::new(&self.title)?,
            ))),
            Target::Calendar { id, expected } => Work::SaveCollection(CollectionSave::Calendar(
                save(*id, *expected, Title::new(&self.title)?),
            )),
        })
    }
}
fn save<I: Copy, T>(id: I, expected: Option<Expected<I>>, data: T) -> Save<I, T> {
    match expected {
        Some(expected) => Save::Update { expected, data },
        None => Save::Create { id, data },
    }
}
fn existing_choice(at: Instant, zone: &ZoneId) -> OverlapChoice {
    if let Ok(local) = local_at(at, zone)
        && let Err(Error::AmbiguousLocalTime { earlier, .. }) =
            resolve_local(local.datetime, zone, OverlapChoice::Reject)
    {
        return if at == earlier {
            OverlapChoice::Earlier
        } else {
            OverlapChoice::Later
        };
    }
    OverlapChoice::Reject
}
pub fn overlap_options(date: &str, time: &str, zone: &ZoneId) -> Option<(String, String)> {
    let clock = normalize_clock(time)?;
    let local = LocalDateTime::parse(&format!("{date}T{clock}:00")).ok()?;
    match resolve_local(local, zone, OverlapChoice::Reject) {
        Err(Error::AmbiguousLocalTime { earlier, later, .. }) => Some((
            super::agenda::instant_label(earlier, zone),
            super::agenda::instant_label(later, zone),
        )),
        _ => None,
    }
}
pub fn normalize_clock(value: &str) -> Option<String> {
    let normalized: String = value
        .trim()
        .chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
            '：' => ':',
            _ => c,
        })
        .collect();
    fn digits(value: &str) -> Option<u8> {
        (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .then(|| value.parse().ok())
            .flatten()
    }
    let (h, m) = if let Some((h, rest)) = normalized.split_once('時') {
        let m = if rest.is_empty() {
            0
        } else {
            digits(rest.strip_suffix('分')?)?
        };
        (digits(h)?, m)
    } else if let Some((h, m)) = normalized.split_once(':') {
        (digits(h)?, digits(m)?)
    } else {
        if !normalized.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        match normalized.len() {
            1 | 2 => (digits(&normalized)?, 0),
            3 | 4 => {
                let (h, m) = normalized.split_at(normalized.len() - 2);
                (digits(h)?, digits(m)?)
            }
            _ => return None,
        }
    };
    (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_committed_japanese_clock_and_rejects_implicit_next_day() {
        for input in ["9", "900", "09:00", "９：００", "9時", "９時", "９時００分"] {
            assert_eq!(normalize_clock(input).as_deref(), Some("09:00"));
        }
        assert_eq!(normalize_clock("９時３０分").as_deref(), Some("09:30"));
        for input in [
            "24:00",
            "12:60",
            "-1",
            "",
            "あ",
            "😀",
            "9時半",
            "9時あ分",
            "9:😀",
            "+9",
            "09\0",
        ] {
            assert_eq!(normalize_clock(input), None, "{input:?}");
        }
    }
    #[test]
    fn unicode_is_safe_in_render_helpers_and_save_commands() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("unicode.sqlite")).unwrap();
        let day = Date::parse("2026-11-11").unwrap();
        let zone = ZoneId::parse("Asia/Tokyo").unwrap();
        let mut draft = Draft::blank(
            Target::Event {
                id: EventId::new().unwrap(),
                expected: None,
            },
            day,
            store.info(),
        );
        draft.title = "入力を保持する".into();
        for text in [
            "9時",
            "９時",
            "あ",
            "😀",
            "👩‍💻",
            "e\u{301}",
            "９：あ",
            "\u{200d}",
        ] {
            draft.start_time = text.into();
            assert!(overlap_options(&draft.start_date, text, &zone).is_none());
            assert_eq!(
                draft.command(&zone).is_ok(),
                ["9時", "９時"].contains(&text)
            );
            assert_eq!(draft.start_time, text);
        }
        for scalar in (0..=0x10ffff).filter_map(char::from_u32) {
            let _ = normalize_clock(&scalar.to_string());
        }
    }
    #[test]
    fn inclusive_all_day_round_trips_single_day_month_and_leap_boundaries() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("all-day.sqlite")).unwrap();
        let zone = ZoneId::parse("Asia/Tokyo").unwrap();
        for (start, last, exclusive) in [
            ("0001-01-01", "0001-01-01", "0001-01-02"),
            ("2026-11-11", "2026-11-11", "2026-11-12"),
            ("2026-12-31", "2027-01-02", "2027-01-03"),
            ("2028-02-28", "2028-02-29", "2028-03-01"),
            ("9999-12-30", "9999-12-30", "9999-12-31"),
        ] {
            let day = Date::parse(start).unwrap();
            let mut draft = Draft::blank(
                Target::Event {
                    id: EventId::new().unwrap(),
                    expected: None,
                },
                day,
                store.info(),
            );
            draft.title = "終日予定".into();
            draft.all_day = true;
            draft.end_date = last.into();
            let Work::SaveEvent(command) = draft.command(&zone).unwrap() else {
                panic!("event expected");
            };
            let saved = store.save_event(command).unwrap().value;
            assert_eq!(
                saved.data.when,
                EventWhen::AllDay(DateSpan::new(day, Date::parse(exclusive).unwrap()).unwrap())
            );
            let mut edited = Draft::event(saved.clone(), day, &zone, store.info());
            assert_eq!(edited.end_date, last);
            let Work::SaveEvent(command) = edited.command(&zone).unwrap() else {
                panic!("event expected");
            };
            let unchanged = store.save_event(command).unwrap().value;
            assert_eq!(unchanged, saved);
            edited.title = "名前だけ編集".into();
            let Work::SaveEvent(command) = edited.command(&zone).unwrap() else {
                panic!("event expected");
            };
            assert_eq!(
                store.save_event(command).unwrap().value.data.when,
                saved.data.when
            );
            if let Ok(previous) = day.add_days(-1) {
                edited.end_date = previous.iso();
                assert!(matches!(
                    edited.command(&zone),
                    Err(Error::InvalidInput {
                        problem: InputProblem::NonIncreasingRange,
                        ..
                    })
                ));
            }
            edited.end_date = "9999-12-31".into();
            assert!(matches!(
                edited.command(&zone),
                Err(Error::InvalidInput {
                    field: Field::Date,
                    problem: InputProblem::OutOfRange,
                })
            ));
            assert_eq!(edited.end_date, "9999-12-31");
        }
    }
    #[test]
    fn opening_existing_block_after_parent_save_uses_latest_parent_revision() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("navigation.sqlite")).unwrap();
        let info = store.info().clone();
        let day = Date::parse("2026-11-11").unwrap();
        let zone = ZoneId::parse("Asia/Tokyo").unwrap();
        let task = store
            .save_task(Save::Create {
                id: TaskId::new().unwrap(),
                data: TaskData {
                    title: Title::new("変更前").unwrap(),
                    list: info.default_list,
                    status: TaskStatus::Open,
                    due: None,
                },
            })
            .unwrap()
            .value;
        let template = Draft::blank(
            Target::Event {
                id: EventId::new().unwrap(),
                expected: None,
            },
            day,
            &info,
        );
        let first = store
            .save_time_block(
                task.key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: BlockData {
                        calendar: info.default_calendar,
                        span: template.span(&zone).unwrap(),
                    },
                },
            )
            .unwrap()
            .value;
        let destination = first.block.key.id;
        let mut parent = Draft::task(store.task_detail(task.key.id).unwrap(), day, &zone, &info);
        parent.title = "変更後".into();
        let Work::SaveTask(command) = parent.command(&zone).unwrap() else {
            panic!("task expected");
        };
        let latest = store.save_task(command).unwrap().value;
        let mut block = Draft::existing_block(
            store.task_detail(task.key.id).unwrap(),
            destination,
            day,
            &zone,
            &info,
        )
        .unwrap();
        block.start_time = "09:15".into();
        let Work::SaveBlock(parent, command) = block.command(&zone).unwrap() else {
            panic!("block expected");
        };
        assert_eq!(parent, latest.key);
        assert_ne!(parent, first.task.key);
        assert_eq!(block.title, "変更後");
        assert!(store.save_time_block(parent, command).is_ok());
    }
}
