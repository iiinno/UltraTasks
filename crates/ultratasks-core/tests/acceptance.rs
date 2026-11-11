#![cfg(feature = "test-support")]

mod common;

use common::*;
use ultratasks_core::{test_support::*, *};

#[test]
fn pc2_two_blocks_share_task_title_status_and_due_without_changing_their_times() {
    let db = Database::new();
    let mut store = db.open();
    let original = create_task(&mut store, "作業前");
    let a = create_block(
        &mut store,
        &original,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    let b = create_block(
        &mut store,
        &a.task,
        "2026-11-11T14:00:00+09:00",
        "2026-11-11T15:30:00+09:00",
    );
    assert!(a.task.key.revision > original.key.revision);
    assert!(b.task.key.revision > a.task.key.revision);
    let event = create_event(
        &mut store,
        "独立した予定",
        EventWhen::Timed(span(
            "2026-11-11T12:00:00+09:00",
            "2026-11-11T13:00:00+09:00",
        )),
    );
    let mut edited = b.task.data.clone();
    edited.title = Title::new("日本語の作業を完了").unwrap();
    edited.status = TaskStatus::Done;
    edited.due = Some(Due::Date(date("2026-11-12")));
    let saved = store
        .save_task(Save::Update {
            expected: b.task.key,
            data: edited.clone(),
        })
        .unwrap();
    assert_eq!(
        saved.changed,
        vec![Change::Upsert(Entity::Task(saved.value.clone()))]
    );
    let detail = store.task_detail(original.key.id).unwrap();
    assert_eq!(detail.task.data, edited);
    assert_eq!(detail.blocks, vec![a.block.clone(), b.block.clone()]);
    let rows = store
        .tasks(query(), request(10, TaskPosition::First))
        .unwrap();
    assert_eq!((rows.total, rows.items.len()), (1, 1));
    assert_eq!(rows.items[0].blocks.total_for_task, 2);
    assert_eq!(
        rows.items[0].blocks.allocated_micros_in_window,
        9_000_000_000
    );
    let agenda = store.agenda(timed_query("2026-11-11"), first(10)).unwrap();
    assert_eq!(agenda.total, 3);
    let summaries: Vec<_> = agenda
        .items
        .iter()
        .filter_map(|item| match item {
            AgendaItem::TimeBlock { task, .. } => Some(task),
            AgendaItem::Event(_) => None,
        })
        .collect();
    assert_eq!(summaries.len(), 2);
    for summary in summaries {
        assert_eq!(summary.key, saved.value.key);
        assert_eq!(summary.title, edited.title);
        assert_eq!(summary.status, TaskStatus::Done);
    }
    assert_eq!(
        store.get(EntityId::Event(event.key.id)).unwrap(),
        Entity::Event(event)
    );
}

#[test]
fn pc3_collection_crud_protects_defaults_and_every_live_reference() {
    let db = Database::new();
    let mut store = db.open();
    let defaults = store.collections(first(10)).unwrap();
    assert_eq!(defaults.total, 2);
    assert_eq!(
        store.history().unwrap(),
        HistoryState {
            undo: None,
            redo: None
        }
    );
    for collection in defaults.items {
        let (id, target, expected_title) = match collection {
            Collection::List(list) => (
                EntityId::List(list.key.id),
                DeleteTarget::List(list.key),
                "タスク",
            ),
            Collection::Calendar(calendar) => (
                EntityId::Calendar(calendar.key.id),
                DeleteTarget::Calendar(calendar.key),
                "個人",
            ),
        };
        let entity = store.get(id).unwrap();
        match entity {
            Entity::List(ref list) => assert_eq!(list.data.as_str(), expected_title),
            Entity::Calendar(ref calendar) => assert_eq!(calendar.data.as_str(), expected_title),
            _ => panic!("所属先以外が返った"),
        }
        let before = snapshot(&mut store, &[id]);
        assert!(
            matches!(store.delete(target), Err(Error::ProtectedCollection(found)) if found == id)
        );
        assert_eq!(snapshot(&mut store, &[id]), before);
    }
    let list = create_list(&mut store, "研究");
    let calendar = create_calendar(&mut store, "仕事");
    let Collection::List(renamed_list) = store
        .save_collection(CollectionSave::List(Save::Update {
            expected: list.key,
            data: Title::new("研究資料").unwrap(),
        }))
        .unwrap()
        .value
    else {
        panic!("List以外が返った")
    };
    let Collection::Calendar(renamed_calendar) = store
        .save_collection(CollectionSave::Calendar(Save::Update {
            expected: calendar.key,
            data: Title::new("仕事の予定").unwrap(),
        }))
        .unwrap()
        .value
    else {
        panic!("Calendar以外が返った")
    };
    assert!(renamed_list.key.revision > list.key.revision);
    assert!(renamed_calendar.key.revision > calendar.key.revision);
    let mut task_data = data(&store, "研究する");
    task_data.list = list.key.id;
    let task = store
        .save_task(Save::Create {
            id: TaskId::new().unwrap(),
            data: task_data,
        })
        .unwrap()
        .value;
    let event = store
        .save_event(Save::Create {
            id: EventId::new().unwrap(),
            data: EventData {
                title: Title::new("研究会").unwrap(),
                calendar: calendar.key.id,
                when: EventWhen::AllDay(days("2026-11-11", "2026-11-12")),
            },
        })
        .unwrap()
        .value;
    let block = store
        .save_time_block(
            task.key,
            Save::Create {
                id: TimeBlockId::new().unwrap(),
                data: BlockData {
                    calendar: calendar.key.id,
                    span: span("2026-11-11T10:00:00+09:00", "2026-11-11T11:00:00+09:00"),
                },
            },
        )
        .unwrap()
        .value;
    let ids = [
        EntityId::List(list.key.id),
        EntityId::Calendar(calendar.key.id),
        EntityId::Task(task.key.id),
        EntityId::Event(event.key.id),
        EntityId::TimeBlock(block.block.key.id),
    ];
    let before = snapshot(&mut store, &ids);
    assert!(
        matches!(store.delete(DeleteTarget::List(renamed_list.key)), Err(Error::NotEmpty(EntityId::List(id))) if id == list.key.id)
    );
    assert!(
        matches!(store.delete(DeleteTarget::Calendar(renamed_calendar.key)), Err(Error::NotEmpty(EntityId::Calendar(id))) if id == calendar.key.id)
    );
    assert_eq!(snapshot(&mut store, &ids), before);
    let rows = store
        .tasks(query(), request(10, TaskPosition::First))
        .unwrap();
    assert_eq!(rows.items[0].list_title.as_str(), "研究資料");
    let mut event_data = event.data.clone();
    event_data.title = Title::new("移動した研究会").unwrap();
    event_data.calendar = store.info().default_calendar;
    event_data.when = EventWhen::Timed(span(
        "2026-11-12T10:00:00+09:00",
        "2026-11-12T11:00:00+09:00",
    ));
    let moved = store
        .save_event(Save::Update {
            expected: event.key,
            data: event_data.clone(),
        })
        .unwrap()
        .value;
    assert_eq!(
        store.get(EntityId::Event(event.key.id)).unwrap(),
        Entity::Event(moved.clone())
    );
    assert_eq!(moved.data, event_data);
    assert!(matches!(
        store.delete(DeleteTarget::Calendar(renamed_calendar.key)),
        Err(Error::NotEmpty(_))
    ));
    store
        .delete(DeleteTarget::TimeBlock {
            task: block.task.key,
            block: block.block.key,
        })
        .unwrap();
    let current = common::task(&mut store, task.key.id);
    assert_eq!(current.data, task.data);
    assert!(current.key.revision > block.task.key.revision);
    assert!(store.task_detail(task.key.id).unwrap().blocks.is_empty());
    store
        .delete(DeleteTarget::Calendar(renamed_calendar.key))
        .unwrap();
    store.delete(DeleteTarget::Task(current.key)).unwrap();
    store.delete(DeleteTarget::List(renamed_list.key)).unwrap();
    store.delete(DeleteTarget::Event(moved.key)).unwrap();
    for id in ids {
        assert!(matches!(store.get(id), Err(Error::NotFound(found)) if found == id));
    }
    assert_eq!(store.collections(first(10)).unwrap().total, 2);
}

#[test]
fn pc3_stale_parent_stale_child_and_different_parent_leave_the_aggregate_unchanged() {
    let db = Database::new();
    let mut store = db.open();
    let original = create_task(&mut store, "親");
    let other = create_task(&mut store, "別の親");
    let first_block = create_block(
        &mut store,
        &original,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    let moved_data = block_data(
        &store,
        "2026-11-11T11:00:00+09:00",
        "2026-11-11T12:00:00+09:00",
    );
    let moved = store
        .save_time_block(
            first_block.task.key,
            Save::Update {
                expected: first_block.block.key,
                data: moved_data.clone(),
            },
        )
        .unwrap()
        .value;
    assert!(moved.task.key.revision > first_block.task.key.revision);
    assert!(moved.block.key.revision > first_block.block.key.revision);
    let unused_id = TimeBlockId::new().unwrap();
    let ids = [
        EntityId::Task(original.key.id),
        EntityId::Task(other.key.id),
        EntityId::TimeBlock(moved.block.key.id),
        EntityId::TimeBlock(unused_id),
    ];
    let before = snapshot(&mut store, &ids);
    for edit in [
        Save::Create {
            id: unused_id,
            data: moved_data.clone(),
        },
        Save::Update {
            expected: moved.block.key,
            data: moved_data.clone(),
        },
    ] {
        assert!(
            matches!(store.save_time_block(first_block.task.key, edit), Err(Error::Conflict { entity: EntityId::Task(id), expected, current: CurrentVersion::Live(actual) }) if id == original.key.id && expected == first_block.task.key.revision && actual == moved.task.key.revision)
        );
    }
    assert!(
        matches!(store.delete(DeleteTarget::TimeBlock { task: first_block.task.key, block: moved.block.key }), Err(Error::Conflict { entity: EntityId::Task(id), .. }) if id == original.key.id)
    );
    assert!(
        matches!(store.save_time_block(moved.task.key, Save::Update { expected: first_block.block.key, data: moved_data.clone() }), Err(Error::Conflict { entity: EntityId::TimeBlock(id), .. }) if id == moved.block.key.id)
    );
    assert!(
        matches!(store.delete(DeleteTarget::TimeBlock { task: moved.task.key, block: first_block.block.key }), Err(Error::Conflict { entity: EntityId::TimeBlock(id), .. }) if id == moved.block.key.id)
    );
    assert!(
        matches!(store.save_time_block(other.key, Save::Update { expected: moved.block.key, data: moved_data }), Err(Error::TimeBlockTaskMismatch { block, expected, actual }) if block == moved.block.key.id && expected == other.key.id && actual == original.key.id)
    );
    assert!(matches!(
        store.delete(DeleteTarget::TimeBlock {
            task: other.key,
            block: moved.block.key
        }),
        Err(Error::TimeBlockTaskMismatch { .. })
    ));
    assert_eq!(snapshot(&mut store, &ids), before);
}

#[test]
fn pc3_missing_references_reject_the_whole_write_and_allow_a_corrected_retry() {
    let db = Database::new();
    let mut store = db.open();
    let parent = create_task(&mut store, "親");
    let task_id = TaskId::new().unwrap();
    let event_id = EventId::new().unwrap();
    let block_id = TimeBlockId::new().unwrap();
    let list_id = ListId::new().unwrap();
    let calendar_id = CalendarId::new().unwrap();
    let ids = [
        EntityId::Task(parent.key.id),
        EntityId::Task(task_id),
        EntityId::Event(event_id),
        EntityId::TimeBlock(block_id),
    ];
    let before = snapshot(&mut store, &ids);
    let mut bad_task = data(&store, "参照がない");
    bad_task.list = list_id;
    assert!(
        matches!(store.save_task(Save::Create { id: task_id, data: bad_task }), Err(Error::NotFound(EntityId::List(id))) if id == list_id)
    );
    let when = EventWhen::AllDay(days("2026-11-11", "2026-11-12"));
    assert!(
        matches!(store.save_event(Save::Create { id: event_id, data: EventData { title: Title::new("予定").unwrap(), calendar: calendar_id, when } }), Err(Error::NotFound(EntityId::Calendar(id))) if id == calendar_id)
    );
    let mut block = block_data(
        &store,
        "2026-11-11T10:00:00+09:00",
        "2026-11-11T11:00:00+09:00",
    );
    block.calendar = calendar_id;
    assert!(
        matches!(store.save_time_block(parent.key, Save::Create { id: block_id, data: block.clone() }), Err(Error::NotFound(EntityId::Calendar(id))) if id == calendar_id)
    );
    assert_eq!(snapshot(&mut store, &ids), before);
    block.calendar = store.info().default_calendar;
    let saved = store
        .save_time_block(
            parent.key,
            Save::Create {
                id: block_id,
                data: block,
            },
        )
        .unwrap();
    assert_eq!(saved.value.block.key.id, block_id);
    assert_eq!(store.task_detail(parent.key.id).unwrap().blocks.len(), 1);
}

#[test]
fn pc5_precommit_failures_roll_back_creates_edits_blocks_events_and_cascade_delete() {
    for point in PRECOMMIT_FAILURES {
        let db = Database::new();
        let mut store = db.open();
        let original = create_task(&mut store, "保存済み");
        let a = create_block(
            &mut store,
            &original,
            "2026-11-11T09:00:00+09:00",
            "2026-11-11T10:00:00+09:00",
        );
        let b = create_block(
            &mut store,
            &a.task,
            "2026-11-11T11:00:00+09:00",
            "2026-11-11T12:00:00+09:00",
        );
        let event = create_event(
            &mut store,
            "既存予定",
            EventWhen::AllDay(days("2026-11-11", "2026-11-12")),
        );
        let new_task = TaskId::new().unwrap();
        let new_block = TimeBlockId::new().unwrap();
        let new_event = EventId::new().unwrap();
        let new_list = ListId::new().unwrap();
        let new_calendar = CalendarId::new().unwrap();
        let ids = [
            EntityId::Task(original.key.id),
            EntityId::TimeBlock(a.block.key.id),
            EntityId::TimeBlock(b.block.key.id),
            EntityId::Event(event.key.id),
            EntityId::Task(new_task),
            EntityId::TimeBlock(new_block),
            EntityId::Event(new_event),
            EntityId::List(new_list),
            EntityId::Calendar(new_calendar),
        ];
        let new_data = data(&store, "未確定の追加");
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_task(Save::Create {
                id: new_task,
                data: new_data.clone(),
            })
        });
        let mut edited = b.task.data.clone();
        edited.status = TaskStatus::Done;
        edited.due = Some(Due::Date(date("2026-11-12")));
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_task(Save::Update {
                expected: b.task.key,
                data: edited,
            })
        });
        let moved = block_data(
            &store,
            "2026-11-11T13:00:00+09:00",
            "2026-11-11T14:00:00+09:00",
        );
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_time_block(
                b.task.key,
                Save::Create {
                    id: new_block,
                    data: moved.clone(),
                },
            )
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_time_block(
                b.task.key,
                Save::Update {
                    expected: a.block.key,
                    data: moved,
                },
            )
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.delete(DeleteTarget::TimeBlock {
                task: b.task.key,
                block: a.block.key,
            })
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.delete(DeleteTarget::Task(b.task.key))
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_event(Save::Create {
                id: new_event,
                data: event.data.clone(),
            })
        });
        let mut edit_event = event.data.clone();
        edit_event.title = Title::new("予定の編集").unwrap();
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_event(Save::Update {
                expected: event.key,
                data: edit_event,
            })
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.delete(DeleteTarget::Event(event.key))
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_collection(CollectionSave::List(Save::Create {
                id: new_list,
                data: Title::new("新規リスト").unwrap(),
            }))
        });
        assert_rollback(&mut store, &ids, point, |s| {
            s.save_collection(CollectionSave::Calendar(Save::Create {
                id: new_calendar,
                data: Title::new("新規カレンダー").unwrap(),
            }))
        });
        let before = snapshot(&mut store, &ids);
        drop(store);
        let mut reopened = db.open();
        assert_eq!(snapshot(&mut reopened, &ids), before, "再open: {point:?}");
        reopened
            .save_task(Save::Create {
                id: new_task,
                data: new_data,
            })
            .unwrap();
        assert_eq!(inspect(&mut reopened).unwrap().live_tasks, 2);
    }
}

