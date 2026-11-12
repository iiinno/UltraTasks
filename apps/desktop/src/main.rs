mod app;
mod cli;
mod diagnostics;
mod presentation;
mod settings;
mod theme;
mod views;
mod worker;

use gpui_kit::{assets::AllAssets, *};
use std::{borrow::Cow, cell::Cell, process::ExitCode, rc::Rc, time::Duration};

fn main() -> ExitCode {
    let options = match cli::Options::parse(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{}", cli::USAGE);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    let (diagnostics, log) = match diagnostics::Diagnostics::start(options.diagnostics.as_deref()) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("計測記録を開始できません: {e}");
            return ExitCode::FAILURE;
        }
    };
    let worker = match worker::Worker::start(options.clone(), diagnostics.clone()) {
        Ok(worker) => worker,
        Err(e) => {
            eprintln!("保存担当を開始できません: {e}");
            return ExitCode::FAILURE;
        }
    };
    let client = worker.client();
    let smoke = options.smoke;
    let smoke_passed = Rc::new(Cell::new(None));
    let result = smoke_passed.clone();
    let launch_failed = Rc::new(Cell::new(false));
    let failure = launch_failed.clone();
    let app_diagnostics = diagnostics.clone();
    gpui_kit::application()
        .with_assets(AllAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            let fonts: Vec<Cow<'static, [u8]>> = vec![
                Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Medium.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/Inter-Bold.ttf")),
            ];
            if let Err(error) = cx.text_system().add_fonts(fonts) {
                eprintln!("Interフォントを登録できません: {error}");
                failure.set(true);
                cx.quit();
                return;
            }
            gpui_kit::init(cx);
            app::bind_keys(cx);
            let rendered = Rc::new(Cell::new(false));
            let frame = Some(rendered.clone());
            let options_window = WindowOptions {
                focus: false,
                window_bounds: Some(WindowBounds::centered(
                    size(
                        px(options.window_size[0] as f32),
                        px(options.window_size[1] as f32),
                    ),
                    cx,
                )),
                window_min_size: Some(size(px(620.), px(480.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("UltraTasks".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options_window, cx, |window, cx| {
                cx.new(|cx| app::Desktop::new(options, client, app_diagnostics, frame, window, cx))
            });
            let (window, _) = match opened {
                Ok(opened) => opened,
                Err(e) => {
                    eprintln!("ウィンドウを作れません: {e}");
                    failure.set(true);
                    cx.quit();
                    return;
                }
            };
            cx.activate(true);
            let _ = window.update(cx, |_, window, _| window.activate_window());
            if smoke {
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_secs(2)).await;
                    let visible = window
                        .update(cx, |_, window, _| {
                            let size = window.viewport_size();
                            eprintln!(
                                "SMOKE_CHECK: frame_callback={} visible={} viewport={size:?}",
                                rendered.get(),
                                window.is_visible()
                            );
                            window.is_visible() && size.width > px(0.) && size.height > px(0.)
                        })
                        .unwrap_or(false);
                    result.set(Some(rendered.get() && visible));
                    cx.update(|cx| cx.quit());
                })
                .detach();
            }
        });
    worker.shutdown_after_event_loop();
    drop(diagnostics);
    log.join();
    if launch_failed.get() {
        return ExitCode::FAILURE;
    }
    if smoke {
        if smoke_passed.get() == Some(true) {
            println!(
                "SMOKE_OK: native window created, frame callback observed, event loop returned; visual/IME/storage acceptance NOT CHECKED"
            );
        } else {
            eprintln!("SMOKE_FAIL: native lifecycle check did not complete");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
