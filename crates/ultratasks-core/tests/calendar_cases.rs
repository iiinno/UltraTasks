#![cfg(feature = "test-support")]

mod common;

use common::*;
use ultratasks_core::*;

#[test]
fn pc9_gregorian_month_leap_year_and_supported_year_boundaries() {
    for (start, delta, expected) in [
        ("2026-01-31", 1, "2026-02-01"),
        ("2026-04-30", 1, "2026-05-01"),
        ("2026-12-31", 1, "2027-01-01"),
        ("2024-02-28", 1, "2024-02-29"),
        ("2024-02-29", 1, "2024-03-01"),
        ("2024-03-01", -1, "2024-02-29"),
        ("2000-02-28", 1, "2000-02-29"),
        ("1900-02-28", 1, "1900-03-01"),
        ("2100-02-28", 1, "2100-03-01"),
    ] {
        assert_eq!(date(start).add_days(delta).unwrap().iso(), expected);
    }
    assert_eq!(date("0001-01-01").ymd(), (1, 1, 1));
    assert_eq!(date("9999-12-31").ymd(), (9999, 12, 31));
    assert!(matches!(
        date("0001-01-01").add_days(-1),
        Err(Error::InvalidInput {
            field: Field::Date,
            problem: InputProblem::OutOfRange
        })
    ));
    assert!(matches!(
        date("9999-12-31").add_days(1),
        Err(Error::InvalidInput {
            field: Field::Date,
            problem: InputProblem::OutOfRange
        })
    ));
    for invalid in [
        "0000-01-01",
        "10000-01-01",
        "2026-02-29",
        "1900-02-29",
        "2026-04-31",
        "2026-00-01",
        "2026-1-01",
    ] {
        assert!(Date::parse(invalid).is_err(), "{invalid}");
    }
    assert!(matches!(
        DateSpan::new(date("2026-11-11"), date("2026-11-11")),
        Err(Error::InvalidInput {
            field: Field::Range,
            problem: InputProblem::NonIncreasingRange
        })
    ));
    assert!(DateSpan::new(date("2026-11-12"), date("2026-11-11")).is_err());
}

#[test]
fn pc9_instants_preserve_microseconds_and_reject_rounding_leap_seconds_and_overflow() {
    assert_eq!(instant("1970-01-01T00:00:00Z").unix_micros(), 0);
    assert_eq!(instant("1969-12-31T23:59:59.999999Z").unix_micros(), -1);
    assert_eq!(
        instant("2026-11-11T23:00:00.123456+09:00").rfc3339(),
        "2026-11-11T14:00:00.123456Z"
    );
    assert_eq!(
        instant("0001-01-01T00:00:00Z").unix_micros(),
        -62_135_596_800_000_000
    );
    assert_eq!(
        instant("9999-12-31T23:59:59.999999Z").unix_micros(),
        253_402_300_799_999_999
    );
    for value in [
        -62_135_596_800_000_001,
        253_402_300_800_000_000,
        i64::MIN,
        i64::MAX,
    ] {
        assert!(matches!(
            Instant::from_unix_micros(value),
            Err(Error::InvalidInput {
                field: Field::Instant,
                problem: InputProblem::OutOfRange
            })
        ));
    }
    for text in [
        "2026-11-11T00:00:00.1234567Z",
        "2026-11-11T00:00:00.1234560Z",
    ] {
        assert!(matches!(
            Instant::parse(text),
            Err(Error::InvalidInput {
                field: Field::Instant,
                problem: InputProblem::UnsupportedPrecision
            })
        ));
    }
    for text in [
        "2016-12-31T23:59:60Z",
        "2026-11-11T24:00:00Z",
        "2026-11-11T00:00:00",
        "0001-01-01T00:00:00+00:01",
        "9999-12-31T23:59:59-00:01",
    ] {
        assert!(Instant::parse(text).is_err(), "{text}");
    }
    assert!(LocalDateTime::parse("2026-11-11T00:00:00.1234567").is_err());
    assert!(LocalDateTime::parse("2016-12-31T23:59:60").is_err());
    assert!(matches!(
        ZoneId::parse("Invalid/AcceptanceZone"),
        Err(Error::InvalidInput {
            field: Field::Zone,
            problem: InputProblem::UnknownZone
        })
    ));
}

