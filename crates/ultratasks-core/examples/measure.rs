#[cfg(not(feature = "fixtures"))]
fn main() {
    eprintln!("measureには --features fixtures が必要です");
    std::process::exit(2);
}

#[cfg(feature = "fixtures")]
fn main() {
    if let Err(error) = measurement::run() {
        eprintln!("MEASURE_FAILED: {error}");
        std::process::exit(1);
    }
}

#[cfg(feature = "fixtures")]
mod measurement {
    use serde::Serialize;
    use serde_json::{Value, json};
    use std::{
        collections::HashSet,
        error::Error as StdError,
        fs::{self, File, OpenOptions},
        io::{BufWriter, Write},
        path::{Path, PathBuf},
        time::{Instant as Clock, SystemTime, UNIX_EPOCH},
    };
    use ultratasks_core::*;

    type Check<T> = std::result::Result<T, Box<dyn StdError>>;
    const OPERATIONS: &[&str] = &[
        "search_and_count",
        "count_only",
        "agenda_timed",
        "agenda_all_day",
        "create",
        "edit",
        "complete",
        "undo",
        "redo",
        "second_block",
    ];

    struct Options {
        sizes: Vec<u64>,
        seed: u64,
        date: Date,
        zone: ZoneId,
        warmup: usize,
        samples: usize,
        trials: usize,
        timeout_ms: u64,
        distribution: fixtures::Distribution,
        out: PathBuf,
        environment: Option<PathBuf>,
    }

    fn options() -> Check<Option<Options>> {
        let mut o = Options {
            sizes: vec![1000, 10000],
            seed: 42,
            date: Date::parse("2026-11-11")?,
            zone: ZoneId::parse("Asia/Tokyo")?,
            warmup: 100,
            samples: 1000,
            trials: 5,
            timeout_ms: 5000,
            distribution: fixtures::Distribution::ThirtyDays,
            out: PathBuf::new(),
            environment: None,
        };
        let mut args = std::env::args().skip(1);
        let mut seen = HashSet::new();
        while let Some(key) = args.next() {
            if key == "--help" || key == "-h" {
                println!(
                    "measure --out <絶対出力先> [--sizes 1000,10000] [--seed 42] [--date 2026-11-11] [--zone Asia/Tokyo] [--warmup 100] [--samples 1000] [--trials 5] [--distribution spread|dense] [--timeout-ms 5000] [--environment <JSON>]"
                );
                return Ok(None);
            }
            ensure(seen.insert(key.clone()), "CLI引数が重複しています")?;
            let value = args.next().ok_or("CLI引数の値がありません")?;
            match key.as_str() {
                "--sizes" => {
                    o.sizes = value
                        .split(',')
                        .map(str::parse)
                        .collect::<std::result::Result<_, _>>()?
                }
                "--seed" => o.seed = value.parse()?,
                "--date" => o.date = Date::parse(&value)?,
                "--zone" => o.zone = ZoneId::parse(&value)?,
                "--warmup" => o.warmup = value.parse()?,
                "--samples" => o.samples = value.parse()?,
                "--trials" => o.trials = value.parse()?,
                "--timeout-ms" => o.timeout_ms = value.parse()?,
                "--distribution" => {
                    o.distribution = match value.as_str() {
                        "spread" => fixtures::Distribution::ThirtyDays,
                        "dense" => fixtures::Distribution::DenseDay,
                        _ => return Err("distributionはspreadまたはdenseです".into()),
                    }
                }
                "--out" => o.out = value.into(),
                "--environment" => o.environment = Some(value.into()),
                _ => return Err(format!("不明な引数: {key}").into()),
            }
        }
        ensure(o.out.is_absolute(), "--outに絶対パスを指定してください")?;
        ensure(
            !o.sizes.is_empty() && o.sizes.iter().all(|s| [1000, 10000].contains(s)),
            "sizesは1000,10000だけです",
        )?;
        ensure(
            o.sizes.iter().collect::<HashSet<_>>().len() == o.sizes.len(),
            "sizesが重複しています",
        )?;
        ensure(
            (1..=100_000).contains(&o.samples)
                && o.warmup <= 100_000
                && (1..=100).contains(&o.trials),
            "samples/trialsが範囲外です",
        )?;
        ensure(
            (1..=3_600_000).contains(&o.timeout_ms),
            "timeout-msが範囲外です",
        )?;
        o.date.add_days(32)?;
        ensure(
            !cfg!(debug_assertions),
            "性能計測は --release で実行してください",
        )?;
        Ok(Some(o))
    }

