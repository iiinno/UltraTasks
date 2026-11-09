use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};
use tempfile::tempdir;
use ultratasks_notifications::{DesiredNotification, Error, NotificationBackend, Scheduler};

#[derive(Default)]
struct NativeQueue {
    pending: BTreeMap<String, DesiredNotification>,
    shown: BTreeMap<String, DesiredNotification>,
    denied: bool,
    fail_after_effect: bool,
    scheduled: usize,
}
impl NotificationBackend for NativeQueue {
    fn pending_ids(&mut self) -> Result<BTreeSet<String>, Error> {
        Ok(self.pending.keys().cloned().collect())
    }
    fn schedule(&mut self, id: &str, value: &DesiredNotification) -> Result<(), Error> {
        if self.denied {
            return Err(Error::Permission("denied".into()));
        }
        self.scheduled += 1;
        self.pending.insert(id.into(), value.clone());
        if self.fail_after_effect {
            Err(Error::Platform("interrupted after registration".into()))
        } else {
            Ok(())
        }
    }
    fn show(&mut self, id: &str, value: &DesiredNotification) -> Result<(), Error> {
        self.pending.remove(id);
        self.shown.insert(id.into(), value.clone());
        Ok(())
    }
    fn cancel(&mut self, id: &str) -> Result<(), Error> {
        self.pending.remove(id);
        Ok(())
    }
}
fn now() -> DateTime<Utc> {
    "2026-11-09T00:00:00Z".parse().unwrap()
}
fn desired() -> DesiredNotification {
    DesiredNotification {
        id: "reminder:2026-11-09".into(),
        fire_at: now() + Duration::minutes(10),
        title: "予定 <日本語>".into(),
        body: "テスト".into(),
    }
}

#[test]
fn restart_repairs_missing_native_queue_without_duplicate_timer() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.db");
    let mut queue = NativeQueue::default();
    let mut scheduler = Scheduler::open(&path, "device-a").unwrap();
    scheduler
        .reconcile(&[desired()], now(), &mut queue)
        .unwrap();
    drop(scheduler);
    let mut scheduler = Scheduler::open(&path, "device-a").unwrap();
    assert_eq!(
        scheduler
            .reconcile(&[desired()], now(), &mut queue)
            .unwrap()
            .scheduled,
        0
    );
    queue.pending.clear(); // native queue lost after reset/reboot
    assert_eq!(
        scheduler
            .reconcile(&[desired()], now(), &mut queue)
            .unwrap()
            .scheduled,
        1
    );
    assert_eq!(queue.pending.len(), 1);
    assert_eq!(queue.pending.keys().next().unwrap().len(), 16);
}

#[test]
fn changed_time_replaces_and_deletion_cancels() {
    let dir = tempdir().unwrap();
    let mut scheduler = Scheduler::open(dir.path().join("state.db"), "device").unwrap();
    let mut queue = NativeQueue::default();
    let mut item = desired();
    scheduler
        .reconcile(&[item.clone()], now(), &mut queue)
        .unwrap();
    let id = queue.pending.keys().next().unwrap().clone();
    item.title = "変更後".into();
    item.fire_at += Duration::minutes(20);
    scheduler
        .reconcile(&[item.clone()], now(), &mut queue)
        .unwrap();
    assert_eq!(queue.pending.len(), 1);
    assert_eq!(queue.pending[&id], item);
    scheduler.reconcile(&[], now(), &mut queue).unwrap();
    assert!(queue.pending.is_empty());
}

#[test]
fn permission_failure_and_interrupted_effect_retry_without_losing_intent() {
    let dir = tempdir().unwrap();
    let mut scheduler = Scheduler::open(dir.path().join("state.db"), "device").unwrap();
    let mut queue = NativeQueue {
        denied: true,
        ..Default::default()
    };
    assert!(matches!(
        scheduler.reconcile(&[desired()], now(), &mut queue),
        Err(Error::Permission(_))
    ));
    queue.denied = false;
    queue.fail_after_effect = true;
    assert!(
        scheduler
            .reconcile(&[desired()], now(), &mut queue)
            .is_err()
    );
    assert_eq!(queue.pending.len(), 1);
    queue.fail_after_effect = false;
    scheduler
        .reconcile(&[desired()], now(), &mut queue)
        .unwrap();
    assert_eq!(queue.pending.len(), 1);
    assert_eq!(
        scheduler
            .reconcile(&[desired()], now(), &mut queue)
            .unwrap()
            .scheduled,
        0
    );
}

#[test]
fn resume_never_replays_timer_already_owned_by_os() {
    let dir = tempdir().unwrap();
    let mut scheduler = Scheduler::open(dir.path().join("state.db"), "device").unwrap();
    let mut queue = NativeQueue::default();
    scheduler
        .reconcile(&[desired()], now(), &mut queue)
        .unwrap();
    queue.pending.clear(); // OS already fired while app was stopped
    let report = scheduler
        .reconcile(&[desired()], now() + Duration::minutes(11), &mut queue)
        .unwrap();
    assert_eq!(report.elapsed, 1);
    assert!(queue.shown.is_empty());
    let mut renamed = desired();
    renamed.title = "時刻を変えない名前変更".into();
    scheduler
        .reconcile(&[renamed], now() + Duration::minutes(11), &mut queue)
        .unwrap();
    assert!(queue.shown.is_empty());
    let mut fresh = desired();
    fresh.id = "arrived-during-offline".into();
    scheduler
        .reconcile(
            &[desired(), fresh],
            now() + Duration::minutes(11),
            &mut queue,
        )
        .unwrap();
    assert_eq!(queue.shown.len(), 1);
}

#[test]
fn wrong_device_foreign_database_and_duplicate_input_are_rejected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("state.db");
    let mut scheduler = Scheduler::open(&path, "a").unwrap();
    assert!(matches!(
        Scheduler::open(&path, "b"),
        Err(Error::WrongDevice)
    ));
    let mut queue = NativeQueue::default();
    assert!(
        scheduler
            .reconcile(&[desired(), desired()], now(), &mut queue)
            .is_err()
    );
    assert_eq!(queue.scheduled, 0);
    let foreign = dir.path().join("foreign.db");
    let connection = rusqlite::Connection::open(&foreign).unwrap();
    connection
        .execute_batch("CREATE TABLE keep(value TEXT); INSERT INTO keep VALUES ('safe');")
        .unwrap();
    assert!(matches!(
        Scheduler::open(&foreign, "a"),
        Err(Error::UnrecognizedDatabase)
    ));
    assert_eq!(
        connection
            .query_row("SELECT value FROM keep", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "safe"
    );
}