#[test]
fn pc9_new_york_gap_rejects_every_choice_and_fold_requires_an_explicit_offset_choice() {
    let ny = zone("America/New_York");
    let missing = LocalDateTime::parse("2026-03-08T02:30:00").unwrap();
    for choice in [
        OverlapChoice::Reject,
        OverlapChoice::Earlier,
        OverlapChoice::Later,
    ] {
        assert!(
            matches!(resolve_local(missing, &ny, choice), Err(Error::NonexistentLocalTime { local, zone }) if local == missing && zone == ny)
        );
    }
    let repeated = LocalDateTime::parse("2026-11-01T01:30:00.123456").unwrap();
    assert!(
        matches!(resolve_local(repeated, &ny, OverlapChoice::Reject), Err(Error::AmbiguousLocalTime { local, zone, earlier, later }) if local == repeated && zone == ny && earlier == instant("2026-11-01T05:30:00.123456Z") && later == instant("2026-11-01T06:30:00.123456Z"))
    );
    let early = resolve_local(repeated, &ny, OverlapChoice::Earlier).unwrap();
    let late = resolve_local(repeated, &ny, OverlapChoice::Later).unwrap();
    assert_eq!(early.instant, instant("2026-11-01T05:30:00.123456Z"));
    assert_eq!(late.instant, instant("2026-11-01T06:30:00.123456Z"));
    assert_eq!(
        local_at(early.instant, &ny).unwrap(),
        LocalView {
            datetime: repeated,
            offset_seconds: -14_400
        }
    );
    assert_eq!(
        local_at(late.instant, &ny).unwrap(),
        LocalView {
            datetime: repeated,
            offset_seconds: -18_000
        }
    );
    for (utc, local, offset) in [
        (
            "2026-03-08T06:59:59.999999Z",
            "2026-03-08T01:59:59.999999",
            -18_000,
        ),
        ("2026-03-08T07:00:00Z", "2026-03-08T03:00:00", -14_400),
    ] {
        assert_eq!(
            local_at(instant(utc), &ny).unwrap(),
            LocalView {
                datetime: LocalDateTime::parse(local).unwrap(),
                offset_seconds: offset
            }
        );
    }
}

#[test]
fn pc9_day_windows_use_first_valid_instant_including_midnight_gap_fold_and_skipped_date() {
    for (day, timezone, start, end, hours) in [
        (
            "2026-11-11",
            "Asia/Tokyo",
            "2026-11-10T15:00:00Z",
            "2026-11-11T15:00:00Z",
            24,
        ),
        (
            "2026-03-08",
            "America/New_York",
            "2026-03-08T05:00:00Z",
            "2026-03-09T04:00:00Z",
            23,
        ),
        (
            "2026-11-01",
            "America/New_York",
            "2026-11-01T04:00:00Z",
            "2026-11-02T05:00:00Z",
            25,
        ),
        (
            "2018-11-04",
            "America/Sao_Paulo",
            "2018-11-04T03:00:00Z",
            "2018-11-05T02:00:00Z",
            23,
        ),
        (
            "2026-11-01",
            "America/Havana",
            "2026-11-01T04:00:00Z",
            "2026-11-02T05:00:00Z",
            25,
        ),
        (
            "2011-12-29",
            "Pacific/Apia",
            "2011-12-29T10:00:00Z",
            "2011-12-30T10:00:00Z",
            24,
        ),
        (
            "2011-12-30",
            "Pacific/Apia",
            "2011-12-30T10:00:00Z",
            "2011-12-30T10:00:00Z",
            0,
        ),
        (
            "2011-12-31",
            "Pacific/Apia",
            "2011-12-30T10:00:00Z",
            "2011-12-31T10:00:00Z",
            24,
        ),
    ] {
        let w = window(day, timezone);
        assert_eq!(
            w.instant_bounds(),
            (instant(start), instant(end)),
            "{day}/{timezone}"
        );
        assert_eq!(w.timed().bounds(), (instant(start), instant(end)));
        assert_eq!(
            instant(end).unix_micros() - instant(start).unix_micros(),
            hours * 3_600_000_000
        );
    }
    for (text, timezone) in [
        ("2018-11-04T00:30:00", "America/Sao_Paulo"),
        ("2011-12-30T12:00:00", "Pacific/Apia"),
    ] {
        assert!(matches!(
            resolve_local(
                LocalDateTime::parse(text).unwrap(),
                &zone(timezone),
                OverlapChoice::Earlier
            ),
            Err(Error::NonexistentLocalTime { .. })
        ));
    }
    let empty = window("2011-12-30", "Pacific/Apia");
    let (start, end) = empty.instant_bounds();
    assert!(matches!(
        ZonedSpan::new(start, end, zone("Pacific/Apia")),
        Err(Error::InvalidInput {
            problem: InputProblem::NonIncreasingRange,
            ..
        })
    ));
}