#[test]
fn pc5_unknown_create_blocks_all_database_operations_until_reopen_without_duplicate_creation() {
    let db = Database::new();
    let mut store = db.open();
    let baseline = create_task(&mut store, "先に保存");
    let mut changed = baseline.data.clone();
    changed.title = Title::new("Undoしておく編集").unwrap();
    let edited = store
        .save_task(Save::Update {
            expected: baseline.key,
            data: changed,
        })
        .unwrap();
    let undone = store.undo(edited.history.undo.unwrap().target).unwrap();
    let target = undone.history.undo.unwrap().target;
    let redo_target = undone.history.redo.unwrap().target;
    let baseline = task(&mut store, baseline.key.id);
    let id = TaskId::new().unwrap();
    let input = data(&store, "応答を失った追加");
    fail_next(&mut store, FailPoint::AfterCommitBeforeReply);
    assert!(matches!(
        store.save_task(Save::Create {
            id,
            data: input.clone()
        }),
        Err(Error::Storage {
            outcome: CommitKnowledge::Unknown,
            ..
        })
    ));
    assert!(matches!(
        store.get(EntityId::Task(id)),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.task_detail(id),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.tasks(query(), request(10, TaskPosition::First)),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.task_rows(query(), &[id]),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.agenda(timed_query("2026-11-11"), first(10)),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.collections(first(10)),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(store.history(), Err(Error::RecoveryRequired)));
    assert!(matches!(inspect(&mut store), Err(Error::RecoveryRequired)));
    assert!(matches!(store.undo(target), Err(Error::RecoveryRequired)));
    assert!(matches!(
        store.redo(redo_target),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.save_event(Save::Create {
            id: EventId::new().unwrap(),
            data: EventData {
                title: Title::new("停止中の予定").unwrap(),
                calendar: store.info().default_calendar,
                when: EventWhen::AllDay(days("2026-11-11", "2026-11-12"))
            },
        }),
        Err(Error::RecoveryRequired)
    ));
    let blocked_data = block_data(
        &store,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    assert!(matches!(
        store.save_time_block(
            baseline.key,
            Save::Create {
                id: TimeBlockId::new().unwrap(),
                data: blocked_data
            }
        ),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.save_collection(CollectionSave::List(Save::Create {
            id: ListId::new().unwrap(),
            data: Title::new("停止中の所属先").unwrap()
        })),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.save_task(Save::Create {
            id,
            data: input.clone()
        }),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        store.delete(DeleteTarget::Task(baseline.key)),
        Err(Error::RecoveryRequired)
    ));
    drop(store);
    let mut store = db.open();
    let recovered = task(&mut store, id);
    assert_eq!(recovered.data, input);
    assert_eq!(inspect(&mut store).unwrap().live_tasks, 2);
    let before = snapshot(
        &mut store,
        &[EntityId::Task(id), EntityId::Task(baseline.key.id)],
    );
    assert!(
        matches!(store.save_task(Save::Create { id, data: input }), Err(Error::AlreadyExists(EntityId::Task(found))) if found == id)
    );
    assert_eq!(
        snapshot(
            &mut store,
            &[EntityId::Task(id), EntityId::Task(baseline.key.id)]
        ),
        before
    );
    let undo = store.history().unwrap().undo.unwrap();
    assert_eq!(undo.focus, EntityId::Task(id));
    assert_eq!(undo.kind, OperationKind::TaskCreated);
    store.undo(undo.target).unwrap();
    assert!(matches!(
        store.get(EntityId::Task(id)),
        Err(Error::NotFound(_))
    ));
    assert_eq!(task(&mut store, baseline.key.id), baseline);
}

#[test]
fn pc5_unknown_edit_and_cascade_delete_are_recovered_by_reading_current_state() {
    for delete in [false, true] {
        let db = Database::new();
        let mut store = db.open();
        let original = create_task(&mut store, "元の内容");
        let a = create_block(
            &mut store,
            &original,
            "2026-11-11T09:00:00+09:00",
            "2026-11-11T10:00:00+09:00",
        );
        let b = create_block(
            &mut store,
            &a.task,
            "2026-11-11T11:00:00+09:00",
            "2026-11-11T12:00:00+09:00",
        );
        let mut edited = b.task.data.clone();
        edited.title = Title::new("保存された下書き").unwrap();
        fail_next(&mut store, FailPoint::AfterCommitBeforeReply);
        let error = if delete {
            store.delete(DeleteTarget::Task(b.task.key)).unwrap_err()
        } else {
            store
                .save_task(Save::Update {
                    expected: b.task.key,
                    data: edited.clone(),
                })
                .unwrap_err()
        };
        assert!(matches!(
            error,
            Error::Storage {
                outcome: CommitKnowledge::Unknown,
                ..
            }
        ));
        drop(store);
        let mut store = db.open();
        if delete {
            for id in [
                EntityId::Task(original.key.id),
                EntityId::TimeBlock(a.block.key.id),
                EntityId::TimeBlock(b.block.key.id),
            ] {
                assert!(matches!(store.get(id), Err(Error::NotFound(found)) if found == id));
            }
            let counts = inspect(&mut store).unwrap();
            assert_eq!((counts.live_tasks, counts.live_blocks), (0, 0));
        } else {
            let detail = store.task_detail(original.key.id).unwrap();
            assert_eq!(detail.task.data, edited);
            assert!(detail.task.key.revision > b.task.key.revision);
            assert_eq!(detail.blocks, vec![a.block.clone(), b.block.clone()]);
            assert!(matches!(
                store.save_task(Save::Update {
                    expected: b.task.key,
                    data: edited
                }),
                Err(Error::Conflict { .. })
            ));
        }
        let target = store.history().unwrap().undo.unwrap().target;
        store.undo(target).unwrap();
        let detail = store.task_detail(original.key.id).unwrap();
        assert_eq!(detail.task.data, original.data);
        assert_eq!(detail.blocks.len(), 2);
        assert_eq!(detail.blocks[0].key.id, a.block.key.id);
        assert_eq!(detail.blocks[1].key.id, b.block.key.id);
    }
}

#[test]
fn pc6_block_limit_allows_128_and_rejects_129_without_consuming_the_id() {
    let db = Database::new();
    let mut store = db.open();
    let mut parent = create_task(&mut store, "128枠の集約");
    let block_data = block_data(
        &store,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    for _ in 0..128 {
        parent = store
            .save_time_block(
                parent.key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: block_data.clone(),
                },
            )
            .unwrap()
            .value
            .task;
    }
    let before_detail = store.task_detail(parent.key.id).unwrap();
    assert_eq!(before_detail.blocks.len(), 128);
    let unused = TimeBlockId::new().unwrap();
    let mut ids: Vec<_> = before_detail
        .blocks
        .iter()
        .map(|b| EntityId::TimeBlock(b.key.id))
        .collect();
    ids.extend([EntityId::Task(parent.key.id), EntityId::TimeBlock(unused)]);
    let before = snapshot(&mut store, &ids);
    assert!(
        matches!(store.save_time_block(parent.key, Save::Create { id: unused, data: block_data.clone() }), Err(Error::LimitExceeded { kind: LimitKind::BlocksPerTask, maximum: 128, observed_at_least }) if observed_at_least >= 129)
    );
    assert_eq!(snapshot(&mut store, &ids), before);
    for point in PRECOMMIT_FAILURES {
        assert_rollback(&mut store, &ids, point, |s| {
            s.delete(DeleteTarget::Task(parent.key))
        });
    }
    let deleted = store.delete(DeleteTarget::Task(parent.key)).unwrap();
    assert_eq!(deleted.changed.len(), 129);
    assert_eq!(deleted.history.undo.as_ref().unwrap().affected, 129);
    assert_eq!(
        (
            inspect(&mut store).unwrap().live_tasks,
            inspect(&mut store).unwrap().live_blocks
        ),
        (0, 0)
    );
    store.undo(deleted.history.undo.unwrap().target).unwrap();
    let restored = store.task_detail(parent.key.id).unwrap();
    assert_eq!(restored.task.data, before_detail.task.data);
    assert!(restored.task.key.revision > before_detail.task.key.revision);
    assert_eq!(restored.blocks.len(), 128);
    for (new, old) in restored.blocks.iter().zip(&before_detail.blocks) {
        assert_eq!(
            (new.key.id, new.task, &new.data),
            (old.key.id, old.task, &old.data)
        );
        assert!(new.key.revision > old.key.revision);
    }
    store
        .delete(DeleteTarget::TimeBlock {
            task: restored.task.key,
            block: restored.blocks[0].key,
        })
        .unwrap();
    let parent = task(&mut store, parent.key.id);
    store
        .save_time_block(
            parent.key,
            Save::Create {
                id: unused,
                data: block_data,
            },
        )
        .unwrap();
    assert_eq!(store.task_detail(parent.key.id).unwrap().blocks.len(), 128);
}

#[test]
fn pc7_reopen_preserves_ids_collections_relations_times_and_both_history_directions() {
    let db = Database::new();
    let mut store = db.open();
    let info = store.info().clone();
    let list = create_list(&mut store, "移動先");
    let calendar = create_calendar(&mut store, "別カレンダー");
    let original = create_task(&mut store, "日を跨ぐ作業");
    let block = create_block(
        &mut store,
        &original,
        "2026-11-11T23:00:00+09:00",
        "2026-11-12T01:00:00+09:00",
    );
    let mut moved_data = block.task.data.clone();
    moved_data.list = list.key.id;
    moved_data.due = Some(Due::At(ZonedInstant {
        instant: instant("2026-11-12T01:00:00.123456+09:00"),
        zone: zone("Asia/Tokyo"),
    }));
    store
        .save_task(Save::Update {
            expected: block.task.key,
            data: moved_data,
        })
        .unwrap();
    let event = store
        .save_event(Save::Create {
            id: EventId::new().unwrap(),
            data: EventData {
                title: Title::new("終日・月跨ぎ").unwrap(),
                calendar: calendar.key.id,
                when: EventWhen::AllDay(days("2026-11-30", "2026-12-02")),
            },
        })
        .unwrap()
        .value;
    let current = task(&mut store, original.key.id);
    let mut edited = current.data.clone();
    edited.status = TaskStatus::Done;
    let completed = store
        .save_task(Save::Update {
            expected: current.key,
            data: edited,
        })
        .unwrap();
    store.undo(completed.history.undo.unwrap().target).unwrap();
    let ids = [
        EntityId::Task(original.key.id),
        EntityId::TimeBlock(block.block.key.id),
        EntityId::Event(event.key.id),
        EntityId::List(list.key.id),
        EntityId::Calendar(calendar.key.id),
    ];
    let before = snapshot(&mut store, &ids);
    let history = store.history().unwrap();
    assert!(history.undo.is_some() && history.redo.is_some());
    drop(store);
    let mut store = db.open();
    assert_eq!(store.info().database, info.database);
    assert_eq!(store.info().actor, info.actor);
    assert_eq!(store.info().default_list, info.default_list);
    assert_eq!(store.info().default_calendar, info.default_calendar);
    assert_eq!(snapshot(&mut store, &ids), before);
    store.redo(history.redo.unwrap().target).unwrap();
    assert_eq!(
        task(&mut store, original.key.id).data.status,
        TaskStatus::Done
    );
    assert_eq!(
        store.get(EntityId::Event(event.key.id)).unwrap(),
        Entity::Event(event)
    );
}

#[test]
fn pc7_double_open_is_rejected_in_this_process_and_a_child_then_released_on_close() {
    let db = Database::new();
    let mut store = db.open();
    let saved = create_task(&mut store, "ロック中のデータ");
    assert!(matches!(Store::open(&db.path), Err(Error::AlreadyOpen)));
    let alias = db
        .path
        .parent()
        .unwrap()
        .join(".")
        .join("acceptance.sqlite");
    assert!(matches!(Store::open(&alias), Err(Error::AlreadyOpen)));
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "pc7_lock_probe_child",
            "--nocapture",
        ])
        .env("ULTRATASKS_ACCEPTANCE_LOCK_PROBE", &db.path)
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "子プロセスの排他確認失敗: {}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("LOCK_PROBE_OK"));
    drop(store);
    let mut reopened = db.open();
    assert_eq!(task(&mut reopened, saved.key.id), saved);
}

