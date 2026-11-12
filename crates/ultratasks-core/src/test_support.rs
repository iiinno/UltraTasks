use crate::{Instant, Store, history, model::*, storage};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailPoint {
    AfterStateWrite,
    AfterHistoryWrite,
    BeforeCommit,
    AfterCommitBeforeReply,
}
#[derive(Clone, Copy, Debug)]
pub struct HistoryLimits {
    pub operations: u16,
    pub history_bytes: u64,
    pub operation_bytes: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Inspection {
    pub retained_operations: u64,
    pub retained_image_bytes: u64,
    pub live_tasks: u64,
    pub live_blocks: u64,
}
pub fn open_as_actor(path: &Path, actor: ActorId, now: Instant) -> Result<Store> {
    Store::open_inner(path, Some(actor), Some(now))
}
pub fn set_now(store: &mut Store, now: Instant) {
    store.fixed_now = Some(now);
}
pub fn fail_next(store: &mut Store, at: FailPoint) {
    store.fail = Some(at);
}
pub fn set_history_limits(store: &mut Store, limits: HistoryLimits) -> Result<()> {
    store.ready()?;
    if limits.operations == 0
        || limits.operations > MAX_HISTORY_OPERATIONS
        || limits.history_bytes == 0
        || limits.history_bytes > MAX_HISTORY_BYTES
        || limits.operation_bytes == 0
        || limits.operation_bytes > MAX_OPERATION_BYTES
    {
        return Err(invalid(Field::Query, InputProblem::OutOfRange));
    }
    let old = store.limits;
    store.limits = history::Limits {
        operations: limits.operations,
        history_bytes: limits.history_bytes,
        operation_bytes: limits.operation_bytes,
    };
    if let Err(error) = store.transaction(|s| history::prune(&s.connection, s.now(), s.limits)) {
        store.limits = old;
        return Err(error);
    }
    Ok(())
}
pub fn inspect(store: &mut Store) -> Result<Inspection> {
    store.read(|s| {
        s.connection.query_row("SELECT count(*),coalesce(sum(image_bytes),0),(SELECT count(*) FROM tasks WHERE deleted=0),(SELECT count(*) FROM blocks WHERE deleted=0) FROM history",[],|r| {
            Ok(Inspection { retained_operations:r.get(0)?,retained_image_bytes:r.get(1)?,live_tasks:r.get(2)?,live_blocks:r.get(3)? })
        }).map_err(|e| storage::sql_error(StorageStage::Read,e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data(store: &Store, title: &str) -> TaskData {
        TaskData {
            title: Title::new(title).unwrap(),
            list: store.info().default_list,
            status: TaskStatus::Open,
            due: None,
        }
    }
    #[test]
    fn failure_injection_is_atomic_and_unknown_requires_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("failure.sqlite");
        let mut store = Store::open(&path).unwrap();
        for point in [
            FailPoint::AfterStateWrite,
            FailPoint::AfterHistoryWrite,
            FailPoint::BeforeCommit,
        ] {
            let id = TaskId::new().unwrap();
            fail_next(&mut store, point);
            let input = data(&store, "失敗から再試行");
            assert!(matches!(
                store.save_task(Save::Create {
                    id,
                    data: input.clone()
                }),
                Err(Error::Storage {
                    outcome: CommitKnowledge::NotCommitted,
                    ..
                })
            ));
            assert!(matches!(
                store.get(EntityId::Task(id)),
                Err(Error::NotFound(_))
            ));
            assert_eq!(inspect(&mut store).unwrap().live_tasks, 0);
            assert_eq!(inspect(&mut store).unwrap().retained_operations, 0);
        }
        let id = TaskId::new().unwrap();
        let input = data(&store, "応答前に中断");
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
        drop(store);
        let mut store = Store::open(&path).unwrap();
        assert!(matches!(store.get(EntityId::Task(id)), Ok(Entity::Task(_))));
        assert!(matches!(
            store.save_task(Save::Create { id, data: input }),
            Err(Error::AlreadyExists(_))
        ));
        assert_eq!(inspect(&mut store).unwrap().live_tasks, 1);
    }
    #[test]
    fn other_actor_and_retention_cannot_rewrite_intervening_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("actors.sqlite");
        let now = Instant::parse("2026-11-11T00:00:00Z").unwrap();
        let actor_a = ActorId::new().unwrap();
        let actor_b = ActorId::new().unwrap();
        let mut a = open_as_actor(&path, actor_a, now).unwrap();
        let id = TaskId::new().unwrap();
        let input = data(&a, "Aが作成");
        let created = a.save_task(Save::Create { id, data: input }).unwrap();
        let target = created.history.undo.unwrap().target;
        drop(a);
        let mut b = open_as_actor(&path, actor_b, now).unwrap();
        let input = data(&b, "Bが編集");
        b.save_task(Save::Update {
            expected: created.value.key,
            data: input,
        })
        .unwrap();
        assert!(matches!(
            b.undo(target.clone()),
            Err(Error::History(HistoryProblem::NotOwn))
        ));
        drop(b);
        let mut a = open_as_actor(&path, actor_a, now).unwrap();
        let target = a.history().unwrap().undo.unwrap().target;
        assert!(matches!(a.undo(target), Err(Error::HistoryConflict { .. })));
        set_now(&mut a, Instant::parse("2026-12-11T00:00:00Z").unwrap());
        assert!(a.history().unwrap().undo.is_none());
        let Entity::Task(current) = a.get(EntityId::Task(id)).unwrap() else {
            panic!()
        };
        a.delete(DeleteTarget::Task(current.key)).unwrap();
        set_now(&mut a, Instant::parse("2027-02-01T00:00:00Z").unwrap());
        assert!(a.history().unwrap().undo.is_none());
        let input = data(&a, "IDは再利用しない");
        assert!(matches!(
            a.save_task(Save::Create { id, data: input }),
            Err(Error::AlreadyExists(_))
        ));
    }
    #[test]
    fn limits_noop_and_cascade_failure_preserve_state() {
        use crate::{BlockData, TimeBlockId, ZoneId, ZonedSpan};
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("limits.sqlite")).unwrap();
        let input = data(&store, "元のタスク");
        let mut task = store
            .save_task(Save::Create {
                id: TaskId::new().unwrap(),
                data: input,
            })
            .unwrap()
            .value;
        let stale = task.key;
        let original = task.data.clone();
        task.data.title = Title::new("編集したタスク").unwrap();
        let edited = store
            .save_task(Save::Update {
                expected: task.key,
                data: task.data,
            })
            .unwrap();
        let undone = store.undo(edited.history.undo.unwrap().target).unwrap();
        let Entity::Task(current) = store.get(EntityId::Task(stale.id)).unwrap() else {
            panic!()
        };
        let noop = store
            .save_task(Save::Update {
                expected: current.key,
                data: original.clone(),
            })
            .unwrap();
        assert!(noop.operation.is_none());
        assert!(noop.changed.is_empty());
        assert_eq!(noop.history, undone.history);
        assert!(matches!(
            store.save_task(Save::Update {
                expected: stale,
                data: original
            }),
            Err(Error::Conflict { .. })
        ));
        let start = Instant::parse("2026-11-11T10:00:00+09:00").unwrap();
        let block = BlockData {
            calendar: store.info().default_calendar,
            span: ZonedSpan::new(
                start,
                Instant::from_unix_micros(start.unix_micros() + 3_600_000_000).unwrap(),
                ZoneId::parse("Asia/Tokyo").unwrap(),
            )
            .unwrap(),
        };
        task = current;
        for _ in 0..MAX_BLOCKS_PER_TASK {
            task = store
                .save_time_block(
                    task.key,
                    Save::Create {
                        id: TimeBlockId::new().unwrap(),
                        data: block.clone(),
                    },
                )
                .unwrap()
                .value
                .task;
        }
        assert!(matches!(
            store.save_time_block(
                task.key,
                Save::Create {
                    id: TimeBlockId::new().unwrap(),
                    data: block
                }
            ),
            Err(Error::LimitExceeded {
                kind: LimitKind::BlocksPerTask,
                ..
            })
        ));
        let before = store.task_detail(task.key.id).unwrap();
        let history = store.history().unwrap();
        assert_eq!(
            inspect(&mut store).unwrap().retained_operations,
            u64::from(MAX_HISTORY_OPERATIONS)
        );
        for point in [
            FailPoint::AfterStateWrite,
            FailPoint::AfterHistoryWrite,
            FailPoint::BeforeCommit,
        ] {
            fail_next(&mut store, point);
            assert!(store.delete(DeleteTarget::Task(task.key)).is_err());
            assert_eq!(store.task_detail(task.key.id).unwrap(), before);
            assert_eq!(store.history().unwrap(), history);
        }
        let deleted = store.delete(DeleteTarget::Task(task.key)).unwrap();
        assert_eq!(deleted.changed.len(), 129);
        store.undo(deleted.history.undo.unwrap().target).unwrap();
        assert_eq!(store.task_detail(task.key.id).unwrap().blocks.len(), 128);
        set_history_limits(
            &mut store,
            HistoryLimits {
                operations: 3,
                history_bytes: MAX_HISTORY_BYTES,
                operation_bytes: MAX_OPERATION_BYTES,
            },
        )
        .unwrap();
        assert_eq!(inspect(&mut store).unwrap().retained_operations, 3);
        set_history_limits(
            &mut store,
            HistoryLimits {
                operations: 3,
                history_bytes: 4000,
                operation_bytes: 1000,
            },
        )
        .unwrap();
        assert!(inspect(&mut store).unwrap().retained_image_bytes <= 4000);
        let Entity::Task(current) = store.get(EntityId::Task(task.key.id)).unwrap() else {
            panic!()
        };
        let mut changed = current.data;
        changed.title = Title::new(&"長".repeat(240)).unwrap();
        assert!(matches!(
            store.save_task(Save::Update {
                expected: current.key,
                data: changed
            }),
            Err(Error::LimitExceeded {
                kind: LimitKind::ChangeBytes,
                ..
            })
        ));
    }
}