#[test]
fn pc9_tokyo_23_to_01_is_one_saved_span_visible_on_two_days_only() {
    let db = Database::new();
    let mut store = db.open();
    let parent = create_task(&mut store, "夜間作業");
    let saved_span = span("2026-11-30T23:00:00+09:00", "2026-12-01T01:00:00+09:00");
    let block = store
        .save_time_block(
            parent.key,
            Save::Create {
                id: TimeBlockId::new().unwrap(),
                data: BlockData {
                    calendar: store.info().default_calendar,
                    span: saved_span.clone(),
                },
            },
        )
        .unwrap()
        .value
        .block;
    let event = create_event(&mut store, "夜間予定", EventWhen::Timed(saved_span.clone()));
    assert_eq!(
        saved_span.bounds(),
        (
            instant("2026-11-30T14:00:00Z"),
            instant("2026-11-30T16:00:00Z")
        )
    );
    for (day, expected) in [
        ("2026-11-29", 0),
        ("2026-11-30", 2),
        ("2026-12-01", 2),
        ("2026-12-02", 0),
    ] {
        let page = store.agenda(timed_query(day), first(10)).unwrap();
        assert_eq!(page.total, expected, "{day}");
        if expected != 0 {
            assert!(page.items.contains(&AgendaItem::Event(event.clone())));
            assert!(page.items.iter().any(
                |item| matches!(item, AgendaItem::TimeBlock { block: found, .. } if found == &block)
            ));
        }
    }
    for (day, start, end) in [
        ("2026-11-30", "2026-11-30T14:00:00Z", "2026-11-30T15:00:00Z"),
        ("2026-12-01", "2026-11-30T15:00:00Z", "2026-11-30T16:00:00Z"),
    ] {
        let w = window(day, "Asia/Tokyo");
        let clipped = w
            .clip(saved_span.bounds().0, saved_span.bounds().1)
            .unwrap();
        assert_eq!(clipped.bounds(), (instant(start), instant(end)));
        assert_eq!(clipped.calendar(), &w);
        let mut q = query();
        q.day = w;
        let rows = store.tasks(q, request(10, TaskPosition::First)).unwrap();
        assert_eq!(
            rows.items[0].blocks.allocated_micros_in_window,
            3_600_000_000
        );
        assert_eq!(
            rows.items[0].blocks.first_in_window,
            Some(saved_span.clone())
        );
    }
    drop(store);
    let mut store = db.open();
    assert_eq!(
        store.get(EntityId::TimeBlock(block.key.id)).unwrap(),
        Entity::TimeBlock(block)
    );
    assert_eq!(
        store.get(EntityId::Event(event.key.id)).unwrap(),
        Entity::Event(event)
    );
}

#[test]
fn pc9_all_day_end_is_exclusive_across_leap_day_and_month_end() {
    let db = Database::new();
    let mut store = db.open();
    let leap = create_event(
        &mut store,
        "うるう日",
        EventWhen::AllDay(days("2024-02-28", "2024-03-01")),
    );
    let march = create_event(
        &mut store,
        "3月の予定",
        EventWhen::AllDay(days("2024-03-01", "2024-03-02")),
    );
    for (day, end, expected) in [
        ("2024-02-27", "2024-02-28", vec![]),
        (
            "2024-02-28",
            "2024-02-29",
            vec![EntityId::Event(leap.key.id)],
        ),
        (
            "2024-02-29",
            "2024-03-01",
            vec![EntityId::Event(leap.key.id)],
        ),
        (
            "2024-03-01",
            "2024-03-02",
            vec![EntityId::Event(march.key.id)],
        ),
        ("2024-03-02", "2024-03-03", vec![]),
    ] {
        let page = store
            .agenda(
                AgendaQuery {
                    range: AgendaRange::AllDay(days(day, end)),
                    calendar: None,
                },
                first(10),
            )
            .unwrap();
        assert_eq!(
            page.items.iter().map(agenda_id).collect::<Vec<_>>(),
            expected,
            "{day}"
        );
        assert_eq!(page.total as usize, expected.len());
        assert_eq!(store.agenda(timed_query(day), first(10)).unwrap().total, 0);
    }
    assert_eq!(
        store.get(EntityId::Event(leap.key.id)).unwrap(),
        Entity::Event(leap)
    );
}

