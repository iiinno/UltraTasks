#![cfg(feature = "test-support")]

mod common;

use common::*;
use ultratasks_core::*;

fn expect_tasks(store: &mut Store, query: TaskQuery, expected: &[TaskId]) {
    let page = store
        .tasks(query, request(200, TaskPosition::First))
        .unwrap();
    assert_eq!(page.total, expected.len() as u64);
    assert_eq!(task_ids(&page), expected);
    assert!(page.previous.is_none());
    assert!(page.next.is_none());
}

#[test]
fn pc8_title_and_search_validate_unicode_scalar_limits_without_normalizing_content() {
    assert_eq!(
        Title::new("　 日本語 😀 e\u{301} 　").unwrap().as_str(),
        "日本語 😀 e\u{301}"
    );
    assert_eq!(
        Title::new(&"😀".repeat(240))
            .unwrap()
            .as_str()
            .chars()
            .count(),
        240
    );
    assert!(matches!(
        Title::new(&"😀".repeat(241)),
        Err(Error::InvalidInput {
            field: Field::Title,
            problem: InputProblem::TooLong
        })
    ));
    assert_eq!(
        SearchText::new(&"日".repeat(128))
            .unwrap()
            .as_str()
            .chars()
            .count(),
        128
    );
    assert!(matches!(
        SearchText::new(&"日".repeat(129)),
        Err(Error::InvalidInput {
            field: Field::Search,
            problem: InputProblem::TooLong
        })
    ));
    for title in ["", "　 ", "前\n後", "前\r後", "前\t後", "前\0後"] {
        assert!(Title::new(title).is_err(), "{title:?}");
    }
    assert!(SearchText::new("　 ").is_err());
    assert_eq!(SearchText::new(" 100%_ ").unwrap().as_str(), "100%_");
    assert_eq!(PageSize::new(1).unwrap().get(), 1);
    assert_eq!(PageSize::new(200).unwrap().get(), 200);
    assert!(PageSize::new(0).is_err());
    assert!(matches!(
        PageSize::new(201),
        Err(Error::LimitExceeded {
            kind: LimitKind::PageSize,
            maximum: 200,
            observed_at_least: 201
        })
    ));
}

#[test]
fn pc8_japanese_literal_substrings_do_not_fold_case_kana_width_or_unicode_normalization() {
    let db = Database::new();
    let mut store = db.open();
    let titles = [
        "日本語入力を確認",
        "進捗100%_完了",
        "進捗100X完了",
        "abc",
        "ABC",
        "カナ",
        "かな",
        "ｶﾅ",
        "é",
        "e\u{301}",
        "' OR 1=1 --",
    ];
    let tasks: Vec<_> = titles
        .iter()
        .map(|title| create_task(&mut store, title))
        .collect();
    for (text, indices) in [
        ("語入", vec![0]),
        ("100%", vec![1]),
        ("_", vec![1]),
        ("%_", vec![1]),
        ("abc", vec![3]),
        ("ABC", vec![4]),
        ("カナ", vec![5]),
        ("かな", vec![6]),
        ("ｶﾅ", vec![7]),
        ("é", vec![8]),
        ("e\u{301}", vec![9]),
        ("' OR 1=1 --", vec![10]),
        ("存在しない", vec![]),
    ] {
        let mut q = query();
        q.title_contains = Some(SearchText::new(text).unwrap());
        let expected: Vec<_> = indices.iter().map(|i| tasks[*i].key.id).collect();
        expect_tasks(&mut store, q, &expected);
    }
    assert_eq!(
        store
            .tasks(query(), request(200, TaskPosition::First))
            .unwrap()
            .total,
        11
    );
}

