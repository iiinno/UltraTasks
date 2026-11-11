#![cfg(feature = "test-support")]

mod common;

use common::*;
use ultratasks_core::{test_support::*, *};

#[test]
fn pc4_three_edits_three_undos_three_redos_preserve_order_and_increase_physical_revisions() {
    let db = Database::new();
    let mut store = db.open();
    let original = create_task(&mut store, "初稿");
    let mut states = vec![original.data.clone()];
    let mut current = original;
    let mut previous_generation = store.task_rows(query(), &[]).unwrap().generation;
    for title in ["第一稿", "第二稿", "第三稿"] {
        let mut data = current.data.clone();
        data.title = Title::new(title).unwrap();
        let commit = store
            .save_task(Save::Update {
                expected: current.key,
                data: data.clone(),
            })
            .unwrap();
        assert!(commit.value.key.revision > current.key.revision);
        assert!(commit.generation > previous_generation);
        assert_eq!(
            commit.history.undo.as_ref().unwrap().kind,
            OperationKind::TaskEdited
        );
        assert_eq!(
            commit.history.undo.as_ref().unwrap().focus,
            EntityId::Task(current.key.id)
        );
        assert_eq!(commit.history.undo.as_ref().unwrap().affected, 1);
        states.push(data);
        current = commit.value;
        previous_generation = commit.generation;
    }
    let retained = inspect(&mut store).unwrap().retained_operations;
    assert_eq!(retained, 4);
    for expected in states[..3].iter().rev() {
        let target = store.history().unwrap().undo.unwrap().target;
        let commit = store.undo(target).unwrap();
        let restored = task(&mut store, current.key.id);
        assert_eq!(&restored.data, expected);
        assert!(restored.key.revision > current.key.revision);
        assert!(commit.generation > previous_generation);
        assert_eq!(
            commit.changed,
            vec![Change::Upsert(Entity::Task(restored.clone()))]
        );
        current = restored;
        previous_generation = commit.generation;
        assert_eq!(inspect(&mut store).unwrap().retained_operations, retained);
    }
    for expected in &states[1..] {
        let target = store.history().unwrap().redo.unwrap().target;
        let commit = store.redo(target).unwrap();
        let restored = task(&mut store, current.key.id);
        assert_eq!(&restored.data, expected);
        assert!(restored.key.revision > current.key.revision);
        assert!(commit.generation > previous_generation);
        current = restored;
        previous_generation = commit.generation;
        assert_eq!(inspect(&mut store).unwrap().retained_operations, retained);
    }
    assert!(store.history().unwrap().redo.is_none());
}