#[test]
#[ignore = "pc7_double_openがロックを保持したまま別プロセスで起動する専用入口"]
fn pc7_lock_probe_child() {
    let path =
        std::env::var_os("ULTRATASKS_ACCEPTANCE_LOCK_PROBE").expect("親テストからDBパスを渡す");
    assert!(matches!(
        Store::open(std::path::Path::new(&path)),
        Err(Error::AlreadyOpen)
    ));
    println!("LOCK_PROBE_OK");
}

#[test]
#[cfg(feature = "fixtures")]
fn fixture_profiles_and_distributions_have_exact_persisted_counts_and_refuse_reseed() {
    use ultratasks_core::fixtures::*;
    for (profile, expected) in [
        (Profile::Small, (5, 3, 2)),
        (Profile::Thousand, (1000, 100, 1000)),
        (Profile::TenThousand, (10000, 1000, 10000)),
    ] {
        for distribution in [Distribution::ThirtyDays, Distribution::DenseDay] {
            let db = Database::new();
            let config = Config {
                profile,
                distribution,
                seed: 42,
                day: date("2026-11-11"),
                zone: zone("Asia/Tokyo"),
            };
            let report = seed_empty(&db.path, config.clone()).unwrap();
            assert_eq!(
                (report.tasks, report.events, report.time_blocks),
                expected,
                "{profile:?}/{distribution:?}"
            );
            assert_eq!(report.seed, 42);
            let mut store = db.open();
            let counts = inspect(&mut store).unwrap();
            assert_eq!(
                (counts.live_tasks, counts.live_blocks),
                (expected.0, expected.2)
            );
            let page = store
                .tasks(query(), request(200, TaskPosition::First))
                .unwrap();
            assert_eq!(page.total, expected.0);
            assert_eq!(page.items.len() as u64, expected.0.min(200));
            assert_eq!(page.next.is_some(), expected.0 > 200);
            let range =
                CalendarWindow::new(days("2026-11-11", "2026-12-13"), zone("Asia/Tokyo")).unwrap();
            let timed = store
                .agenda(
                    AgendaQuery {
                        range: AgendaRange::Timed(range.timed()),
                        calendar: None,
                    },
                    first(1),
                )
                .unwrap();
            let all_day = store
                .agenda(
                    AgendaQuery {
                        range: AgendaRange::AllDay(range.days()),
                        calendar: None,
                    },
                    first(1),
                )
                .unwrap();
            assert_eq!(timed.total + all_day.total, expected.1 + expected.2);
            if distribution == Distribution::DenseDay {
                let today = store.agenda(timed_query("2026-11-11"), first(1)).unwrap();
                let today_all_day = store
                    .agenda(
                        AgendaQuery {
                            range: AgendaRange::AllDay(query().day.days()),
                            calendar: None,
                        },
                        first(1),
                    )
                    .unwrap();
                assert_eq!(today.total + today_all_day.total, expected.1 + expected.2);
            }
            let ids: Vec<_> = page
                .items
                .iter()
                .take(3)
                .map(|row| EntityId::Task(row.task.key.id))
                .collect();
            let before = snapshot(&mut store, &ids);
            drop(store);
            assert!(matches!(
                seed_empty(&db.path, config),
                Err(Error::FixtureRequiresEmptyDatabase)
            ));
            let mut store = db.open();
            assert_eq!(snapshot(&mut store, &ids), before);
            assert_eq!(
                store
                    .agenda(
                        AgendaQuery {
                            range: AgendaRange::Timed(range.timed()),
                            calendar: None
                        },
                        first(1)
                    )
                    .unwrap()
                    .total,
                timed.total
            );
            assert_eq!(
                store
                    .agenda(
                        AgendaQuery {
                            range: AgendaRange::AllDay(range.days()),
                            calendar: None
                        },
                        first(1)
                    )
                    .unwrap()
                    .total,
                all_day.total
            );
        }
    }
}

#[test]
#[cfg(feature = "fixtures")]
fn fixture_rejects_a_database_with_only_a_tombstone_and_keeps_its_history() {
    use ultratasks_core::fixtures::*;
    let db = Database::new();
    let mut store = db.open();
    let task = create_task(&mut store, "削除済みでも空DBではない");
    store.delete(DeleteTarget::Task(task.key)).unwrap();
    let ids = [EntityId::Task(task.key.id)];
    let before = snapshot(&mut store, &ids);
    drop(store);
    assert!(matches!(
        seed_empty(
            &db.path,
            Config {
                profile: Profile::Small,
                distribution: Distribution::ThirtyDays,
                seed: 42,
                day: date("2026-11-11"),
                zone: zone("Asia/Tokyo")
            }
        ),
        Err(Error::FixtureRequiresEmptyDatabase)
    ));
    let mut store = db.open();
    assert_eq!(snapshot(&mut store, &ids), before);
    let target = store.history().unwrap().undo.unwrap().target;
    store.undo(target).unwrap();
    assert_eq!(common::task(&mut store, task.key.id).data, task.data);
}