#[test]
fn pc8_list_status_due_and_blocks_filters_are_anded_and_never_duplicate_tasks() {
    let db = Database::new();
    let mut store = db.open();
    let a = create_list(&mut store, "仕事");
    let b = create_list(&mut store, "生活");
    let dates = [
        Some(Due::Date(date("2026-11-11"))),
        Some(Due::Date(date("2026-11-10"))),
        Some(Due::At(ZonedInstant {
            instant: instant("2026-11-11T23:59:59.999999+09:00"),
            zone: zone("Asia/Tokyo"),
        })),
        Some(Due::Date(date("2026-11-12"))),
        Some(Due::At(ZonedInstant {
            instant: instant("2026-11-12T00:00:00+09:00"),
            zone: zone("Asia/Tokyo"),
        })),
        None,
        Some(Due::At(ZonedInstant {
            instant: instant("2026-11-10T23:59:59.999999+09:00"),
            zone: zone("Asia/Tokyo"),
        })),
        Some(Due::At(ZonedInstant {
            instant: instant("2026-11-11T00:00:00+09:00"),
            zone: zone("Asia/Tokyo"),
        })),
    ];
    let mut tasks = Vec::new();
    for (i, due) in dates.into_iter().enumerate() {
        tasks.push(
            store
                .save_task(Save::Create {
                    id: TaskId::new().unwrap(),
                    data: TaskData {
                        title: Title::new(&format!("日本語入力 {i}")).unwrap(),
                        list: if i == 2 { b.key.id } else { a.key.id },
                        status: if i == 1 {
                            TaskStatus::Done
                        } else {
                            TaskStatus::Open
                        },
                        due,
                    },
                })
                .unwrap()
                .value,
        );
    }
    for (index, start, end) in [
        (0, "2026-11-11T09:00:00+09:00", "2026-11-11T10:00:00+09:00"),
        (0, "2026-11-11T09:30:00+09:00", "2026-11-11T10:30:00+09:00"),
        (2, "2026-11-10T23:00:00+09:00", "2026-11-11T00:30:00+09:00"),
        (3, "2026-11-12T00:00:00+09:00", "2026-11-12T01:00:00+09:00"),
        (5, "2026-11-10T23:00:00+09:00", "2026-11-11T00:00:00+09:00"),
    ] {
        tasks[index] = create_block(&mut store, &tasks[index], start, end).task;
    }
    let ids = |indices: &[usize]| indices.iter().map(|i| tasks[*i].key.id).collect::<Vec<_>>();
    expect_tasks(&mut store, query(), &ids(&[0, 1, 2, 3, 4, 5, 6, 7]));
    let mut q = query();
    q.list = Some(a.key.id);
    expect_tasks(&mut store, q, &ids(&[0, 1, 3, 4, 5, 6, 7]));
    for (status, expected) in [
        (StatusFilter::Open, vec![0, 2, 3, 4, 5, 6, 7]),
        (StatusFilter::Done, vec![1]),
    ] {
        let mut q = query();
        q.status = status;
        expect_tasks(&mut store, q, &ids(&expected));
    }
    for (due, expected) in [
        (DueFilter::Undated, vec![5]),
        (DueFilter::ThroughSelectedDay, vec![0, 1, 2, 6, 7]),
        (DueFilter::Within(query().day), vec![0, 2, 7]),
    ] {
        let mut q = query();
        q.due = due;
        expect_tasks(&mut store, q, &ids(&expected));
    }
    for (blocks, expected) in [
        (BlockFilter::InSelectedWindow, vec![0, 2]),
        (BlockFilter::NotInSelectedWindow, vec![1, 3, 4, 5, 6, 7]),
    ] {
        let mut q = query();
        q.blocks = blocks;
        expect_tasks(&mut store, q, &ids(&expected));
    }
    let mut q = query();
    q.list = Some(a.key.id);
    q.status = StatusFilter::Open;
    q.due = DueFilter::ThroughSelectedDay;
    q.blocks = BlockFilter::InSelectedWindow;
    q.title_contains = Some(SearchText::new("語入力").unwrap());
    expect_tasks(&mut store, q, &ids(&[0]));
}