    fn ensure(condition: bool, message: &str) -> Check<()> {
        if condition {
            Ok(())
        } else {
            Err(message.to_owned().into())
        }
    }

    fn new_file(path: &Path) -> Check<File> {
        Ok(OpenOptions::new().write(true).create_new(true).open(path)?)
    }

    fn write_json(path: &Path, value: &impl Serialize) -> Check<()> {
        let mut file = BufWriter::new(new_file(path)?);
        serde_json::to_writer_pretty(&mut file, value)?;
        writeln!(file)?;
        file.flush()?;
        Ok(())
    }

    fn csv_cell(value: impl std::fmt::Display) -> String {
        format!("\"{}\"", value.to_string().replace('"', "\"\""))
    }

    fn nanos(start: Clock) -> u64 {
        start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }

    #[derive(Serialize)]
    struct Sample {
        schema_version: u8,
        size: u64,
        distribution: String,
        trial: usize,
        phase: &'static str,
        operation: &'static str,
        sample: usize,
        live_tasks_before: u64,
        processing_ns: u64,
        verification_ns: u64,
        queue_wait_ns: Option<u64>,
        presentation_ns: Option<u64>,
        api_succeeded: bool,
        verified: bool,
        deadline_exceeded: bool,
        status: &'static str,
        error: Option<String>,
        rows: Option<usize>,
        total: Option<u64>,
    }

    struct Recorder {
        json: BufWriter<File>,
        csv: BufWriter<File>,
        size: u64,
        distribution: String,
        trial: usize,
        phase: &'static str,
        index: usize,
        timeout_ns: u64,
        failures: usize,
        timeouts: usize,
    }

    impl Recorder {
        fn new(run: &Path, o: &Options) -> Check<Self> {
            let mut csv = BufWriter::new(new_file(&run.join("core-samples.csv"))?);
            writeln!(
                csv,
                "schema_version,size,distribution,trial,phase,operation,sample,live_tasks_before,processing_ns,verification_ns,queue_wait_ns,presentation_ns,api_succeeded,verified,deadline_exceeded,status,error,rows,total"
            )?;
            Ok(Self {
                json: BufWriter::new(new_file(&run.join("core-samples.jsonl"))?),
                csv,
                size: 0,
                distribution: distribution(o).into(),
                trial: 0,
                phase: "warmup",
                index: 0,
                timeout_ns: o.timeout_ms * 1_000_000,
                failures: 0,
                timeouts: 0,
            })
        }