#[test]
fn pc4_old_targets_and_old_screen_revisions_do_not_overwrite_new_state() {
    let db = Database::new();
    let mut store = db.open();
    let original = create_task(&mut store, "保存済み");
    let old_undo = store.history().unwrap().undo.unwrap().target;
    let mut data = original.data.clone();
    data.title = Title::new("最新の編集").unwrap();
    let edited = store
        .save_task(Save::Update {
            expected: original.key,
            data,
        })
        .unwrap();
    let ids = [EntityId::Task(original.key.id)];
    let before = snapshot(&mut store, &ids);
    assert!(matches!(
        store.undo(old_undo),
        Err(Error::History(HistoryProblem::StaleTarget))
    ));
    assert!(
        matches!(store.save_task(Save::Update { expected: original.key, data: original.data.clone() }), Err(Error::Conflict { entity: EntityId::Task(id), expected, current: CurrentVersion::Live(actual) }) if id == original.key.id && expected == original.key.revision && actual == edited.value.key.revision)
    );
    assert!(matches!(
        store.delete(DeleteTarget::Task(original.key)),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(snapshot(&mut store, &ids), before);
    let undone = store.undo(edited.history.undo.unwrap().target).unwrap();
    let old_redo = undone.history.redo.unwrap().target;
    let current = task(&mut store, original.key.id);
    let mut data = current.data.clone();
    data.title = Title::new("別の編集を確定").unwrap();
    store
        .save_task(Save::Update {
            expected: current.key,
            data,
        })
        .unwrap();
    let before = snapshot(&mut store, &ids);
    assert!(matches!(
        store.redo(old_redo),
        Err(Error::History(
            HistoryProblem::Expired | HistoryProblem::StaleTarget
        ))
    ));
    assert_eq!(snapshot(&mut store, &ids), before);
    assert!(store.history().unwrap().redo.is_none());
}

#[test]
fn pc4_undo_and_redo_targets_cannot_cross_database_identity() {
    let first_db = Database::new();
    let mut first_store = first_db.open();
    let original = create_task(&mut first_store, "同じ内容");
    let undo = first_store.history().unwrap().undo.unwrap().target;
    let redo = first_store
        .undo(undo.clone())
        .unwrap()
        .history
        .redo
        .unwrap()
        .target;
    let second_db = Database::new();
    let mut second_store = second_db.open();
    let second = create_task(&mut second_store, "同じ内容");
    let ids = [EntityId::Task(second.key.id)];
    let before = snapshot(&mut second_store, &ids);
    assert!(matches!(
        second_store.undo(undo),
        Err(Error::History(HistoryProblem::WrongDatabase))
    ));
    assert!(matches!(
        second_store.redo(redo.clone()),
        Err(Error::History(HistoryProblem::WrongDatabase))
    ));
    assert_eq!(snapshot(&mut second_store, &ids), before);
    first_store.redo(redo).unwrap();
    assert_eq!(task(&mut first_store, original.key.id).data, original.data);
}

#[test]
fn pc4_other_actor_cannot_use_targets_and_intervening_edits_reject_the_whole_aggregate() {
    for redo in [false, true] {
        let db = Database::new();
        let actor_a = ActorId::new().unwrap();
        let actor_b = ActorId::new().unwrap();
        let at = "2026-11-11T00:00:00Z";
        let mut a = db.open_as(actor_a, at);
        let parent = create_task(&mut a, "元の親");
        let first_block = create_block(
            &mut a,
            &parent,
            "2026-11-11T09:00:00+09:00",
            "2026-11-11T10:00:00+09:00",
        );
        let second_block = create_block(
            &mut a,
            &first_block.task,
            "2026-11-11T11:00:00+09:00",
            "2026-11-11T12:00:00+09:00",
        );
        let target = a.history().unwrap().undo.unwrap().target;
        let redo_target = if redo {
            Some(a.undo(target.clone()).unwrap().history.redo.unwrap().target)
        } else {
            None
        };
        drop(a);
        let mut b = db.open_as(actor_b, at);
        let ids = [
            EntityId::Task(parent.key.id),
            EntityId::TimeBlock(first_block.block.key.id),
            EntityId::TimeBlock(second_block.block.key.id),
        ];
        let before = snapshot(&mut b, &ids);
        assert!(matches!(
            b.undo(target),
            Err(Error::History(HistoryProblem::NotOwn))
        ));
        if let Some(target) = redo_target {
            assert!(matches!(
                b.redo(target),
                Err(Error::History(HistoryProblem::NotOwn))
            ));
        }
        assert_eq!(snapshot(&mut b, &ids), before);
        let current = task(&mut b, parent.key.id);
        let moved = block_data(&b, "2026-11-11T15:00:00+09:00", "2026-11-11T16:00:00+09:00");
        b.save_time_block(
            current.key,
            Save::Update {
                expected: first_block.block.key,
                data: moved,
            },
        )
        .unwrap();
        drop(b);
        let mut a = db.open_as(actor_a, at);
        let before = snapshot(&mut a, &ids);
        let history = a.history().unwrap();
        let error = if redo {
            a.redo(history.redo.unwrap().target).unwrap_err()
        } else {
            a.undo(history.undo.unwrap().target).unwrap_err()
        };
        assert!(
            matches!(error, Error::HistoryConflict { entity: EntityId::Task(id) } if id == parent.key.id)
        );
        assert_eq!(snapshot(&mut a, &ids), before);
    }
}

#[test]
fn pc4_other_actors_unrelated_write_does_not_prevent_a_fresh_own_undo() {
    let db = Database::new();
    let actor_a = ActorId::new().unwrap();
    let actor_b = ActorId::new().unwrap();
    let mut a = db.open_as(actor_a, "2026-11-11T00:00:00Z");
    let own = create_task(&mut a, "Aの作業");
    drop(a);
    let mut b = db.open_as(actor_b, "2026-11-11T00:00:00Z");
    let other = create_task(&mut b, "Bの独立作業");
    drop(b);
    let mut a = db.open_as(actor_a, "2026-11-11T00:00:00Z");
    let target = a.history().unwrap().undo.unwrap().target;
    a.undo(target).unwrap();
    assert!(matches!(
        a.get(EntityId::Task(own.key.id)),
        Err(Error::NotFound(_))
    ));
    assert_eq!(task(&mut a, other.key.id), other);
}

#[test]
fn pc4_undo_collection_creation_cannot_orphan_another_actors_task_or_event() {
    for calendar_case in [false, true] {
        let db = Database::new();
        let actor_a = ActorId::new().unwrap();
        let actor_b = ActorId::new().unwrap();
        let mut a = db.open_as(actor_a, "2026-11-11T00:00:00Z");
        let collection = if calendar_case {
            EntityId::Calendar(create_calendar(&mut a, "共有予定").key.id)
        } else {
            EntityId::List(create_list(&mut a, "共有作業").key.id)
        };
        drop(a);
        let mut b = db.open_as(actor_b, "2026-11-11T00:00:00Z");
        let dependent = match collection {
            EntityId::List(list) => {
                let mut data = data(&b, "参照中");
                data.list = list;
                let saved = b
                    .save_task(Save::Create {
                        id: TaskId::new().unwrap(),
                        data,
                    })
                    .unwrap();
                EntityId::Task(saved.value.key.id)
            }
            EntityId::Calendar(calendar) => {
                let saved = b
                    .save_event(Save::Create {
                        id: EventId::new().unwrap(),
                        data: EventData {
                            title: Title::new("参照中").unwrap(),
                            calendar,
                            when: EventWhen::AllDay(days("2026-11-11", "2026-11-12")),
                        },
                    })
                    .unwrap();
                EntityId::Event(saved.value.key.id)
            }
            _ => unreachable!(),
        };
        drop(b);
        let mut a = db.open_as(actor_a, "2026-11-11T00:00:00Z");
        let before = snapshot(&mut a, &[collection, dependent]);
        let target = a.history().unwrap().undo.unwrap().target;
        assert!(
            matches!(a.undo(target), Err(Error::HistoryConflict { entity }) if entity == collection)
        );
        assert_eq!(snapshot(&mut a, &[collection, dependent]), before);
    }
}

#[test]
fn pc3_pc4_cascade_delete_undo_redo_restores_all_relations_without_reusing_old_revisions() {
    let db = Database::new();
    let mut store = db.open();
    let parent = create_task(&mut store, "まとめて削除");
    let a = create_block(
        &mut store,
        &parent,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    let b = create_block(
        &mut store,
        &a.task,
        "2026-11-11T14:00:00+09:00",
        "2026-11-11T15:00:00+09:00",
    );
    let ids = [
        EntityId::Task(parent.key.id),
        EntityId::TimeBlock(a.block.key.id),
        EntityId::TimeBlock(b.block.key.id),
    ];
    let deleted = store.delete(DeleteTarget::Task(b.task.key)).unwrap();
    assert_eq!(deleted.changed.len(), 3);
    for id in ids {
        assert!(matches!(store.get(id), Err(Error::NotFound(found)) if found == id));
        assert!(
            deleted
                .changed
                .iter()
                .any(|change| matches!(change, Change::Deleted { id: found, .. } if *found == id))
        );
    }
    let undo = store.undo(deleted.history.undo.unwrap().target).unwrap();
    let restored = store.task_detail(parent.key.id).unwrap();
    assert_eq!(restored.task.data, b.task.data);
    assert_eq!(restored.blocks.len(), 2);
    assert!(restored.task.key.revision > b.task.key.revision);
    for (old, new) in [a.block, b.block].iter().zip(&restored.blocks) {
        assert_eq!(
            (old.key.id, old.task, &old.data),
            (new.key.id, new.task, &new.data)
        );
        assert!(new.key.revision > old.key.revision);
    }
    let before = snapshot(&mut store, &ids);
    assert!(matches!(
        store.save_task(Save::Update {
            expected: b.task.key,
            data: b.task.data
        }),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(snapshot(&mut store, &ids), before);
    store.redo(undo.history.redo.unwrap().target).unwrap();
    assert_eq!(
        (
            inspect(&mut store).unwrap().live_tasks,
            inspect(&mut store).unwrap().live_blocks
        ),
        (0, 0)
    );
}

#[test]
fn pc5_failed_undo_and_redo_preserve_all_rows_and_leave_the_same_target_retryable() {
    for point in PRECOMMIT_FAILURES {
        let db = Database::new();
        let mut store = db.open();
        let parent = create_task(&mut store, "Undoの原子性");
        let a = create_block(
            &mut store,
            &parent,
            "2026-11-11T09:00:00+09:00",
            "2026-11-11T10:00:00+09:00",
        );
        let b = create_block(
            &mut store,
            &a.task,
            "2026-11-11T11:00:00+09:00",
            "2026-11-11T12:00:00+09:00",
        );
        let ids = [
            EntityId::Task(parent.key.id),
            EntityId::TimeBlock(a.block.key.id),
            EntityId::TimeBlock(b.block.key.id),
        ];
        let deleted = store.delete(DeleteTarget::Task(b.task.key)).unwrap();
        let undo_target = deleted.history.undo.unwrap().target;
        assert_rollback(&mut store, &ids, point, |s| s.undo(undo_target.clone()));
        let undone = store.undo(undo_target).unwrap();
        assert_eq!(store.task_detail(parent.key.id).unwrap().blocks.len(), 2);
        let redo_target = undone.history.redo.unwrap().target;
        assert_rollback(&mut store, &ids, point, |s| s.redo(redo_target.clone()));
        store.redo(redo_target).unwrap();
        assert!(matches!(
            store.get(EntityId::Task(parent.key.id)),
            Err(Error::NotFound(_))
        ));
    }
}

#[test]
fn pc5_failed_new_edit_does_not_discard_the_existing_redo_branch() {
    for point in PRECOMMIT_FAILURES {
        let db = Database::new();
        let mut store = db.open();
        let original = create_task(&mut store, "元の内容");
        let mut edited = original.data.clone();
        edited.title = Title::new("Redoで戻す内容").unwrap();
        let saved = store
            .save_task(Save::Update {
                expected: original.key,
                data: edited.clone(),
            })
            .unwrap();
        let undone = store.undo(saved.history.undo.unwrap().target).unwrap();
        let current = task(&mut store, original.key.id);
        let mut competing = current.data.clone();
        competing.title = Title::new("失敗する別の編集").unwrap();
        assert_rollback(&mut store, &[EntityId::Task(original.key.id)], point, |s| {
            s.save_task(Save::Update {
                expected: current.key,
                data: competing,
            })
        });
        store.redo(undone.history.redo.unwrap().target).unwrap();
        assert_eq!(task(&mut store, original.key.id).data, edited);
    }
}

#[test]
fn pc5_unknown_undo_or_redo_reopens_at_the_committed_history_position() {
    for redo in [false, true] {
        let db = Database::new();
        let mut store = db.open();
        let original = create_task(&mut store, "元の内容");
        let mut data = original.data.clone();
        data.title = Title::new("編集後").unwrap();
        let edited = store
            .save_task(Save::Update {
                expected: original.key,
                data: data.clone(),
            })
            .unwrap();
        let target = edited.history.undo.unwrap().target;
        let redo_target = if redo {
            Some(
                store
                    .undo(target.clone())
                    .unwrap()
                    .history
                    .redo
                    .unwrap()
                    .target,
            )
        } else {
            None
        };
        let retained = inspect(&mut store).unwrap().retained_operations;
        fail_next(&mut store, FailPoint::AfterCommitBeforeReply);
        let error = if let Some(target) = redo_target {
            store.redo(target).unwrap_err()
        } else {
            store.undo(target).unwrap_err()
        };
        assert!(matches!(
            error,
            Error::Storage {
                outcome: CommitKnowledge::Unknown,
                ..
            }
        ));
        assert!(matches!(store.history(), Err(Error::RecoveryRequired)));
        drop(store);
        let mut store = db.open();
        let recovered = task(&mut store, original.key.id);
        assert_eq!(recovered.data, if redo { data } else { original.data });
        assert!(recovered.key.revision > edited.value.key.revision);
        assert_eq!(inspect(&mut store).unwrap().retained_operations, retained);
        let history = store.history().unwrap();
        if redo {
            assert!(history.redo.is_none());
            assert_eq!(history.undo.unwrap().kind, OperationKind::TaskEdited);
        } else {
            assert_eq!(history.redo.unwrap().kind, OperationKind::TaskEdited);
            assert_eq!(history.undo.unwrap().kind, OperationKind::TaskCreated);
        }
    }
}

#[test]
fn pc6_noop_for_every_entity_preserves_redo_but_an_old_revision_still_conflicts() {
    let db = Database::new();
    let mut store = db.open();
    let list = create_list(&mut store, "リスト");
    let calendar = create_calendar(&mut store, "カレンダー");
    let event = create_event(
        &mut store,
        "予定",
        EventWhen::AllDay(days("2026-11-11", "2026-11-12")),
    );
    let parent = create_task(&mut store, "変更前");
    let block = create_block(
        &mut store,
        &parent,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    let mut changed = block.task.data.clone();
    changed.title = Title::new("変更後").unwrap();
    let edited = store
        .save_task(Save::Update {
            expected: block.task.key,
            data: changed.clone(),
        })
        .unwrap();
    let undone = store.undo(edited.history.undo.unwrap().target).unwrap();
    let current = task(&mut store, parent.key.id);
    let ids = [
        EntityId::Task(parent.key.id),
        EntityId::TimeBlock(block.block.key.id),
        EntityId::Event(event.key.id),
        EntityId::List(list.key.id),
        EntityId::Calendar(calendar.key.id),
    ];
    let before = snapshot(&mut store, &ids);
    let task_noop = store
        .save_task(Save::Update {
            expected: current.key,
            data: current.data.clone(),
        })
        .unwrap();
    let block_noop = store
        .save_time_block(
            current.key,
            Save::Update {
                expected: block.block.key,
                data: block.block.data.clone(),
            },
        )
        .unwrap();
    let event_noop = store
        .save_event(Save::Update {
            expected: event.key,
            data: event.data.clone(),
        })
        .unwrap();
    let list_noop = store
        .save_collection(CollectionSave::List(Save::Update {
            expected: list.key,
            data: list.data.clone(),
        }))
        .unwrap();
    let calendar_noop = store
        .save_collection(CollectionSave::Calendar(Save::Update {
            expected: calendar.key,
            data: calendar.data.clone(),
        }))
        .unwrap();
    for (operation, changes, generation, history) in [
        (
            task_noop.operation,
            task_noop.changed,
            task_noop.generation,
            task_noop.history,
        ),
        (
            block_noop.operation,
            block_noop.changed,
            block_noop.generation,
            block_noop.history,
        ),
        (
            event_noop.operation,
            event_noop.changed,
            event_noop.generation,
            event_noop.history,
        ),
        (
            list_noop.operation,
            list_noop.changed,
            list_noop.generation,
            list_noop.history,
        ),
        (
            calendar_noop.operation,
            calendar_noop.changed,
            calendar_noop.generation,
            calendar_noop.history,
        ),
    ] {
        assert!(operation.is_none());
        assert!(changes.is_empty());
        assert_eq!(generation, undone.generation);
        assert_eq!(history, undone.history);
    }
    assert!(matches!(
        store.save_task(Save::Update {
            expected: block.task.key,
            data: current.data
        }),
        Err(Error::Conflict { .. })
    ));
    assert!(matches!(
        store.save_time_block(
            block.task.key,
            Save::Update {
                expected: block.block.key,
                data: block.block.data
            }
        ),
        Err(Error::Conflict {
            entity: EntityId::Task(_),
            ..
        })
    ));
    assert_eq!(snapshot(&mut store, &ids), before);
    store.redo(undone.history.redo.unwrap().target).unwrap();
    assert_eq!(task(&mut store, parent.key.id).data, changed);
}

#[test]
fn pc6_default_retention_is_100_original_operations_across_actors() {
    let db = Database::new();
    let actor_a = ActorId::new().unwrap();
    let actor_b = ActorId::new().unwrap();
    let mut a = db.open_as(actor_a, "2026-11-11T00:00:00Z");
    let first_task = create_task(&mut a, "最初の操作");
    let first_target = a.history().unwrap().undo.unwrap().target;
    for i in 1..60 {
        create_task(&mut a, &format!("A-{i:03}"));
    }
    assert_eq!(inspect(&mut a).unwrap().retained_operations, 60);
    drop(a);
    let mut b = db.open_as(actor_b, "2026-11-11T00:00:00Z");
    for i in 0..40 {
        create_task(&mut b, &format!("B-{i:03}"));
    }
    assert_eq!(inspect(&mut b).unwrap().retained_operations, 100);
    create_task(&mut b, "101件目");
    assert_eq!(inspect(&mut b).unwrap().retained_operations, 100);
    assert_eq!(inspect(&mut b).unwrap().live_tasks, 101);
    drop(b);
    let mut a = db.open_as(actor_a, "2026-11-11T00:00:00Z");
    let before = snapshot(&mut a, &[EntityId::Task(first_task.key.id)]);
    assert!(matches!(
        a.undo(first_target),
        Err(Error::History(HistoryProblem::Expired))
    ));
    assert_eq!(
        snapshot(&mut a, &[EntityId::Task(first_task.key.id)]),
        before
    );
    assert_eq!(task(&mut a, first_task.key.id), first_task);
}

#[test]
fn pc6_retention_survives_clock_reversal_and_all_100_retained_undos_still_work() {
    let db = Database::new();
    let actor = ActorId::new().unwrap();
    let mut store = db.open_as(actor, "2026-11-11T00:00:00Z");
    let mut current = create_task(&mut store, "版000");
    let now = instant("2026-11-11T00:00:00Z").unix_micros();
    for i in 1..=105 {
        set_now(&mut store, Instant::from_unix_micros(now - i).unwrap());
        let mut data = current.data.clone();
        data.title = Title::new(&format!("版{i:03}")).unwrap();
        current = store
            .save_task(Save::Update {
                expected: current.key,
                data,
            })
            .unwrap()
            .value;
        assert!(inspect(&mut store).unwrap().retained_operations <= 100);
    }
    assert_eq!(inspect(&mut store).unwrap().retained_operations, 100);
    for version in (5..105).rev() {
        let target = store.history().unwrap().undo.unwrap().target;
        store.undo(target).unwrap();
        let restored = task(&mut store, current.key.id);
        assert_eq!(restored.data.title.as_str(), format!("版{version:03}"));
        assert!(restored.key.revision > current.key.revision);
        current = restored;
    }
    assert!(store.history().unwrap().undo.is_none());
    assert_eq!(inspect(&mut store).unwrap().retained_operations, 100);
    assert_eq!(inspect(&mut store).unwrap().live_tasks, 1);
}

#[test]
fn pc6_thirty_day_boundary_is_exclusive_and_undo_redo_does_not_extend_it() {
    let db = Database::new();
    let actor = ActorId::new().unwrap();
    let mut store = db.open_as(actor, "2026-01-01T00:00:00Z");
    let original = create_task(&mut store, "30日保持");
    let original_target = store.history().unwrap().undo.unwrap().target;
    set_now(&mut store, instant("2026-01-30T23:59:59.999999Z"));
    assert!(store.history().unwrap().undo.is_some());
    let undone = store.undo(original_target).unwrap();
    let redone = store.redo(undone.history.redo.unwrap().target).unwrap();
    assert_eq!(task(&mut store, original.key.id).data, original.data);
    assert_eq!(inspect(&mut store).unwrap().retained_operations, 1);
    set_now(&mut store, instant("2026-01-31T00:00:00Z"));
    assert_eq!(
        store.history().unwrap(),
        HistoryState {
            undo: None,
            redo: None
        }
    );
    assert_eq!(inspect(&mut store).unwrap().retained_operations, 0);
    let before = snapshot(&mut store, &[EntityId::Task(original.key.id)]);
    assert!(matches!(
        store.undo(redone.history.undo.unwrap().target),
        Err(Error::History(HistoryProblem::Expired))
    ));
    assert_eq!(
        snapshot(&mut store, &[EntityId::Task(original.key.id)]),
        before
    );
    let current = task(&mut store, original.key.id);
    store.delete(DeleteTarget::Task(current.key)).unwrap();
    drop(store);
    let mut store = db.open_as(actor, "2026-03-02T00:00:00Z");
    assert_eq!(inspect(&mut store).unwrap().retained_operations, 0);
    let before = snapshot(&mut store, &[EntityId::Task(original.key.id)]);
    assert!(
        matches!(store.save_task(Save::Create { id: original.key.id, data: original.data }), Err(Error::AlreadyExists(EntityId::Task(id))) if id == original.key.id)
    );
    assert!(
        matches!(store.save_task(Save::Update { expected: current.key, data: data(&store, "古い編集") }), Err(Error::Conflict { current: CurrentVersion::Deleted(revision), .. }) if revision > current.key.revision)
    );
    assert_eq!(
        snapshot(&mut store, &[EntityId::Task(original.key.id)]),
        before
    );
}

#[test]
fn pc6_startup_expires_history_but_preserves_current_content_and_versions() {
    let db = Database::new();
    let actor = ActorId::new().unwrap();
    let mut store = db.open_as(actor, "2026-11-11T00:00:00Z");
    let original = create_task(&mut store, "現在状態が正本");
    let block = create_block(
        &mut store,
        &original,
        "2026-11-11T23:00:00+09:00",
        "2026-11-12T01:00:00+09:00",
    );
    let detail = store.task_detail(original.key.id).unwrap();
    drop(store);
    let mut store = db.open_as(actor, "2026-12-11T00:00:00Z");
    assert_eq!(store.task_detail(original.key.id).unwrap(), detail);
    assert_eq!(
        store.get(EntityId::TimeBlock(block.block.key.id)).unwrap(),
        Entity::TimeBlock(block.block)
    );
    assert_eq!(inspect(&mut store).unwrap().retained_operations, 0);
    assert_eq!(
        store.history().unwrap(),
        HistoryState {
            undo: None,
            redo: None
        }
    );
}

#[test]
fn pc6_production_byte_limits_are_public_contract_constants() {
    assert_eq!(MAX_OPERATION_BYTES, 1_048_576);
    assert_eq!(MAX_HISTORY_BYTES, 16_777_216);
    assert_eq!(MAX_HISTORY_OPERATIONS, 100);
    assert_eq!(HISTORY_DAYS, 30);
    assert_eq!(MAX_BLOCKS_PER_TASK, 128);
}

#[test]
fn pc6_reduced_operation_byte_limit_rejects_without_state_history_or_redo_changes() {
    let db = Database::new();
    let mut store = db.open();
    let original = create_task(&mut store, "元の内容");
    let mut edited = original.data.clone();
    edited.title = Title::new(&"長".repeat(240)).unwrap();
    let saved = store
        .save_task(Save::Update {
            expected: original.key,
            data: edited.clone(),
        })
        .unwrap();
    store.undo(saved.history.undo.unwrap().target).unwrap();
    set_history_limits(
        &mut store,
        HistoryLimits {
            operations: 100,
            history_bytes: MAX_HISTORY_BYTES,
            operation_bytes: 1,
        },
    )
    .unwrap();
    let current = task(&mut store, original.key.id);
    let before = snapshot(&mut store, &[EntityId::Task(original.key.id)]);
    assert!(
        matches!(store.save_task(Save::Update { expected: current.key, data: edited.clone() }), Err(Error::LimitExceeded { kind: LimitKind::ChangeBytes, maximum: 1, observed_at_least }) if observed_at_least > 1)
    );
    assert_eq!(
        snapshot(&mut store, &[EntityId::Task(original.key.id)]),
        before
    );
    set_history_limits(
        &mut store,
        HistoryLimits {
            operations: 100,
            history_bytes: MAX_HISTORY_BYTES,
            operation_bytes: MAX_OPERATION_BYTES,
        },
    )
    .unwrap();
    let redo = store.history().unwrap().redo.unwrap().target;
    store.redo(redo).unwrap();
    assert_eq!(task(&mut store, original.key.id).data, edited);
}

#[test]
fn pc6_reduced_history_bytes_prune_whole_operations_and_preserve_multirow_undo() {
    let db = Database::new();
    let mut store = db.open();
    let parent = create_task(&mut store, "集約の履歴容量");
    let a = create_block(
        &mut store,
        &parent,
        "2026-11-11T09:00:00+09:00",
        "2026-11-11T10:00:00+09:00",
    );
    let b = create_block(
        &mut store,
        &a.task,
        "2026-11-11T11:00:00+09:00",
        "2026-11-11T12:00:00+09:00",
    );
    let bytes_before = inspect(&mut store).unwrap().retained_image_bytes;
    store.delete(DeleteTarget::Task(b.task.key)).unwrap();
    let operation_bytes = inspect(&mut store).unwrap().retained_image_bytes - bytes_before;
    assert!(operation_bytes > 1);
    set_history_limits(
        &mut store,
        HistoryLimits {
            operations: 100,
            history_bytes: operation_bytes,
            operation_bytes: MAX_OPERATION_BYTES,
        },
    )
    .unwrap();
    let counts = inspect(&mut store).unwrap();
    assert_eq!(
        (counts.retained_operations, counts.retained_image_bytes),
        (1, operation_bytes)
    );
    let history = store.history().unwrap();
    assert_eq!(history.undo.as_ref().unwrap().affected, 3);
    store.undo(history.undo.unwrap().target).unwrap();
    let restored = store.task_detail(parent.key.id).unwrap();
    assert_eq!(restored.task.data, parent.data);
    assert_eq!(restored.blocks.len(), 2);
    assert_eq!(restored.blocks[0].data, a.block.data);
    assert_eq!(restored.blocks[1].data, b.block.data);
    set_history_limits(
        &mut store,
        HistoryLimits {
            operations: 100,
            history_bytes: operation_bytes - 1,
            operation_bytes: MAX_OPERATION_BYTES,
        },
    )
    .unwrap();
    let counts = inspect(&mut store).unwrap();
    assert_eq!(
        (counts.retained_operations, counts.retained_image_bytes),
        (0, 0)
    );
    assert_eq!(store.task_detail(parent.key.id).unwrap(), restored);
    assert_eq!(
        store.history().unwrap(),
        HistoryState {
            undo: None,
            redo: None
        }
    );
}
