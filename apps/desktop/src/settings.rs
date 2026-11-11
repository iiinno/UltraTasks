use crate::theme::{self, MotionSpec, SystemColors, Tokens};
pub use crate::theme::{MotionMode, ThemeMode};
use gpui_kit::{App, WindowAppearance, component::Theme};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    pub theme: ThemeMode,
    pub motion: MotionMode,
}
impl Settings {
    pub fn load(db: &Path) -> Result<Self, String> {
        match std::fs::read(db.with_extension("ui.json")) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "表示設定を読めませんでした".into())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("表示設定を読めませんでした: {e}")),
        }
    }
    pub fn save(&self, db: &Path) -> Result<(), String> {
        use std::io::Write;
        let destination = db.with_extension("ui.json");
        let temporary = db.with_extension(format!("ui.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let mut file = std::fs::File::create(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(temporary, destination).map_err(|e| e.to_string())
    }
    pub fn apply(&self, cx: &mut App) -> (Tokens, MotionSpec) {
        let dark = matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let (os_reduced, contrast) = os_preferences();
        let tokens = theme::resolve(self.theme, dark, contrast.as_ref());
        let motion = theme::motion(self.motion, os_reduced);
        theme::apply(self.theme, dark, contrast.as_ref(), cx);
        let reduced = motion.fade_ms == 0 && motion.move_ms == 0;
        cx.set_reduce_motion(reduced);
        Theme::update(cx, |theme| {
            theme.font_family = "Yu Gothic UI".into();
            theme.focus_ring = false;
            theme.motion = Default::default();
            theme.cursor_blink = if reduced {
                gpui_kit::base::input::CursorBlink::steady()
            } else {
                gpui_kit::base::input::CursorBlink::default()
                    .with_idle_timeout(Duration::from_secs(3))
            };
            if reduced {
                theme.motion.duration_fast = Duration::ZERO;
                theme.motion.duration_normal = Duration::ZERO;
                theme.motion.duration_slow = Duration::ZERO;
                theme.motion.spring_control = gpui_kit::base::Spring::new(Duration::ZERO);
                theme.motion.spring_move = gpui_kit::base::Spring::new(Duration::ZERO);
            }
        });
        (tokens, motion)
    }
}

#[cfg(windows)]
fn os_preferences() -> (bool, Option<SystemColors>) {
    use gpui_kit::{Hsla, rgb};
    use windows::Win32::{
        Graphics::Gdi::{
            COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWFRAME,
            COLOR_WINDOWTEXT, GetSysColor,
        },
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::{
                SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST,
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
            },
        },
    };
    let mut animations = 1u32;
    let mut contrast = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut animations as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let valid = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok();
        let colors = (valid && contrast.dwFlags.contains(HCF_HIGHCONTRASTON)).then(|| {
            let color = |index| -> Hsla {
                let c = GetSysColor(index);
                rgb(((c & 255) << 16) | (c & 0xff00) | ((c >> 16) & 255)).into()
            };
            SystemColors {
                canvas: color(COLOR_WINDOW),
                text: color(COLOR_WINDOWTEXT),
                highlight: color(COLOR_HIGHLIGHT),
                highlight_text: color(COLOR_HIGHLIGHTTEXT),
                disabled_text: color(COLOR_GRAYTEXT),
                border: color(COLOR_WINDOWFRAME),
            }
        });
        (animations == 0, colors)
    }
}
#[cfg(not(windows))]
fn os_preferences() -> (bool, Option<SystemColors>) {
    (false, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_survive_replacement_without_touching_database() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("settings-test.sqlite");
        std::fs::write(&db, b"untouched").unwrap();
        Settings {
            theme: ThemeMode::Dark,
            motion: MotionMode::None,
        }
        .save(&db)
        .unwrap();
        assert_eq!(Settings::load(&db).unwrap().theme, ThemeMode::Dark);
        Settings {
            theme: ThemeMode::Light,
            motion: MotionMode::Reduced,
        }
        .save(&db)
        .unwrap();
        let restored = Settings::load(&db).unwrap();
        assert_eq!(restored.theme, ThemeMode::Light);
        assert_eq!(restored.motion, MotionMode::Reduced);
        assert_eq!(std::fs::read(db).unwrap(), b"untouched");
    }
}
