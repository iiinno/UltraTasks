//! OS設定はdesktopから受け取り、色と寸法だけを解決する。

mod motion;
mod tokens;

pub use motion::{MotionMode, MotionSpec, motion};
pub use tokens::{BODY_WEIGHT, CONTROL_WEIGHT, SystemColors, Tokens, resolve};

use std::sync::Arc;

use gpui_kit::{
    App, Font, FontFallbacks,
    component::{Theme, ThemeMode as KitThemeMode, ThemeTokens},
    font, px,
};

pub fn ui_font() -> Font {
    Font {
        fallbacks: Some(FontFallbacks::from_fonts(vec!["Yu Gothic UI".into()])),
        weight: BODY_WEIGHT,
        ..font("Inter")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    fn is_dark(self, system_dark: bool) -> bool {
        match self {
            Self::System => system_dark,
            Self::Light => false,
            Self::Dark => true,
        }
    }
}

/// `gpui_kit::init`後に呼ぶ。入力Entity・選択・scrollには触れない。
pub fn apply(mode: ThemeMode, system_dark: bool, contrast: Option<&SystemColors>, cx: &mut App) {
    let tokens = resolve(mode, system_dark, contrast);
    let kit_mode = if mode.is_dark(system_dark) {
        KitThemeMode::Dark
    } else {
        KitThemeMode::Light
    };
    // modeと色を同一updateで変更すると、Kitが登録済みの配色で上書きする。
    Theme::change(kit_mode, None, cx);
    Theme::update(cx, |theme| apply_tokens(theme, &tokens, contrast));
}

fn apply_tokens(theme: &mut Theme, t: &Tokens, contrast: Option<&SystemColors>) {
    theme.font_family = ui_font().family;
    theme.font_size = px(t.body_size);
    theme.radius = px(t.control_radius);
    theme.radius_lg = px(12.);
    theme.shadow = contrast.is_none();
    theme.focus_ring = true;
    let editor = &mut Arc::make_mut(&mut theme.highlight_theme).style;
    editor.editor_background = Some(t.surface);
    editor.editor_foreground = Some(t.text_primary);
    editor.editor_active_line = Some(t.surface);
    editor.editor_gutter_background = Some(t.surface);
    editor.editor_line_number = Some(t.text_secondary);
    editor.editor_active_line_number = Some(t.text_primary);
    editor.editor_invisible = Some(t.text_secondary);

    let c = &mut theme.colors;
    c.background = t.canvas;
    c.foreground = t.text_primary;
    c.border = t.border;
    c.input = t.border;
    c.ring = t.focus_ring;
    c.caret = t.text_primary;
    c.accent = t.surface_hover;
    c.accent_foreground = t.text_primary;
    c.muted = t.surface_hover;
    c.muted_foreground = contrast.map_or(t.text_secondary, |system| system.disabled_text);
    c.popover = t.surface;
    c.popover_foreground = t.text_primary;
    c.primary = t.action_background;
    c.primary_foreground = t.action_text;
    c.primary_hover = t.action_hover;
    c.primary_active = t.action_hover;
    c.secondary = t.surface;
    c.secondary_foreground = t.text_primary;
    c.secondary_hover = t.surface_hover;
    c.secondary_active = t.selection_background;
    // Inputは選択中の文字色を指定できない。OS色の薄い塗りで通常文字を残す。
    c.selection = if contrast.is_some() {
        t.selection_background.alpha(0.35)
    } else {
        t.selection_background
    };
    c.button = t.surface;
    c.button_foreground = t.text_primary;
    c.button_hover = t.surface_hover;
    c.button_active = t.surface_hover;
    c.button_primary = t.action_background;
    c.button_primary_foreground = t.action_text;
    c.button_primary_hover = t.action_hover;
    c.button_primary_active = t.action_hover;
    c.button_secondary = t.surface;
    c.button_secondary_foreground = t.text_primary;
    c.button_secondary_hover = t.surface_hover;
    c.button_secondary_active = t.surface_hover;
    // Kitのdangerは入力の不正を示す線にも使われるため、読み取れる濃い側を使う。
    c.danger = t.error_text;
    c.danger_foreground = t.error_background;
    c.danger_hover = t.error_text;
    c.danger_active = t.error_text;
    c.button_danger = t.error_background;
    c.button_danger_foreground = t.error_text;
    c.button_danger_hover = t.error_background;
    c.button_danger_active = t.error_background;
    c.warning = t.warning_background;
    c.warning_foreground = t.warning_text;
    c.warning_hover = t.warning_background;
    c.warning_active = t.warning_background;
    c.button_warning = t.warning_background;
    c.button_warning_foreground = t.warning_text;
    c.button_warning_hover = t.warning_background;
    c.button_warning_active = t.warning_background;
    c.list = t.canvas;
    c.list_even = t.canvas;
    c.list_head = t.surface;
    c.list_hover = t.surface_hover;
    // Kit ListItemには選択専用の文字色がないのでInputと同じ安全な背景にする。
    c.list_active = c.selection;
    c.list_active_border = t.focus_ring;
    c.link = t.focus_ring;
    c.link_hover = t.focus_ring;
    c.link_active = t.focus_ring;
    c.drag_border = t.focus_ring;
    c.drop_target = t.selection_background;
    c.scrollbar = t.canvas;
    c.scrollbar_thumb = t.text_secondary;
    c.scrollbar_thumb_hover = t.text_primary;
    c.sidebar = t.sidebar;
    c.sidebar_foreground = t.text_secondary;
    c.sidebar_border = t.border;
    c.sidebar_accent = t.surface_hover;
    c.sidebar_accent_foreground = t.text_primary;
    c.sidebar_primary = t.selection_background;
    c.sidebar_primary_foreground = t.selection_text;
    c.tab = t.surface;
    c.tab_foreground = t.text_secondary;
    c.tab_bar = t.canvas;
    c.tab_bar_segmented = t.surface;
    c.tab_active = t.selection_background;
    c.tab_active_foreground = t.selection_text;
    c.title_bar = t.canvas;
    c.title_bar_border = t.border;
    c.status_bar = t.surface;
    c.status_bar_border = t.border;
    c.window_border = t.border;
    // changeでロードされた背景gradientも消し、全Kit表現をsolidの配色へ揃える。
    theme.tokens = ThemeTokens::from(&theme.colors);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Hsla, rgb};

    #[test]
    fn kit_palette_updates_component_tokens_and_semantic_projection_together() {
        let mut theme = Theme::default();
        for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::Dark] {
            let t = resolve(mode, false, None);
            apply_tokens(&mut theme, &t, None);
            assert_eq!(theme.tokens, ThemeTokens::from(&theme.colors));
            assert_eq!(theme.semantic_tokens().colors.background, t.canvas);
            assert_eq!(theme.semantic_tokens().colors.foreground, t.text_primary);
            assert_eq!(theme.semantic_tokens().colors.ring, t.focus_ring);
            assert_eq!(theme.tokens.button_primary.color, t.action_background);
            assert_eq!(theme.button_primary_foreground, t.action_text);
            assert_eq!(theme.primary_hover, t.action_hover);
            assert_eq!(theme.primary_active, t.action_hover);
            assert_eq!(theme.button_primary_hover, t.action_hover);
            assert_eq!(theme.button_primary_active, t.action_hover);
            assert_eq!(theme.popover, t.surface);
            assert_eq!(theme.tokens.list_hover.color, t.surface_hover);
            assert_eq!(theme.tokens.list_active.color, t.selection_background);
            assert_eq!(theme.button_danger_foreground, t.error_text);
            assert_eq!(theme.button_warning_foreground, t.warning_text);
            assert_eq!(theme.font_size, px(13.));
            assert_eq!(theme.font_family, ui_font().family);
            assert_eq!(theme.sidebar, t.sidebar);
            assert_eq!(theme.radius, px(6.));
            assert_eq!(
                theme.highlight_theme.style.editor_background,
                Some(t.surface)
            );
            assert_eq!(
                theme.highlight_theme.style.editor_foreground,
                Some(t.text_primary)
            );
            assert_eq!(
                theme.motion.duration_normal,
                Theme::default().motion.duration_normal
            );
        }
    }

    fn contrast(foreground: Hsla, background: Hsla) -> f64 {
        let luminance = |color: Hsla| {
            let color = color.to_rgb();
            let linear = |channel: f32| {
                let channel = f64::from(channel);
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
        };
        let a = luminance(foreground);
        let b = luminance(background);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn scrollbar_thumb_is_distinct_in_both_appearances() {
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            let t = resolve(mode, false, None);
            let mut theme = Theme::default();
            apply_tokens(&mut theme, &t, None);
            assert!(contrast(theme.scrollbar_thumb, theme.scrollbar) >= 3.0);
            assert!(contrast(theme.scrollbar_thumb_hover, theme.scrollbar) >= 3.0);
        }
    }

    #[test]
    fn kit_selection_tint_is_readable_on_explicit_os_canvas() {
        for (canvas, text, highlight, highlight_text) in [
            (0x000000, 0xffffff, 0xffff00, 0x000000),
            (0xffffff, 0x000000, 0x000080, 0xffffff),
        ] {
            let system = SystemColors {
                canvas: rgb(canvas).into(),
                text: rgb(text).into(),
                highlight: rgb(highlight).into(),
                highlight_text: rgb(highlight_text).into(),
                disabled_text: rgb(0x808080).into(),
                border: rgb(text).into(),
            };
            let t = resolve(ThemeMode::System, false, Some(&system));
            let mut theme = Theme::default();
            apply_tokens(&mut theme, &t, Some(&system));
            let selected_background: Hsla =
                t.canvas.to_rgb().blend(theme.selection.to_rgb()).into();
            let ratio = contrast(theme.foreground, selected_background);
            println!("Kit high contrast canvas=#{canvas:06x} input selection: {ratio:.3}:1");
            assert!(ratio >= 4.5);
            assert_eq!(theme.muted_foreground, system.disabled_text);
            assert_eq!(theme.tokens.button_primary.color, system.highlight);
            assert_eq!(theme.button_primary_foreground, system.highlight_text);
            assert!(!theme.shadow);
        }
    }
}
