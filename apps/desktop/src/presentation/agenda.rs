use ultratasks_core::*;

pub const MINUTE: i64 = 60_000_000;
pub const AXIS_INSET: f32 = 12.;

pub fn date_heading(day: Date) -> String {
    use chrono::Datelike;
    let (year, month, date) = day.ymd();
    let weekday = chrono::NaiveDate::from_ymd_opt(year, month as u32, date as u32)
        .map(|d| {
            ["月", "火", "水", "木", "金", "土", "日"][d.weekday().num_days_from_monday() as usize]
        })
        .unwrap_or("");
    format!("{month}月{date}日（{weekday}）")
}

pub fn compact_card(height: f32) -> bool {
    height < 40.
}
pub fn initial_axis_offset(minute: f32) -> f32 {
    (minute - AXIS_INSET).max(0.)
}
pub fn calendar(day: Date, zone: &ZoneId) -> Result<CalendarWindow> {
    CalendarWindow::new(DateSpan::new(day, day.add_days(1)?)?, zone.clone())
}
pub fn instant_label(instant: Instant, zone: &ZoneId) -> String {
    match local_at(instant, zone) {
        Ok(view) => {
            let (h, m, _, _) = view.datetime.clock();
            format!(
                "{} {h:02}:{m:02} {}",
                view.datetime.date().iso(),
                offset_label(view.offset_seconds)
            )
        }
        Err(_) => instant.rfc3339(),
    }
}
pub fn clock_label(instant: Instant, zone: &ZoneId) -> String {
    match local_at(instant, zone) {
        Ok(v) => {
            let (h, m, _, _) = v.datetime.clock();
            format!("{h:02}:{m:02}")
        }
        Err(_) => "時刻不明".into(),
    }
}
pub fn offset_label(offset: i32) -> String {
    format!(
        "UTC{}{:#02}:{:02}",
        if offset < 0 { "-" } else { "+" },
        offset.abs() / 3600,
        (offset.abs() % 3600) / 60
    )
}
pub fn span_label(span: &ZonedSpan, zone: &ZoneId) -> String {
    let (a, b) = span.bounds();
    match (local_at(a, zone), local_at(b, zone)) {
        (Ok(start), Ok(end)) if start.datetime.date() == end.datetime.date() => format!(
            "{}–{} · {}分",
            clock_label(a, zone),
            clock_label(b, zone),
            (b.unix_micros() - a.unix_micros()) / MINUTE
        ),
        _ => format!("{} → {}", instant_label(a, zone), instant_label(b, zone)),
    }
}
pub fn title(item: &AgendaItem) -> &str {
    match item {
        AgendaItem::Event(e) => e.data.title.as_str(),
        AgendaItem::TimeBlock { task, .. } => task.title.as_str(),
    }
}
pub fn id(item: &AgendaItem) -> EntityId {
    match item {
        AgendaItem::Event(e) => EntityId::Event(e.key.id),
        AgendaItem::TimeBlock { block, .. } => EntityId::TimeBlock(block.key.id),
    }
}
pub fn span(item: &AgendaItem) -> Option<&ZonedSpan> {
    match item {
        AgendaItem::Event(e) => match &e.data.when {
            EventWhen::Timed(span) => Some(span),
            _ => None,
        },
        AgendaItem::TimeBlock { block, .. } => Some(&block.data.span),
    }
}
#[derive(Clone)]
pub struct Placement {
    pub index: usize,
    pub start: f32,
    pub end: f32,
    pub lane: usize,
    pub lanes: usize,
    pub group: usize,
}
pub struct AgendaCard {
    pub placement: Placement,
    pub candidates: Option<usize>,
}
pub fn cards(items: &[AgendaItem], window: &CalendarWindow, width: f32) -> Vec<AgendaCard> {
    let placed = placements(items, window);
    let mut cards = Vec::new();
    for group in placed.chunk_by(|a, b| a.group == b.group) {
        let first = &group[0];
        if width / (first.lanes.max(1) as f32) < 120. {
            let mut placement = first.clone();
            placement.start = group.iter().map(|p| p.start).fold(f32::INFINITY, f32::min);
            placement.end = group.iter().map(|p| p.end).fold(0., f32::max);
            cards.push(AgendaCard {
                placement,
                candidates: Some(group.len()),
            });
        } else {
            cards.extend(group.iter().cloned().map(|placement| AgendaCard {
                placement,
                candidates: None,
            }));
        }
    }
    cards
}
pub fn placements(items: &[AgendaItem], window: &CalendarWindow) -> Vec<Placement> {
    let (base, end) = window.instant_bounds();
    let mut spans: Vec<_> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| span(item).map(|s| (index, s.bounds())))
        .filter(|(_, (a, b))| *a < end && *b > base)
        .collect();
    spans.sort_by_key(|(index, (a, b))| (*a, *b, *index));
    let mut output: Vec<Placement> = Vec::new();
    let mut lane_ends = Vec::new();
    let mut group_start = 0;
    let mut group_end = base;
    for (index, (a, b)) in spans {
        if a >= group_end {
            let lanes = lane_ends.len();
            for placement in &mut output[group_start..] {
                placement.lanes = lanes;
            }
            lane_ends.clear();
            group_start = output.len();
        }
        let lane = lane_ends
            .iter()
            .position(|end| *end <= a)
            .unwrap_or(lane_ends.len());
        if lane == lane_ends.len() {
            lane_ends.push(b);
        } else {
            lane_ends[lane] = b;
        }
        group_end = group_end.max(b);
        output.push(Placement {
            index,
            start: (a.max(base).unix_micros() - base.unix_micros()) as f32 / MINUTE as f32,
            end: (b.min(end).unix_micros() - base.unix_micros()) as f32 / MINUTE as f32,
            lane,
            lanes: 1,
            group: group_start,
        });
    }
    for placement in &mut output[group_start..] {
        placement.lanes = lane_ends.len();
    }
    output
}
pub fn snap_minute(minutes: f32) -> i64 {
    (((minutes.max(0.) - 0.00001) / 15. + 0.5).floor() as i64) * 15
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_group_covers_longest_member_and_counts_all_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(&temp.path().join("dense.sqlite")).unwrap();
        let calendar_id = store.info().default_calendar;
        let zone = ZoneId::parse("Asia/Tokyo").unwrap();
        let day = calendar(Date::parse("2026-11-11").unwrap(), &zone).unwrap();
        let mut items = Vec::new();
        for index in 0..7 {
            let span = ZonedSpan::new(
                Instant::parse("2026-11-11T09:00:00+09:00").unwrap(),
                Instant::parse(if index == 0 {
                    "2026-11-11T09:30:00+09:00"
                } else {
                    "2026-11-11T11:00:00+09:00"
                })
                .unwrap(),
                zone.clone(),
            )
            .unwrap();
            let saved = store
                .save_event(Save::Create {
                    id: EventId::new().unwrap(),
                    data: EventData {
                        title: Title::new(&format!("候補{index}")).unwrap(),
                        calendar: calendar_id,
                        when: EventWhen::Timed(span),
                    },
                })
                .unwrap()
                .value;
            items.push(AgendaItem::Event(saved));
        }
        for width in [468., 704.] {
            let cards = cards(&items, &day, width);
            assert_eq!(cards.len(), 1);
            assert_eq!(cards[0].placement.start, 540.);
            assert_eq!(cards[0].placement.end, 660.);
            assert_eq!(cards[0].candidates, Some(7));
        }
        let short = cards(&items[..1], &day, 704.);
        assert_eq!(short[0].placement.end - short[0].placement.start, 30.);
        assert!(compact_card(30.));
        assert!(compact_card(15.));
        assert!(!compact_card(60.));

        let mut later = vec![("2026-11-11T10:45:00+09:00", "2026-11-11T12:00:00+09:00")];
        later.extend((0..7).map(|index| {
            (
                "2026-11-11T13:00:00+09:00",
                if index == 0 {
                    "2026-11-11T13:30:00+09:00"
                } else {
                    "2026-11-11T15:00:00+09:00"
                },
            )
        }));
        later.push(("2026-11-11T23:30:00+09:00", "2026-11-12T00:30:00+09:00"));
        for (start, end) in later {
            let saved = store
                .save_event(Save::Create {
                    id: EventId::new().unwrap(),
                    data: EventData {
                        title: Title::new("別の占有区間").unwrap(),
                        calendar: calendar_id,
                        when: EventWhen::Timed(
                            ZonedSpan::new(
                                Instant::parse(start).unwrap(),
                                Instant::parse(end).unwrap(),
                                zone.clone(),
                            )
                            .unwrap(),
                        ),
                    },
                })
                .unwrap()
                .value;
            items.push(AgendaItem::Event(saved));
        }
        let collapsed = cards(&items, &day, 704.);
        assert_eq!(collapsed.len(), 3);
        assert_eq!(
            (collapsed[0].placement.start, collapsed[0].placement.end),
            (540., 720.)
        );
        assert_eq!(collapsed[0].candidates, Some(8));
        assert_eq!(
            (collapsed[1].placement.start, collapsed[1].placement.end),
            (780., 900.)
        );
        assert_eq!(collapsed[1].candidates, Some(7));
        assert_eq!(
            (collapsed[2].placement.start, collapsed[2].placement.end),
            (1410., 1440.)
        );
        assert_eq!(collapsed[2].candidates, None);
        let expanded = cards(&items, &day, 1400.);
        assert_eq!(expanded.len(), items.len());
        assert!(expanded.iter().all(|card| card.candidates.is_none()));
    }
    #[test]
    fn initial_hour_label_has_room_above_the_true_time_line() {
        assert_eq!(540. - initial_axis_offset(540.), 12.);
        assert_eq!(initial_axis_offset(0.), 0.);
        assert_eq!(
            date_heading(Date::parse("2026-11-11").unwrap()),
            "11月11日（水）"
        );
    }
    #[test]
    fn drag_midpoint_chooses_earlier_quarter() {
        assert_eq!(snap_minute(7.5), 0);
        assert_eq!(snap_minute(7.51), 15);
        assert_eq!(snap_minute(23.), 30);
    }
}