#[test]
fn pc9_adjacent_blocks_are_excluded_at_the_edge_and_overlaps_count_each_clipped_duration() {
    let db = Database::new();
    let mut store = db.open();
    let mut parent = create_task(&mut store, "重なりを許す");
    let mut blocks = Vec::new();
    for (start, end) in [
        ("2026-11-10T23:00:00+09:00", "2026-11-11T00:00:00+09:00"),
        ("2026-11-10T23:30:00+09:00", "2026-11-11T00:30:00+09:00"),
        ("2026-11-11T00:00:00+09:00", "2026-11-11T01:00:00+09:00"),
        ("2026-11-11T00:30:00+09:00", "2026-11-11T01:30:00+09:00"),
        ("2026-11-12T00:00:00+09:00", "2026-11-12T01:00:00+09:00"),
    ] {
        let saved = create_block(&mut store, &parent, start, end);
        parent = saved.task;
        blocks.push(saved.block);
    }
    let page = store.agenda(timed_query("2026-11-11"), first(10)).unwrap();
    assert_eq!(
        page.items.iter().map(agenda_id).collect::<Vec<_>>(),
        blocks[1..4]
            .iter()
            .map(|b| EntityId::TimeBlock(b.key.id))
            .collect::<Vec<_>>()
    );
    assert_eq!(page.total, 3);
    let row = store
        .tasks(query(), request(10, TaskPosition::First))
        .unwrap()
        .items
        .remove(0);
    assert_eq!(row.blocks.total_for_task, 5);
    assert_eq!(row.blocks.in_window, 3);
    assert_eq!(row.blocks.allocated_micros_in_window, 9_000_000_000);
    assert_eq!(
        row.blocks.first_in_window,
        Some(blocks[1].data.span.clone())
    );
    let visible = query()
        .day
        .clip(
            instant("2026-11-11T00:30:00+09:00"),
            instant("2026-11-11T01:00:00+09:00"),
        )
        .unwrap();
    let page = store
        .agenda(
            AgendaQuery {
                range: AgendaRange::Timed(visible),
                calendar: None,
            },
            first(10),
        )
        .unwrap();
    assert_eq!(
        page.items.iter().map(agenda_id).collect::<Vec<_>>(),
        vec![
            EntityId::TimeBlock(blocks[2].key.id),
            EntityId::TimeBlock(blocks[3].key.id)
        ]
    );
    assert!(matches!(
        query().day.clip(
            instant("2026-11-12T00:00:00+09:00"),
            instant("2026-11-12T01:00:00+09:00")
        ),
        Err(Error::InvalidInput {
            problem: InputProblem::OutsideWindow,
            ..
        })
    ));
}

#[test]
fn pc9_skipped_apia_date_has_no_timed_matches_even_when_a_span_straddles_its_boundary() {
    let db = Database::new();
    let mut store = db.open();
    let parent = create_task(&mut store, "欠落日の比較");
    let span = ZonedSpan::new(
        instant("2011-12-30T09:00:00Z"),
        instant("2011-12-30T11:00:00Z"),
        zone("Pacific/Apia"),
    )
    .unwrap();
    store
        .save_time_block(
            parent.key,
            Save::Create {
                id: TimeBlockId::new().unwrap(),
                data: BlockData {
                    calendar: store.info().default_calendar,
                    span: span.clone(),
                },
            },
        )
        .unwrap();
    create_event(&mut store, "時刻付き", EventWhen::Timed(span));
    let all_day = create_event(
        &mut store,
        "暦日を保存",
        EventWhen::AllDay(days("2011-12-30", "2011-12-31")),
    );
    let empty = window("2011-12-30", "Pacific/Apia");
    assert_eq!(
        store
            .agenda(
                AgendaQuery {
                    range: AgendaRange::Timed(empty.timed()),
                    calendar: None
                },
                first(10)
            )
            .unwrap()
            .total,
        0
    );
    let page = store
        .agenda(
            AgendaQuery {
                range: AgendaRange::AllDay(empty.days()),
                calendar: None,
            },
            first(10),
        )
        .unwrap();
    assert_eq!(page.items, vec![AgendaItem::Event(all_day)]);
    let mut q = query();
    q.day = empty;
    let row = store
        .tasks(q.clone(), request(10, TaskPosition::First))
        .unwrap()
        .items
        .remove(0);
    assert_eq!(
        row.blocks,
        BlockSummary {
            total_for_task: 1,
            in_window: 0,
            allocated_micros_in_window: 0,
            first_in_window: None
        }
    );
    q.blocks = BlockFilter::InSelectedWindow;
    assert_eq!(
        store
            .tasks(q.clone(), request(10, TaskPosition::First))
            .unwrap()
            .total,
        0
    );
    q.blocks = BlockFilter::NotInSelectedWindow;
    assert_eq!(
        task_ids(&store.tasks(q, request(10, TaskPosition::First)).unwrap()),
        vec![parent.key.id]
    );
}

#[test]
fn pc9_calendar_query_window_limit_accepts_366_days_and_rejects_367() {
    assert!(CalendarWindow::new(days("2024-01-01", "2025-01-01"), zone("UTC")).is_ok());
    assert!(matches!(
        CalendarWindow::new(days("2024-01-01", "2025-01-02"), zone("UTC")),
        Err(Error::LimitExceeded {
            kind: LimitKind::QueryDays,
            maximum: 366,
            observed_at_least: 367
        })
    ));
    let db = Database::new();
    let mut store = db.open();
    assert!(matches!(
        store.agenda(
            AgendaQuery {
                range: AgendaRange::AllDay(days("2024-01-01", "2025-01-02")),
                calendar: None
            },
            first(1)
        ),
        Err(Error::LimitExceeded {
            kind: LimitKind::QueryDays,
            maximum: 366,
            observed_at_least: 367
        })
    ));
}