        fn measure<T>(
            &mut self,
            store: &mut Store,
            operation: &'static str,
            live: u64,
            work: impl FnOnce(&mut Store) -> Result<T>,
            verify: impl FnOnce(&mut Store, &T) -> Check<(usize, u64)>,
        ) -> Check<T> {
            let start = Clock::now();
            let result = work(store);
            let processing_ns = nanos(start);
            let verification = Clock::now();
            let check = match &result {
                Ok(value) => verify(store, value),
                Err(error) => Err(format!("core: {error:?}").into()),
            };
            let verification_ns = nanos(verification);
            let deadline_exceeded = processing_ns > self.timeout_ns;
            let status = if check.is_err() {
                "failure"
            } else if deadline_exceeded {
                "timeout"
            } else {
                "success"
            };
            self.failures += usize::from(status == "failure");
            self.timeouts += usize::from(status == "timeout");
            let sample = Sample {
                schema_version: 1,
                size: self.size,
                distribution: self.distribution.clone(),
                trial: self.trial,
                phase: self.phase,
                operation,
                sample: self.index,
                live_tasks_before: live,
                processing_ns,
                verification_ns,
                queue_wait_ns: None,
                presentation_ns: None,
                api_succeeded: result.is_ok(),
                verified: check.is_ok(),
                deadline_exceeded,
                status,
                error: check.as_ref().err().map(ToString::to_string),
                rows: check.as_ref().ok().map(|v| v.0),
                total: check.as_ref().ok().map(|v| v.1),
            };
            serde_json::to_writer(&mut self.json, &sample)?;
            writeln!(self.json)?;
            writeln!(
                self.csv,
                "1,{},{},{},{},{},{},{},{},{},,,{},{},{},{},{},{},{}",
                sample.size,
                csv_cell(&sample.distribution),
                sample.trial,
                sample.phase,
                sample.operation,
                sample.sample,
                sample.live_tasks_before,
                processing_ns,
                verification_ns,
                sample.api_succeeded,
                sample.verified,
                deadline_exceeded,
                status,
                csv_cell(sample.error.as_deref().unwrap_or("")),
                sample.rows.map(|v| v.to_string()).unwrap_or_default(),
                sample.total.map(|v| v.to_string()).unwrap_or_default()
            )?;
            self.json.flush()?;
            self.csv.flush()?;
            check?;
            Ok(result?)
        }
    }