#[test]
fn pc8_pc9_due_filters_use_complete_local_days_on_dst_and_skipped_days() {
    for (day, timezone, before_start, start, before_end, end, expected_within) in [
        (
            "2026-03-08",
            "America/New_York",
            "2026-03-08T04:59:59.999999Z",
            "2026-03-08T05:00:00Z",
            "2026-03-09T03:59:59.999999Z",
            "2026-03-09T04:00:00Z",
            vec![0, 4, 5],
        ),
        (
            "2026-11-01",
            "America/New_York",
            "2026-11-01T03:59:59.999999Z",
            "2026-11-01T04:00:00Z",
            "2026-11-02T04:59:59.999999Z",
            "2026-11-02T05:00:00Z",
            vec![0, 4, 5],
        ),
        (
            "2011-12-30",
            "Pacific/Apia",
            "2011-12-30T09:59:59.999999Z",
            "2011-12-30T10:00:00Z",
            "2011-12-30T09:59:59.999999Z",
            "2011-12-30T10:00:00Z",
            vec![0],
        ),
    ] {
        let db = Database::new();
        let mut store = db.open();
        let w = window(day, timezone);
        let due = [
            Some(Due::Date(date(day))),
            Some(Due::Date(date(day).add_days(-1).unwrap())),
            Some(Due::Date(date(day).add_days(1).unwrap())),
            Some(Due::At(ZonedInstant {
                instant: instant(before_start),
                zone: zone(timezone),
            })),
            Some(Due::At(ZonedInstant {
                instant: instant(start),
                zone: zone(timezone),
            })),
            Some(Due::At(ZonedInstant {
                instant: instant(before_end),
                zone: zone(timezone),
            })),
            Some(Due::At(ZonedInstant {
                instant: instant(end),
                zone: zone(timezone),
            })),
            None,
        ];
        let tasks: Vec<_> = due
            .into_iter()
            .enumerate()
            .map(|(i, due)| {
                let mut input = data(&store, &format!("期限-{i}"));
                input.due = due;
                store
                    .save_task(Save::Create {
                        id: TaskId::new().unwrap(),
                        data: input,
                    })
                    .unwrap()
                    .value
            })
            .collect();
        let mut q = query();
        q.day = w.clone();
        q.due = DueFilter::Within(w);
        expect_tasks(
            &mut store,
            q.clone(),
            &expected_within
                .iter()
                .map(|i| tasks[*i].key.id)
                .collect::<Vec<_>>(),
        );
        q.due = DueFilter::ThroughSelectedDay;
        let through = if timezone == "Pacific/Apia" {
            vec![0, 1, 3, 5]
        } else {
            vec![0, 1, 3, 4, 5]
        };
        expect_tasks(
            &mut store,
            q,
            &through.iter().map(|i| tasks[*i].key.id).collect::<Vec<_>>(),
        );
    }
}

#[test]
fn pc8_through_selected_day_rejects_a_multi_day_window_without_affecting_state() {
    let db = Database::new();
    let mut store = db.open();
    let task = create_task(&mut store, "日付窓の誤用");
    let before = snapshot(&mut store, &[EntityId::Task(task.key.id)]);
    let mut q = query();
    q.day = CalendarWindow::new(days("2026-11-11", "2026-11-13"), zone("Asia/Tokyo")).unwrap();
    q.due = DueFilter::ThroughSelectedDay;
    assert!(matches!(
        store.tasks(q.clone(), request(10, TaskPosition::First)),
        Err(Error::InvalidInput {
            field: Field::Query,
            problem: InputProblem::RequiresSingleDay
        })
    ));
    assert!(matches!(
        store.task_rows(q, &[task.key.id]),
        Err(Error::InvalidInput {
            field: Field::Query,
            problem: InputProblem::RequiresSingleDay
        })
    ));
    assert_eq!(snapshot(&mut store, &[EntityId::Task(task.key.id)]), before);
}

#[test]
fn pc8_task_keyset_pages_move_forward_and_backward_with_exact_totals_and_stable_order() {
    let db = Database::new();
    let mut store = db.open();
    let tasks: Vec<_> = (0..8)
        .map(|i| create_task(&mut store, &format!("作成順{i}")))
        .collect();
    let ids: Vec<_> = tasks.iter().map(|t| t.key.id).collect();
    let mut edited = tasks[2].data.clone();
    edited.title = Title::new("編集しても移動しない").unwrap();
    store
        .save_task(Save::Update {
            expected: tasks[2].key,
            data: edited,
        })
        .unwrap();
    let first = store
        .tasks(query(), request(3, TaskPosition::First))
        .unwrap();
    assert_eq!(first.total, 8);
    assert_eq!(task_ids(&first), ids[..3]);
    assert!(first.previous.is_none());
    let second = store
        .tasks(
            query(),
            request(3, TaskPosition::After(first.next.clone().unwrap())),
        )
        .unwrap();
    assert_eq!(task_ids(&second), ids[3..6]);
    assert_eq!((second.total, second.generation), (8, first.generation));
    let back = store
        .tasks(
            query(),
            request(3, TaskPosition::Before(second.previous.unwrap())),
        )
        .unwrap();
    assert_eq!(task_ids(&back), ids[..3]);
    assert!(back.previous.is_none());
    let last = store
        .tasks(
            query(),
            request(3, TaskPosition::After(second.next.unwrap())),
        )
        .unwrap();
    assert_eq!(task_ids(&last), ids[6..]);
    assert_eq!(last.total, 8);
    assert!(last.next.is_none());
    let back = store
        .tasks(
            query(),
            request(3, TaskPosition::Before(last.previous.unwrap())),
        )
        .unwrap();
    assert_eq!(task_ids(&back), ids[3..6]);
    let one = store
        .tasks(query(), request(1, TaskPosition::First))
        .unwrap();
    assert_eq!(task_ids(&one), ids[..1]);
    assert_eq!(one.total, 8);
    assert!(one.next.is_some());
}

