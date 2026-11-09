use chrono::{DateTime, Duration, Utc};
use ultratasks_core::{
    Command, DueDate, Engine, EntityId, EventException, EventSchedule, Frequency, Outcome,
    Recurrence, ReminderTrigger,
};
use ultratasks_notifications::{DevicePolicy, plan_notifications};

fn now() -> DateTime<Utc> {
    "2026-11-09T00:00:00Z".parse().unwrap()
}
fn policy() -> DevicePolicy {
    DevicePolicy {
        timezone: "Asia/Tokyo".into(),
        ..DevicePolicy::default()
    }
}

#[test]
fn task_completion_disabled_device_and_missed_grace_suppress_delivery() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Task(task) = engine
        .execute(Command::CreateTask {
            title: "期限".into(),
            due: Some(DueDate::At {
                at: now() + Duration::hours(1),
                timezone: "Asia/Tokyo".into(),
            }),
            list_id: None,
        })
        .unwrap()
    else {
        panic!()
    };
    engine
        .execute(Command::CreateReminder {
            target: EntityId::Task(task.id),
            trigger: ReminderTrigger::Before { minutes: 10 },
        })
        .unwrap();
    assert_eq!(
        plan_notifications(&engine, &policy(), now()).unwrap()[0].fire_at,
        now() + Duration::minutes(50)
    );
    let mut disabled = policy();
    disabled.enabled = false;
    assert!(
        plan_notifications(&engine, &disabled, now())
            .unwrap()
            .is_empty()
    );
    assert!(
        plan_notifications(&engine, &policy(), now() + Duration::hours(2))
            .unwrap()
            .is_empty()
    );
    engine
        .execute(Command::SetTaskCompleted {
            id: task.id,
            expected_revision: task.revision,
            completed: true,
        })
        .unwrap();
    assert!(
        plan_notifications(&engine, &policy(), now())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn relative_recurring_reminders_follow_exception_while_retaining_identity() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Event(event) = engine
        .execute(Command::CreateEvent {
            title: "朝の予定".into(),
            schedule: EventSchedule::Timed {
                start: now() + Duration::hours(1),
                end: now() + Duration::hours(2),
                timezone: "Asia/Tokyo".into(),
            },
        })
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Event(event) = engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: event.revision,
            recurrence: Some(Recurrence {
                frequency: Frequency::Daily,
                interval: 1,
                count: Some(3),
                until: None,
            }),
        })
        .unwrap()
    else {
        panic!()
    };
    engine
        .execute(Command::CreateReminder {
            target: EntityId::Event(event.id),
            trigger: ReminderTrigger::Before { minutes: 15 },
        })
        .unwrap();
    let before = plan_notifications(&engine, &policy(), now()).unwrap();
    assert_eq!(before.len(), 3);
    let date = "2026-11-10".parse().unwrap();
    let moved = now() + Duration::days(1) + Duration::hours(4);
    engine
        .execute(Command::SetEventException {
            id: event.id,
            expected_revision: event.revision,
            date,
            exception: Some(EventException::Override {
                title: Some("移動後".into()),
                schedule: EventSchedule::Timed {
                    start: moved,
                    end: moved + Duration::hours(1),
                    timezone: "Asia/Tokyo".into(),
                },
            }),
        })
        .unwrap();
    let after = plan_notifications(&engine, &policy(), now()).unwrap();
    assert_eq!(after[1].id, before[1].id);
    assert_eq!(after[1].fire_at, moved - Duration::minutes(15));
    assert_eq!(after[1].title, "移動後");
}

#[test]
fn date_only_uses_explicit_device_zone_and_absolute_reminders_fire_once_for_series() {
    let mut engine = Engine::in_memory().unwrap();
    let Outcome::Event(event) = engine
        .execute(Command::CreateEvent {
            title: "終日".into(),
            schedule: EventSchedule::AllDay {
                start: "2026-11-10".parse().unwrap(),
                end: "2026-11-11".parse().unwrap(),
            },
        })
        .unwrap()
    else {
        panic!()
    };
    engine
        .execute(Command::CreateReminder {
            target: EntityId::Event(event.id),
            trigger: ReminderTrigger::Before { minutes: 0 },
        })
        .unwrap();
    assert_eq!(
        plan_notifications(&engine, &policy(), now()).unwrap()[0].fire_at,
        "2026-11-09T15:00:00Z".parse::<DateTime<Utc>>().unwrap()
    );
    engine
        .execute(Command::SetEventRecurrence {
            id: event.id,
            expected_revision: event.revision,
            recurrence: Some(Recurrence {
                frequency: Frequency::Daily,
                interval: 1,
                count: Some(3),
                until: None,
            }),
        })
        .unwrap();
    engine
        .execute(Command::CreateReminder {
            target: EntityId::Event(event.id),
            trigger: ReminderTrigger::At(now() + Duration::hours(2)),
        })
        .unwrap();
    assert_eq!(
        plan_notifications(&engine, &policy(), now()).unwrap().len(),
        4
    );
}
