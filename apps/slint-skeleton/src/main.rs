#![windows_subsystem = "windows"]

use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use ultratasks_core::*;

slint::include_modules!();

struct State {
    store: Store,
    rows: Vec<TaskRow>,
}

fn load(store: &mut Store) -> Result<Vec<TaskRow>> {
    let zone = ZoneId::parse("Asia/Tokyo")?;
    let day = Date::today(&zone)?;
    let query = TaskQuery {
        day: CalendarWindow::new(DateSpan::new(day, day.add_days(1)?)?, zone)?,
        list: None,
        status: StatusFilter::Any,
        due: DueFilter::Any,
        blocks: BlockFilter::Any,
        title_contains: None,
    };
    let request = TaskPageRequest {
        size: PageSize::new(MAX_PAGE_SIZE)?,
        position: TaskPosition::First,
    };
    Ok(store.tasks(query, request)?.items)
}

fn create(store: &mut Store, title: &str) -> Result<()> {
    let data = TaskData {
        title: Title::new(title)?,
        list: store.info().default_list,
        status: TaskStatus::Open,
        due: None,
    };
    store.save_task(Save::Create {
        id: TaskId::new()?,
        data,
    })?;
    Ok(())
}

fn toggle(store: &mut Store, row: &TaskRow) -> Result<()> {
    let mut data = row.task.data.clone();
    data.status = match data.status {
        TaskStatus::Open => TaskStatus::Done,
        TaskStatus::Done => TaskStatus::Open,
    };
    store.save_task(Save::Update {
        expected: row.task.key,
        data,
    })?;
    Ok(())
}

// 保存の成否にかかわらず一覧を読み直し、画面をDBの状態へ合わせる。
fn finish(ui: &AppWindow, state: &mut State, saved: Result<()>) {
    let loaded = load(&mut state.store).map(|rows| state.rows = rows);
    let default_list = state.store.info().default_list;
    let items: Vec<TaskItem> = state
        .rows
        .iter()
        .map(|row| TaskItem {
            title: row.task.data.title.as_str().into(),
            // 既定リストの名前は全行で同じになるので出さない。
            meta: if row.task.data.list == default_list {
                "".into()
            } else {
                row.list_title.as_str().into()
            },
            done: row.task.data.status == TaskStatus::Done,
        })
        .collect();
    ui.set_tasks(ModelRc::new(VecModel::from(items)));
    ui.set_status(match saved.and(loaded) {
        Ok(()) => "".into(),
        Err(error) => error.to_string().into(),
    });
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA")?)
        .join("UltraTasks")
        .join("skeleton");
    std::fs::create_dir_all(&dir)?;
    // shortcut: 保存をUIスレッドで直接呼ぶ。骨格の判定が済んだら既存workerへ移す。
    let state = Rc::new(RefCell::new(State {
        store: Store::open(&dir.join("skeleton.sqlite"))?,
        rows: Vec::new(),
    }));

    let ui = AppWindow::new()?;
    finish(&ui, &mut state.borrow_mut(), Ok(()));

    let (weak, shared) = (ui.as_weak(), state.clone());
    ui.on_create(move |title| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = shared.borrow_mut();
        let saved = create(&mut state.store, &title);
        if saved.is_ok() {
            ui.set_draft("".into());
        }
        finish(&ui, &mut state, saved);
    });

    let weak = ui.as_weak();
    ui.on_toggle(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(row) = usize::try_from(index).ok().and_then(|i| state.rows.get(i).cloned())
        else {
            return;
        };
        let saved = toggle(&mut state.store, &row);
        finish(&ui, &mut state, saved);
    });

    ui.run()?;
    Ok(())
}