#[test]
fn pc8_maximum_page_returns_200_rows_and_exposes_the_201st() {
    let db = Database::new();
    let mut store = db.open();
    let tasks: Vec<_> = (0..201)
        .map(|i| create_task(&mut store, &format!("件数境界{i}")))
        .collect();
    let first = store
        .tasks(query(), request(200, TaskPosition::First))
        .unwrap();
    assert_eq!((first.total, first.items.len()), (201, 200));
    assert_eq!(
        task_ids(&first),
        tasks[..200].iter().map(|t| t.key.id).collect::<Vec<_>>()
    );
    let next = store
        .tasks(
            query(),
            request(200, TaskPosition::After(first.next.unwrap())),
        )
        .unwrap();
    assert_eq!(task_ids(&next), vec![tasks[200].key.id]);
    assert_eq!(next.total, 201);
    assert!(next.next.is_none());
}

#[test]
fn pc8_task_cursors_reject_another_query_database_or_generation() {
    let db = Database::new();
    let mut store = db.open();
    let first_task = create_task(&mut store, "先頭");
    create_task(&mut store, "続き");
    let cursor = store
        .tasks(query(), request(1, TaskPosition::First))
        .unwrap()
        .next
        .unwrap();
    let mut other_query = query();
    other_query.status = StatusFilter::Open;
    assert!(matches!(
        store.tasks(other_query, request(1, TaskPosition::After(cursor.clone()))),
        Err(Error::CursorMismatch)
    ));
    let mut other_day = query();
    other_day.day = window("2026-11-12", "Asia/Tokyo");
    assert!(matches!(
        store.tasks(other_day, request(1, TaskPosition::Before(cursor.clone()))),
        Err(Error::CursorMismatch)
    ));
    let another_db = Database::new();
    let mut another = another_db.open();
    create_task(&mut another, "先頭");
    create_task(&mut another, "続き");
    assert!(matches!(
        another.tasks(query(), request(1, TaskPosition::After(cursor.clone()))),
        Err(Error::CursorMismatch)
    ));
    let ids = [EntityId::Task(first_task.key.id)];
    let before = snapshot(&mut store, &ids);
    let noop = store
        .save_task(Save::Update {
            expected: first_task.key,
            data: first_task.data.clone(),
        })
        .unwrap();
    assert!(noop.operation.is_none());
    assert_eq!(snapshot(&mut store, &ids), before);
    assert_eq!(
        store
            .tasks(query(), request(1, TaskPosition::After(cursor.clone())))
            .unwrap()
            .items
            .len(),
        1
    );
    create_event(
        &mut store,
        "世代だけが変わる",
        EventWhen::AllDay(days("2026-11-11", "2026-11-12")),
    );
    let before = snapshot(&mut store, &ids);
    assert!(matches!(
        store.tasks(query(), request(1, TaskPosition::After(cursor))),
        Err(Error::StalePage)
    ));
    assert_eq!(snapshot(&mut store, &ids), before);
}

