#![windows_subsystem = "windows"]

use slint::{ComponentHandle, ModelRc, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use ultratasks_core::*;

slint::include_modules!();

fn rows(store: &mut Store) -> Result<Vec<TaskItem>> {
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
    Ok(store
        .tasks(query, request)?
        .items
        .into_iter()
        .map(|row| TaskItem {
            title: row.task.data.title.as_str().into(),
            done: row.task.data.status == TaskStatus::Done,
        })
        .collect())
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

fn show(ui: &AppWindow, store: &mut Store) {
    match rows(store) {
        Ok(items) => ui.set_tasks(ModelRc::new(VecModel::from(items))),
        Err(error) => ui.set_status(error.to_string().into()),
    }
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA")?)
        .join("UltraTasks")
        .join("skeleton");
    std::fs::create_dir_all(&dir)?;
    // shortcut: 保存をUIスレッドで直接呼ぶ。骨格の判定が済んだら既存workerへ移す。
    let store = Rc::new(RefCell::new(Store::open(&dir.join("skeleton.sqlite"))?));

    let ui = AppWindow::new()?;
    show(&ui, &mut store.borrow_mut());

    let weak = ui.as_weak();
    ui.on_create(move |title| {
        let Some(ui) = weak.upgrade() else { return };
        let mut store = store.borrow_mut();
        match create(&mut store, &title) {
            Ok(()) => {
                ui.set_draft("".into());
                ui.set_status("".into());
                show(&ui, &mut store);
            }
            Err(error) => ui.set_status(error.to_string().into()),
        }
    });

    ui.run()?;
    Ok(())
}