    fn distribution(o: &Options) -> &'static str {
        match o.distribution {
            fixtures::Distribution::ThirtyDays => "spread",
            fixtures::Distribution::DenseDay => "dense",
        }
    }

    fn task_query(window: &CalendarWindow) -> TaskQuery {
        TaskQuery {
            day: window.clone(),
            list: None,
            status: StatusFilter::Any,
            due: DueFilter::Any,
            blocks: BlockFilter::Any,
            title_contains: None,
        }
    }

    fn first_tasks() -> Result<TaskPageRequest> {
        Ok(TaskPageRequest {
            size: PageSize::new(200)?,
            position: TaskPosition::First,
        })
    }

    fn count(store: &mut Store, window: &CalendarWindow, expected: u64) -> Check<()> {
        let result = store.task_rows(task_query(window), &[])?;
        ensure(
            result.total == expected && result.rows.is_empty() && result.matching_ids.is_empty(),
            "生存Task件数またはCOUNT応答が不正です",
        )
    }

    fn all_tasks(store: &mut Store, query: TaskQuery, size: u64) -> Check<Vec<TaskRow>> {
        let mut request = first_tasks()?;
        let mut rows = Vec::new();
        let mut ids = HashSet::new();
        loop {
            let page = store.tasks(query.clone(), request)?;
            ensure(page.total == size, "fixtureのTask件数が一致しません")?;
            for row in page.items {
                ensure(ids.insert(row.task.key.id), "Task頁が重複しています")?;
                rows.push(row);
            }
            match page.next {
                Some(next) => {
                    ensure(
                        rows.len() < size as usize,
                        "Task cursorが終端を超えています",
                    )?;
                    request = TaskPageRequest {
                        size: PageSize::new(200)?,
                        position: TaskPosition::After(next),
                    };
                }
                None => break,
            }
        }
        ensure(rows.len() as u64 == size, "Task頁に欠落があります")?;
        Ok(rows)
    }

    fn agenda_key(item: &AgendaItem) -> (i64, u8, [u8; 16]) {
        match item {
            AgendaItem::TimeBlock { block, .. } => (
                block.data.span.bounds().0.unix_micros(),
                2,
                *block.key.id.as_bytes(),
            ),
            AgendaItem::Event(event) => {
                let start = match &event.data.when {
                    EventWhen::Timed(span) => span.bounds().0.unix_micros(),
                    EventWhen::AllDay(span) => {
                        let (y, m, d) = span.bounds().0.ymd();
                        i64::from(y) * 10000 + i64::from(m) * 100 + i64::from(d)
                    }
                };
                (start, 1, *event.key.id.as_bytes())
            }
        }
    }

    fn all_agenda(store: &mut Store, query: AgendaQuery, expected: u64) -> Check<Vec<AgendaItem>> {
        let mut request = PageRequest::first(PageSize::new(200)?);
        let mut items = Vec::new();
        let mut ids = HashSet::new();
        loop {
            let page = store.agenda(query.clone(), request)?;
            ensure(
                page.total == expected,
                "fixtureのEvent/TimeBlock件数が一致しません",
            )?;
            for item in page.items {
                let (_, kind, id) = agenda_key(&item);
                ensure(ids.insert((kind, id)), "agenda頁が重複しています")?;
                items.push(item);
            }
            match page.next {
                Some(next) => {
                    ensure(
                        items.len() < expected as usize,
                        "agenda cursorが終端を超えています",
                    )?;
                    request = PageRequest {
                        size: PageSize::new(200)?,
                        position: PagePosition::After(next),
                    };
                }
                None => break,
            }
        }
        ensure(items.len() as u64 == expected, "agenda頁に欠落があります")?;
        Ok(items)
    }

    fn overlaps(item: &AgendaItem, window: &CalendarWindow) -> bool {
        match item {
            AgendaItem::Event(Event {
                data:
                    EventData {
                        when: EventWhen::AllDay(span),
                        ..
                    },
                ..
            }) => {
                let (a, b) = span.bounds();
                let (c, d) = window.days().bounds();
                a < d && b > c
            }
            _ => {
                let (a, b) = match item {
                    AgendaItem::TimeBlock { block, .. } => block.data.span.bounds(),
                    AgendaItem::Event(Event {
                        data:
                            EventData {
                                when: EventWhen::Timed(span),
                                ..
                            },
                        ..
                    }) => span.bounds(),
                    _ => return false,
                };
                let (c, d) = window.instant_bounds();
                c < d && a < d && b > c
            }
        }
    }

    struct Baseline {
        tasks: Vec<TaskRow>,
        timed: Vec<AgendaItem>,
        all_day: Vec<AgendaItem>,
    }

    fn baseline(
        store: &mut Store,
        o: &Options,
        size: u64,
        window: &CalendarWindow,
    ) -> Check<Baseline> {
        let tasks = all_tasks(store, task_query(window), size)?;
        let wide =
            CalendarWindow::new(DateSpan::new(o.date, o.date.add_days(32)?)?, o.zone.clone())?;
        let timed = all_agenda(
            store,
            AgendaQuery {
                range: AgendaRange::Timed(wide.timed()),
                calendar: None,
            },
            size + size / 10 * 4 / 5,
        )?;
        let all_day = all_agenda(
            store,
            AgendaQuery {
                range: AgendaRange::AllDay(wide.days()),
                calendar: None,
            },
            size / 50,
        )?;
        let mut parents = HashSet::new();
        for item in &timed {
            if let AgendaItem::TimeBlock { block, task } = item {
                let parent = tasks
                    .iter()
                    .find(|row| row.task.key.id == block.task)
                    .ok_or("TimeBlockの親がありません")?;
                ensure(
                    parents.insert(block.task)
                        && task.key == parent.task.key
                        && task.title == parent.task.data.title
                        && task.status == parent.task.data.status,
                    "fixtureの親子投影が不正です",
                )?;
            }
        }
        ensure(
            parents.len() as u64 == size,
            "fixtureはTaskごとに一枠必要です",
        )?;
        let select = |items: Vec<AgendaItem>| {
            let mut items: Vec<_> = items
                .into_iter()
                .filter(|item| overlaps(item, window))
                .collect();
            items.sort_by_key(agenda_key);
            items
        };
        Ok(Baseline {
            tasks,
            timed: select(timed),
            all_day: select(all_day),
        })
    }

    fn query_samples(
        store: &mut Store,
        log: &mut Recorder,
        baseline: &Baseline,
        window: &CalendarWindow,
        o: &Options,
    ) -> Check<()> {
        let size = log.size;
        let generation = store.task_rows(task_query(window), &[])?.generation;
        let cases = ["日本語", "資料", "_%"];
        let expected: Vec<Vec<TaskRow>> = cases
            .iter()
            .map(|text| {
                baseline
                    .tasks
                    .iter()
                    .filter(|row| row.task.data.title.as_str().contains(text))
                    .cloned()
                    .collect()
            })
            .collect();
        for i in 0..(o.warmup + o.samples) {
            log.phase = if i < o.warmup { "warmup" } else { "sample" };
            log.index = if i < o.warmup { i } else { i - o.warmup };
            let case = log.index % cases.len();
            let mut query = task_query(window);
            query.title_contains = Some(SearchText::new(cases[case])?);
            let matches = &expected[case];
            ensure(!matches.is_empty(), "検索fixtureが空です")?;
            let request = first_tasks()?;
            let search = query.clone();
            log.measure(
                store,
                "search_and_count",
                size,
                |s| s.tasks(search, request),
                |_, page| {
                    ensure(
                        page.generation == generation
                            && page.total == matches.len() as u64
                            && page.items == matches[..matches.len().min(200)]
                            && page.previous.is_none()
                            && page.next.is_some() == (matches.len() > 200),
                        "検索頁またはCOUNTが期待結果と違います",
                    )?;
                    Ok((page.items.len(), page.total))
                },
            )?;
            log.measure(
                store,
                "count_only",
                size,
                |s| s.task_rows(query, &[]),
                |_, result| {
                    ensure(
                        result.generation == generation
                            && result.total == matches.len() as u64
                            && result.rows.is_empty()
                            && result.matching_ids.is_empty(),
                        "COUNTが期待結果と違います",
                    )?;
                    Ok((0, result.total))
                },
            )?;
            for (name, range, items) in [
                (
                    "agenda_timed",
                    AgendaRange::Timed(window.timed()),
                    &baseline.timed,
                ),
                (
                    "agenda_all_day",
                    AgendaRange::AllDay(window.days()),
                    &baseline.all_day,
                ),
            ] {
                let request = PageRequest::first(PageSize::new(200)?);
                log.measure(
                    store,
                    name,
                    size,
                    |s| {
                        s.agenda(
                            AgendaQuery {
                                range,
                                calendar: None,
                            },
                            request,
                        )
                    },
                    |_, page| {
                        ensure(
                            page.generation == generation
                                && page.total == items.len() as u64
                                && page.items == items[..items.len().min(200)]
                                && page.previous.is_none()
                                && page.next.is_some() == (items.len() > 200),
                            "agendaの値・順序・COUNTが期待結果と違います",
                        )?;
                        Ok((page.items.len(), page.total))
                    },
                )?;
            }
        }
        Ok(())
    }

    fn changed<T>(commit: &Commit<T>) -> Check<()> {
        ensure(
            commit.operation.is_some() && !commit.changed.is_empty(),
            "実変更のない保存を検出しました",
        )
    }

    fn verify_task(
        store: &mut Store,
        commit: &Commit<Task>,
        data: &TaskData,
        old: Option<&Task>,
        blocks: &[TimeBlock],
    ) -> Check<(usize, u64)> {
        changed(commit)?;
        ensure(
            &commit.value.data == data
                && old.is_none_or(|before| {
                    before.key.id == commit.value.key.id
                        && before.key.revision < commit.value.key.revision
                        && before.data != *data
                }),
            "Taskが意図どおり変更されていません",
        )?;
        let detail = store.task_detail(commit.value.key.id)?;
        ensure(
            detail.task == commit.value && detail.blocks == blocks,
            "保存済みTaskまたは既存枠が不正です",
        )?;
        ensure(commit.changed.iter().any(|change| matches!(change, Change::Upsert(Entity::Task(task)) if task == &detail.task)), "変更通知にTaskがありません")?;
        Ok((commit.changed.len(), 1))
    }

    fn replay_task(
        store: &mut Store,
        commit: &Commit<()>,
        before: &Task,
        data: &TaskData,
        blocks: &[TimeBlock],
    ) -> Check<(usize, u64)> {
        changed(commit)?;
        let detail = store.task_detail(before.key.id)?;
        ensure(
            detail.task.data == *data
                && detail.task.key.revision > before.key.revision
                && detail.blocks == blocks,
            "Undo/Redo後の値・版・枠が不正です",
        )?;
        ensure(commit.changed.iter().any(|change| matches!(change, Change::Upsert(Entity::Task(task)) if task == &detail.task)), "Undo/Redo通知が保存値と違います")?;
        Ok((commit.changed.len(), 1))
    }

    fn updated_task(store: &mut Store, id: TaskId) -> Check<Task> {
        match store.get(EntityId::Task(id))? {
            Entity::Task(task) => Ok(task),
            _ => Err("Taskでない応答です".into()),
        }
    }

    fn undo_target<T>(commit: &Commit<T>) -> Check<UndoTarget> {
        Ok(commit
            .history
            .undo
            .as_ref()
            .ok_or("Undo対象がありません")?
            .target
            .clone())
    }

    fn mutation_samples(
        store: &mut Store,
        log: &mut Recorder,
        baseline: &Baseline,
        window: &CalendarWindow,
        o: &Options,
    ) -> Check<()> {
        let original = baseline
            .tasks
            .iter()
            .find(|row| row.task.data.status == TaskStatus::Open)
            .ok_or("未完了Taskがありません")?
            .task
            .clone();
        let mut task = original.clone();
        let blocks = store.task_detail(task.key.id)?.blocks;
        ensure(blocks.len() == 1, "二枠試行の初期枠数が不正です")?;
        let size = log.size;
        for i in 0..(o.warmup + o.samples) {
            log.phase = if i < o.warmup { "warmup" } else { "sample" };
            log.index = if i < o.warmup { i } else { i - o.warmup };
            count(store, window, size)?;
            let mut bytes = [0u8; 16];
            bytes[..8].copy_from_slice(&o.seed.to_be_bytes());
            bytes[6] = 0xf0;
            bytes[8..].copy_from_slice(&(i as u64 + 1).to_be_bytes());
            let id = TaskId::from_bytes(bytes)?;
            let data = TaskData {
                title: Title::new(&format!("計測用作成 {i:08}"))?,
                list: store.info().default_list,
                status: TaskStatus::Open,
                due: Some(Due::Date(o.date)),
            };
            let edit = Save::Create {
                id,
                data: data.clone(),
            };
            let created = log.measure(
                store,
                "create",
                size,
                |s| s.save_task(edit),
                |s, commit| {
                    ensure(commit.value.key.id == id, "作成IDが一致しません")?;
                    let work = verify_task(s, commit, &data, None, &[])?;
                    count(s, window, size + 1)?;
                    Ok(work)
                },
            )?;
            let deleted = store.delete(DeleteTarget::Task(created.value.key))?;
            changed(&deleted)?;
            ensure(
                matches!(store.get(EntityId::Task(id)), Err(Error::NotFound(EntityId::Task(found))) if found == id),
                "作成Taskの後片付けに失敗しました",
            )?;
            count(store, window, size)?;

            let mut data = task.data.clone();
            data.title = Title::new(&format!("計測用編集 {i:08}"))?;
            let edit = Save::Update {
                expected: task.key,
                data: data.clone(),
            };
            let edited = log.measure(
                store,
                "edit",
                size,
                |s| s.save_task(edit),
                |s, commit| verify_task(s, commit, &data, Some(&task), &blocks),
            )?;
            let mut done = edited.value.data.clone();
            done.status = TaskStatus::Done;
            let edit = Save::Update {
                expected: edited.value.key,
                data: done.clone(),
            };
            let completed = log.measure(
                store,
                "complete",
                size,
                |s| s.save_task(edit),
                |s, commit| verify_task(s, commit, &done, Some(&edited.value), &blocks),
            )?;
            let undo = undo_target(&completed)?;
            let undone = log.measure(
                store,
                "undo",
                size,
                |s| s.undo(undo),
                |s, commit| replay_task(s, commit, &completed.value, &edited.value.data, &blocks),
            )?;
            let reopened = updated_task(store, task.key.id)?;
            let redo = undone
                .history
                .redo
                .as_ref()
                .ok_or("Redo対象がありません")?
                .target
                .clone();
            let redone = log.measure(
                store,
                "redo",
                size,
                |s| s.redo(redo),
                |s, commit| replay_task(s, commit, &reopened, &done, &blocks),
            )?;
            let reset = store.undo(undo_target(&redone)?)?;
            changed(&reset)?;
            task = updated_task(store, task.key.id)?;
            ensure(
                task.data == edited.value.data,
                "完了試行を未完了へ復元できませんでした",
            )?;

            let block_id = TimeBlockId::from_bytes(bytes)?;
            let block_data = blocks[0].data.clone();
            let edit = Save::Create {
                id: block_id,
                data: block_data.clone(),
            };
            let saved = log.measure(
                store,
                "second_block",
                size,
                |s| s.save_time_block(task.key, edit),
                |s, commit| {
                    changed(commit)?;
                    let detail = s.task_detail(task.key.id)?;
                    ensure(
                        detail.task == commit.value.task
                            && detail.task.data == task.data
                            && detail.task.key.revision > task.key.revision
                            && detail.blocks.len() == 2
                            && detail.blocks.contains(&blocks[0])
                            && detail.blocks.contains(&commit.value.block)
                            && commit.value.block.key.id == block_id
                            && commit.value.block.task == task.key.id
                            && commit.value.block.data == block_data,
                        "二枠が保存されていません",
                    )?;
                    count(s, window, size)?;
                    Ok((commit.changed.len(), 2))
                },
            )?;
            let restored = store.undo(undo_target(&saved)?)?;
            changed(&restored)?;
            let detail = store.task_detail(task.key.id)?;
            ensure(detail.blocks == blocks, "二枠試行の後片付けに失敗しました")?;
            task = detail.task;
            let reset = store.save_task(Save::Update {
                expected: task.key,
                data: original.data.clone(),
            })?;
            verify_task(store, &reset, &original.data, Some(&task), &blocks)?;
            task = reset.value;
            count(store, window, size)?;
        }
        Ok(())
    }

    fn trial(
        o: &Options,
        size: u64,
        number: usize,
        run: &Path,
        log: &mut Recorder,
        setup: &mut BufWriter<File>,
    ) -> Check<()> {
        let directory = run.join(format!("db-{size}-trial-{number:02}"));
        fs::create_dir(&directory)?;
        let db = directory.join("fixture.sqlite");
        let start = Clock::now();
        let report = fixtures::seed_empty(
            &db,
            fixtures::Config {
                profile: if size == 1000 {
                    fixtures::Profile::Thousand
                } else {
                    fixtures::Profile::TenThousand
                },
                distribution: o.distribution,
                seed: o.seed,
                day: o.date,
                zone: o.zone.clone(),
            },
        )?;
        let fixture_ns = nanos(start);
        ensure(
            report.tasks == size
                && report.time_blocks == size
                && report.events == size / 10
                && report.seed == o.seed,
            "fixture生成件数が一致しません",
        )?;
        let start = Clock::now();
        let mut store = Store::open(&db)?;
        let window =
            CalendarWindow::new(DateSpan::new(o.date, o.date.add_days(1)?)?, o.zone.clone())?;
        let baseline = baseline(&mut store, o, size, &window)?;
        serde_json::to_writer(
            &mut *setup,
            &json!({
                "size": size, "trial": number, "db": db, "fixture_ns": fixture_ns,
                "oracle_setup_ns": nanos(start), "tasks": report.tasks,
                "time_blocks": report.time_blocks, "events": report.events,
                "agenda_timed_matches": baseline.timed.len(), "agenda_all_day_matches": baseline.all_day.len(),
                "excluded_from_samples": true
            }),
        )?;
        writeln!(setup)?;
        setup.flush()?;
        log.size = size;
        log.trial = number;
        query_samples(&mut store, log, &baseline, &window, o)?;
        mutation_samples(&mut store, log, &baseline, &window, o)?;
        count(&mut store, &window, size)?;
        drop(store);
        let mut reopened = Store::open(&db)?;
        count(&mut reopened, &window, size)?;
        Ok(())
    }

    pub fn run() -> Check<()> {
        let Some(o) = options()? else { return Ok(()) };
        fs::create_dir_all(&o.out)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let run = o.out.join(format!("core-{stamp}-{}", std::process::id()));
        fs::create_dir(&run)?;
        let environment = match &o.environment {
            Some(path) => serde_json::from_reader::<_, Value>(File::open(path)?)?,
            None => {
                json!({"hardware": null, "note": "詳細環境はperformance.ps1経由で取得。直接実行では未観測"})
            }
        };
        let rustc = std::process::Command::new("rustc")
            .args(["--version", "--verbose"])
            .output()?;
        ensure(rustc.status.success(), "rustc環境情報の取得に失敗しました")?;
        write_json(
            &run.join("run.json"),
            &json!({
                "schema_version": 1, "kind": "core", "sizes": o.sizes, "seed": o.seed,
                "date": o.date.iso(), "zone": o.zone.as_str(), "distribution": distribution(&o),
                "warmup": o.warmup, "samples": o.samples, "trials": o.trials,
                "operations": OPERATIONS, "timeout_ms": o.timeout_ms,
                "timeout_policy": "同期呼出し完了後の処理時間超過。キャンセルしない。QueryBudgetExceededはfailure",
                "profile": "release", "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
                "logical_processors": std::thread::available_parallelism()?.get(),
                "rustc": String::from_utf8_lossy(&rustc.stdout), "rustflags": std::env::var("RUSTFLAGS").ok(),
                "cargo_encoded_rustflags": std::env::var("CARGO_ENCODED_RUSTFLAGS").ok(),
                "executable": std::env::current_exe()?, "argv": std::env::args().collect::<Vec<_>>(),
                "harness_source": Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/measure.rs"),
                "lock_file": Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
                "environment": environment, "queue_wait": "NOT_OBSERVED", "presentation": "NOT_OBSERVED"
            }),
        )?;
        let mut log = Recorder::new(&run, &o)?;
        let mut setup = BufWriter::new(new_file(&run.join("fixture-setup.jsonl"))?);
        let mut trials = Vec::new();
        for number in 1..=o.trials {
            let sizes: Vec<_> = if number % 2 == 0 {
                o.sizes.iter().rev().copied().collect()
            } else {
                o.sizes.clone()
            };
            for size in sizes {
                let result = trial(&o, size, number, &run, &mut log, &mut setup);
                trials.push(json!({"size": size, "trial": number, "completed": result.is_ok(), "error": result.err().map(|e| e.to_string())}));
                write_json(
                    &run.join(format!("trial-{size}-{number:02}.json")),
                    trials.last().ok_or("試行結果がありません")?,
                )?;
            }
        }
        let completed = trials.iter().all(|t| t["completed"] == true);
        write_json(
            &run.join("completion.json"),
            &json!({"completed": completed, "failures": log.failures, "timeouts": log.timeouts, "trials": trials}),
        )?;
        println!("CORE_SAMPLES: {}", run.display());
        println!(
            "SUMMARY: python -I scripts/summarize-performance.py --input \"{}\"",
            run.display()
        );
        ensure(
            completed && log.failures == 0 && log.timeouts == 0,
            "失敗・期限超過・未完了の試行があります。生sampleと試行JSONを確認してください",
        )
    }
}