#[test]
fn pc8_around_retains_position_at_edges_after_filtering_deletion_and_restoration() {
    let db = Database::new();
    let mut store = db.open();
    let tasks: Vec<_> = (0..9)
        .map(|i| create_task(&mut store, &format!("位置{i}")))
        .collect();
    let ids: Vec<_> = tasks.iter().map(|t| t.key.id).collect();
    for (anchor, size, expected) in [
        (ids[4], 5, vec![ids[2], ids[3], ids[4], ids[5], ids[6]]),
        (ids[4], 4, vec![ids[2], ids[3], ids[4], ids[5]]),
        (ids[0], 5, ids[..5].to_vec()),
        (ids[8], 5, ids[4..].to_vec()),
    ] {
        let page = store
            .tasks(query(), request(size, TaskPosition::Around(anchor)))
            .unwrap();
        assert_eq!(task_ids(&page), expected);
        assert_eq!(page.total, 9);
    }
    let mut done = tasks[4].data.clone();
    done.status = TaskStatus::Done;
    let saved = store
        .save_task(Save::Update {
            expected: tasks[4].key,
            data: done,
        })
        .unwrap();
    let mut open = query();
    open.status = StatusFilter::Open;
    let expected = vec![ids[2], ids[3], ids[5], ids[6], ids[7]];
    let filtered = store
        .tasks(open.clone(), request(5, TaskPosition::Around(ids[4])))
        .unwrap();
    assert_eq!(task_ids(&filtered), expected);
    assert_eq!(filtered.total, 8);
    let deleted = store.delete(DeleteTarget::Task(saved.value.key)).unwrap();
    let page = store
        .tasks(query(), request(5, TaskPosition::Around(ids[4])))
        .unwrap();
    assert_eq!(task_ids(&page), expected);
    assert_eq!(page.total, 8);
    assert_eq!(
        task_ids(
            &store
                .tasks(query(), request(1, TaskPosition::Around(ids[4])))
                .unwrap()
        ),
        vec![ids[5]]
    );
    store.undo(deleted.history.undo.unwrap().target).unwrap();
    assert_eq!(
        task_ids(
            &store
                .tasks(query(), request(5, TaskPosition::Around(ids[4])))
                .unwrap()
        ),
        ids[2..7]
    );
    let unknown = TaskId::new().unwrap();
    assert!(
        matches!(store.tasks(query(), request(5, TaskPosition::Around(unknown))), Err(Error::NotFound(EntityId::Task(id))) if id == unknown)
    );
    open.title_contains = Some(SearchText::new("一致しない").unwrap());
    let empty = store
        .tasks(open, request(5, TaskPosition::Around(ids[4])))
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(empty.items.is_empty() && empty.previous.is_none() && empty.next.is_none());
}

#[test]
fn pc8_task_rows_returns_requested_live_rows_including_newly_nonmatching_rows_only_once() {
    let db = Database::new();
    let mut store = db.open();
    let a = create_task(&mut store, "完了する行");
    let b = create_task(&mut store, "未完了の行");
    let c = create_task(&mut store, "削除する行");
    let d = create_task(&mut store, "要求範囲外");
    let mut done = a.data.clone();
    done.status = TaskStatus::Done;
    let a = store
        .save_task(Save::Update {
            expected: a.key,
            data: done,
        })
        .unwrap()
        .value;
    let deleted = store.delete(DeleteTarget::Task(c.key)).unwrap();
    let mut q = query();
    q.status = StatusFilter::Open;
    let absent = TaskId::new().unwrap();
    let refresh = store
        .task_rows(q.clone(), &[b.key.id, a.key.id, a.key.id, c.key.id, absent])
        .unwrap();
    assert_eq!(refresh.total, 2);
    assert_eq!(refresh.generation, deleted.generation);
    assert_eq!(refresh.rows.len(), 2);
    assert!(refresh.rows.iter().any(|row| row.task == a));
    assert!(refresh.rows.iter().any(|row| row.task == b));
    assert_eq!(refresh.matching_ids, vec![b.key.id]);
    assert!(!refresh.rows.iter().any(|row| row.task.key.id == d.key.id));
    let empty = store.task_rows(q, &[]).unwrap();
    assert_eq!((empty.total, empty.generation), (2, refresh.generation));
    assert!(empty.rows.is_empty() && empty.matching_ids.is_empty());
}

#[test]
fn pc8_task_rows_accepts_129_distinct_ids_and_rejects_130_without_changes() {
    let db = Database::new();
    let mut store = db.open();
    let saved = create_task(&mut store, "更新する可視行");
    let mut ids = vec![saved.key.id];
    ids.extend((0..128).map(|_| TaskId::new().unwrap()));
    let page = store.task_rows(query(), &ids).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].task, saved);
    assert_eq!(page.matching_ids, vec![saved.key.id]);
    ids.push(TaskId::new().unwrap());
    let before = snapshot(&mut store, &[EntityId::Task(saved.key.id)]);
    assert!(matches!(
        store.task_rows(query(), &ids),
        Err(Error::LimitExceeded {
            kind: LimitKind::RefreshIds,
            maximum: 129,
            observed_at_least: 130
        })
    ));
    assert_eq!(
        snapshot(&mut store, &[EntityId::Task(saved.key.id)]),
        before
    );
}

