mod backend;
mod presentation;
mod theme;
mod views;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use chrono_tz::Tz;
use directories::ProjectDirs;
use gpui_kit::{
    component::{Theme, ThemeMode},
    *,
};

fn main() -> Result<()> {
    let mut database = None;
    let mut timezone: Tz = iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse().ok())
        .unwrap_or(chrono_tz::UTC);
    let mut motion = true;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--database") => {
                database = Some(PathBuf::from(
                    args.next().context("--database requires a path")?,
                ))
            }
            Some("--timezone") => {
                timezone = args
                    .next()
                    .context("--timezone requires an IANA timezone")?
                    .to_str()
                    .context("invalid timezone encoding")?
                    .parse()
                    .context("invalid IANA timezone")?
            }
            Some("--reduce-motion") => motion = false,
            Some("--help") => {
                println!(
                    "UltraTasks\n  --database PATH       SQLite database (default: OS application data)\n  --timezone IANA       Display timezone (default: system timezone)\n  --reduce-motion       Disable prototype animations"
                );
                return Ok(());
            }
            _ => bail!("Unknown option: {}", arg.to_string_lossy()),
        }
    }
    let database = database.unwrap_or_else(|| {
        ProjectDirs::from("app", "UltraTasks", "UltraTasks")
            .map(|dirs| dirs.data_local_dir().join("ultratasks.db"))
            .unwrap_or_else(|| PathBuf::from("ultratasks.db"))
    });
    if let Some(parent) = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let backend = backend::Backend::start(database)?;
    let tokens = theme::ThemeTokens::load()?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1120.), px(760.)), cx)),
                window_min_size: Some(size(px(880.), px(540.))),
                ..Default::default()
            };
            if let Err(error) = gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("UltraTasks");
                Theme::change(ThemeMode::Dark, Some(window), cx);
                cx.new(|cx| views::TasksView::new(backend, timezone, tokens, motion, window, cx))
            }) {
                eprintln!("Failed to open UltraTasks: {error}");
                cx.quit();
            }
            cx.activate(true);
        });
    Ok(())
}