#[test]
fn pc8_agenda_paging_is_start_then_kind_then_id_with_no_duplicates_at_equal_start_times() {
    let db = Database::new();
    let mut store = db.open();
    let parent = create_task(&mut store, "並び順");
    let range = span("2026-11-11T09:00:00+09:00", "2026-11-11T10:00:00+09:00");
    let event_ids = [
        EventId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
        EventId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
    ];
    let block_ids = [
        TimeBlockId::parse("00000000-0000-4000-8000-000000000001").unwrap(),
        TimeBlockId::parse("00000000-0000-4000-8000-000000000002").unwrap(),
    ];
    for id in event_ids.iter().rev() {
        store
            .save_event(Save::Create {
                id: *id,
                data: EventData {
                    title: Title::new("同時刻の予定").unwrap(),
                    calendar: store.info().default_calendar,
                    when: EventWhen::Timed(range.clone()),
                },
            })
            .unwrap();
    }
    let mut current = parent;
    for id in block_ids.iter().rev() {
        current = store
            .save_time_block(
                current.key,
                Save::Create {
                    id: *id,
                    data: BlockData {
                        calendar: store.info().default_calendar,
                        span: range.clone(),
                    },
                },
            )
            .unwrap()
            .value
            .task;
    }
    let early = create_event(
        &mut store,
        "先の予定",
        EventWhen::Timed(span(
            "2026-11-11T08:00:00+09:00",
            "2026-11-11T08:30:00+09:00",
        )),
    );
    let late = create_event(
        &mut store,
        "後の予定",
        EventWhen::Timed(span(
            "2026-11-11T10:00:00+09:00",
            "2026-11-11T11:00:00+09:00",
        )),
    );
    let expected = [
        EntityId::Event(early.key.id),
        EntityId::Event(event_ids[0]),
        EntityId::Event(event_ids[1]),
        EntityId::TimeBlock(block_ids[0]),
        EntityId::TimeBlock(block_ids[1]),
        EntityId::Event(late.key.id),
    ];
    let first_page = store.agenda(timed_query("2026-11-11"), first(2)).unwrap();
    assert_eq!(
        first_page.items.iter().map(agenda_id).collect::<Vec<_>>(),
        expected[..2]
    );
    assert!(first_page.previous.is_none());
    let second = store
        .agenda(
            timed_query("2026-11-11"),
            PageRequest {
                size: PageSize::new(2).unwrap(),
                position: PagePosition::After(first_page.next.unwrap()),
            },
        )
        .unwrap();
    assert_eq!(
        second.items.iter().map(agenda_id).collect::<Vec<_>>(),
        expected[2..4]
    );
    assert_eq!(
        (second.total, second.generation),
        (6, first_page.generation)
    );
    let back = store
        .agenda(
            timed_query("2026-11-11"),
            PageRequest {
                size: PageSize::new(2).unwrap(),
                position: PagePosition::Before(second.previous.unwrap()),
            },
        )
        .unwrap();
    assert_eq!(
        back.items.iter().map(agenda_id).collect::<Vec<_>>(),
        expected[..2]
    );
    let last = store
        .agenda(
            timed_query("2026-11-11"),
            PageRequest {
                size: PageSize::new(2).unwrap(),
                position: PagePosition::After(second.next.unwrap()),
            },
        )
        .unwrap();
    assert_eq!(
        last.items.iter().map(agenda_id).collect::<Vec<_>>(),
        expected[4..]
    );
    assert!(last.next.is_none());
    assert_eq!(last.total, 6);
}

#[test]
fn pc8_agenda_calendar_filter_and_cursor_scope_cover_range_calendar_database_and_generation() {
    let db = Database::new();
    let mut store = db.open();
    let alternate = create_calendar(&mut store, "別のカレンダー");
    let a = create_event(
        &mut store,
        "既定の予定",
        EventWhen::Timed(span(
            "2026-11-11T09:00:00+09:00",
            "2026-11-11T10:00:00+09:00",
        )),
    );
    let b = store
        .save_event(Save::Create {
            id: EventId::new().unwrap(),
            data: EventData {
                title: Title::new("別の予定").unwrap(),
                calendar: alternate.key.id,
                when: EventWhen::Timed(span(
                    "2026-11-11T11:00:00+09:00",
                    "2026-11-11T12:00:00+09:00",
                )),
            },
        })
        .unwrap()
        .value;
    let mut filtered = timed_query("2026-11-11");
    filtered.calendar = Some(alternate.key.id);
    assert_eq!(
        store.agenda(filtered.clone(), first(10)).unwrap().items,
        vec![AgendaItem::Event(b)]
    );
    let cursor = store
        .agenda(timed_query("2026-11-11"), first(1))
        .unwrap()
        .next
        .unwrap();
    for mismatch in [
        filtered,
        timed_query("2026-11-12"),
        AgendaQuery {
            range: AgendaRange::AllDay(query().day.days()),
            calendar: None,
        },
        AgendaQuery {
            range: AgendaRange::Timed(
                query()
                    .day
                    .clip(
                        instant("2026-11-11T08:00:00+09:00"),
                        instant("2026-11-11T13:00:00+09:00"),
                    )
                    .unwrap(),
            ),
            calendar: None,
        },
    ] {
        assert!(matches!(
            store.agenda(
                mismatch,
                PageRequest {
                    size: PageSize::new(1).unwrap(),
                    position: PagePosition::After(cursor.clone())
                }
            ),
            Err(Error::CursorMismatch)
        ));
    }
    let other_db = Database::new();
    let mut other = other_db.open();
    assert!(matches!(
        other.agenda(
            timed_query("2026-11-11"),
            PageRequest {
                size: PageSize::new(1).unwrap(),
                position: PagePosition::After(cursor.clone())
            }
        ),
        Err(Error::CursorMismatch)
    ));
    let mut edit = a.data.clone();
    edit.title = Title::new("予定を編集").unwrap();
    store
        .save_event(Save::Update {
            expected: a.key,
            data: edit,
        })
        .unwrap();
    assert!(matches!(
        store.agenda(
            timed_query("2026-11-11"),
            PageRequest {
                size: PageSize::new(1).unwrap(),
                position: PagePosition::After(cursor)
            }
        ),
        Err(Error::StalePage)
    ));
}

#[test]
fn pc8_collections_page_by_kind_then_id_and_reject_old_or_foreign_cursors() {
    let db = Database::new();
    let mut store = db.open();
    let mut lists = vec![store.info().default_list];
    let mut calendars = vec![store.info().default_calendar];
    for label in ["B", "A"] {
        lists.push(create_list(&mut store, label).key.id);
        calendars.push(create_calendar(&mut store, label).key.id);
    }
    lists.sort();
    calendars.sort();
    let expected: Vec<_> = lists
        .into_iter()
        .map(EntityId::List)
        .chain(calendars.into_iter().map(EntityId::Calendar))
        .collect();
    let ids = |items: &[Collection]| {
        items
            .iter()
            .map(|c| match c {
                Collection::List(v) => EntityId::List(v.key.id),
                Collection::Calendar(v) => EntityId::Calendar(v.key.id),
            })
            .collect::<Vec<_>>()
    };
    let first_page = store.collections(first(2)).unwrap();
    assert_eq!(first_page.total, 6);
    assert_eq!(ids(&first_page.items), expected[..2]);
    assert!(first_page.previous.is_none());
    let cursor = first_page.next.unwrap();
    let middle = store
        .collections(PageRequest {
            size: PageSize::new(2).unwrap(),
            position: PagePosition::After(cursor.clone()),
        })
        .unwrap();
    assert_eq!(ids(&middle.items), expected[2..4]);
    let back = store
        .collections(PageRequest {
            size: PageSize::new(2).unwrap(),
            position: PagePosition::Before(middle.previous.unwrap()),
        })
        .unwrap();
    assert_eq!(ids(&back.items), expected[..2]);
    let last = store
        .collections(PageRequest {
            size: PageSize::new(2).unwrap(),
            position: PagePosition::After(middle.next.unwrap()),
        })
        .unwrap();
    assert_eq!(ids(&last.items), expected[4..]);
    assert_eq!((last.total, last.generation), (6, first_page.generation));
    assert!(last.next.is_none());
    let other_db = Database::new();
    let mut other = other_db.open();
    assert!(matches!(
        other.collections(PageRequest {
            size: PageSize::new(2).unwrap(),
            position: PagePosition::After(cursor.clone())
        }),
        Err(Error::CursorMismatch)
    ));
    create_task(&mut store, "所属先以外の更新も世代を進める");
    assert!(matches!(
        store.collections(PageRequest {
            size: PageSize::new(2).unwrap(),
            position: PagePosition::After(cursor)
        }),
        Err(Error::StalePage)
    ));
}
