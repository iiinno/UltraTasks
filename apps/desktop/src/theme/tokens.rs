use gpui_kit::{FontWeight, Hsla, rgb};

use super::ThemeMode;

pub const BODY_WEIGHT: FontWeight = FontWeight(450.);
pub const CONTROL_WEIGHT: FontWeight = FontWeight(500.);

/// desktopが高コントラスト有効時に取得したOS色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemColors {
    pub canvas: Hsla,
    pub text: Hsla,
    pub highlight: Hsla,
    pub highlight_text: Hsla,
    pub disabled_text: Hsla,
    pub border: Hsla,
}

/// 色の状態変更で寸法は変えない。寸法の単位はlogical px。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub canvas: Hsla,
    pub sidebar: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub border: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub focus_ring: Hsla,
    pub action_background: Hsla,
    pub action_text: Hsla,
    pub selection_background: Hsla,
    pub selection_text: Hsla,
    pub error_background: Hsla,
    pub error_text: Hsla,
    pub warning_background: Hsla,
    pub warning_text: Hsla,
    pub body_size: f32,
    pub body_line: f32,
    pub secondary_size: f32,
    pub secondary_line: f32,
    pub sidebar_width: f32,
    pub header_height: f32,
    pub nav_height: f32,
    pub control_height: f32,
    pub row_height: f32,
    pub control_radius: f32,
}

pub fn resolve(mode: ThemeMode, system_dark: bool, contrast: Option<&SystemColors>) -> Tokens {
    let dark = mode.is_dark(system_dark);
    let color = |light, dark_value| rgb(if dark { dark_value } else { light }).into();
    // design/tokens.jsonの役割とdesign/palettes.jsonのsRGB値。左がlight、右がdark。
    let mut tokens = Tokens {
        canvas: color(0xf9f9fa, 0x121213),
        sidebar: color(0xefeff0, 0x09090a),
        surface: color(0xffffff, 0x121213),
        surface_hover: color(0xf0f0f3, 0x171718),
        border: color(0xe2e2e2, 0x212224),
        text_primary: color(0x1c2024, 0xe2e4e7),
        text_secondary: color(0x60646c, 0x8a8f98),
        focus_ring: color(0x60646c, 0x8a8f98),
        action_background: color(0x1c2024, 0xe2e4e7),
        action_text: color(0xffffff, 0x121213),
        selection_background: color(0xe8e8ec, 0x1b1b1c),
        selection_text: color(0x1c2024, 0xe2e4e7),
        error_background: color(0xfeebe7, 0x391714),
        error_text: color(0x5c271f, 0xff977d),
        warning_background: color(0xfff7c2, 0x302008),
        warning_text: color(0x4f3422, 0xffca16),
        body_size: 13.,
        body_line: 19.5,
        secondary_size: 12.,
        secondary_line: 18.,
        sidebar_width: 244.,
        header_height: 58.,
        nav_height: 28.,
        control_height: 28.,
        row_height: 44.,
        control_radius: 6.,
    };
    if let Some(system) = contrast {
        tokens.canvas = system.canvas;
        tokens.sidebar = system.canvas;
        tokens.surface = system.canvas;
        // hoverの専用文字色を持たないため背景は保ち、描画側の輪郭で示す。
        tokens.surface_hover = system.canvas;
        tokens.border = system.border;
        tokens.text_primary = system.text;
        tokens.text_secondary = system.text;
        tokens.focus_ring = system.highlight;
        tokens.action_background = system.highlight;
        tokens.action_text = system.highlight_text;
        tokens.selection_background = system.highlight;
        tokens.selection_text = system.highlight_text;
        // 強制色では「保存失敗」「未保存」等の文字とアイコンで意味を残す。
        tokens.error_background = system.canvas;
        tokens.error_text = system.text;
        tokens.warning_background = system.canvas;
        tokens.warning_text = system.text;
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(foreground: Hsla, background: Hsla) -> f64 {
        fn luminance(color: Hsla) -> f64 {
            let rgb = color.to_rgb();
            let linear = |channel: f32| {
                let channel = f64::from(channel);
                if channel <= 0.04045 {
                    channel / 12.92
                } else {
                    ((channel + 0.055) / 1.055).powf(2.4)
                }
            };
            linear(rgb.r) * 0.2126 + linear(rgb.g) * 0.7152 + linear(rgb.b) * 0.0722
        }
        let foreground = luminance(foreground);
        let background = luminance(background);
        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
    }

    fn geometry(tokens: Tokens) -> [f32; 10] {
        [
            tokens.body_size,
            tokens.body_line,
            tokens.secondary_size,
            tokens.secondary_line,
            tokens.sidebar_width,
            tokens.header_height,
            tokens.nav_height,
            tokens.control_height,
            tokens.row_height,
            tokens.control_radius,
        ]
    }

    #[test]
    fn explicit_modes_ignore_os_appearance_and_system_follows_it() {
        for dark in [false, true] {
            let expected = if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            assert_eq!(
                resolve(ThemeMode::System, dark, None),
                resolve(expected, !dark, None)
            );
        }
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            assert_eq!(resolve(mode, false, None), resolve(mode, true, None));
        }
        assert_ne!(
            resolve(ThemeMode::Light, false, None),
            resolve(ThemeMode::Dark, false, None)
        );
    }

    #[test]
    fn text_contrast_covers_normal_hover_selection_action_and_feedback() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let t = resolve(mode, false, None);
            let mut pairs = Vec::new();
            for (background_name, background) in [
                ("canvas", t.canvas),
                ("sidebar", t.sidebar),
                ("surface", t.surface),
                ("hover", t.surface_hover),
            ] {
                pairs.push(("primary", background_name, t.text_primary, background));
                pairs.push(("secondary", background_name, t.text_secondary, background));
            }
            pairs.extend([
                ("action", "action", t.action_text, t.action_background),
                (
                    "selection",
                    "selection",
                    t.selection_text,
                    t.selection_background,
                ),
                ("error", "error", t.error_text, t.error_background),
                ("warning", "warning", t.warning_text, t.warning_background),
                // KitのInputは選択中も通常の文字色を使う。
                ("input", "selection", t.text_primary, t.selection_background),
            ]);
            for (foreground_name, background_name, foreground, background) in pairs {
                assert_eq!(foreground.a, 1.);
                assert_eq!(background.a, 1.);
                let contrast = ratio(foreground, background);
                println!("{mode:?} {foreground_name}/{background_name}: {contrast:.3}:1");
                assert!(
                    contrast >= 4.5,
                    "{mode:?} {foreground_name}/{background_name}: {contrast}"
                );
            }
            for (name, background) in [
                ("canvas", t.canvas),
                ("sidebar", t.sidebar),
                ("surface", t.surface),
                ("hover", t.surface_hover),
                ("selection", t.selection_background),
            ] {
                let contrast = ratio(t.focus_ring, background);
                println!("{mode:?} focus/{name}: {contrast:.3}:1");
                assert!(contrast >= 3., "{mode:?} focus/{name}: {contrast}");
            }
            assert_ne!(t.surface_hover, t.selection_background);
            assert_ne!(t.focus_ring, t.selection_background);
            assert_ne!(t.error_background, t.warning_background);
            assert_ne!(t.error_text, t.warning_text);
        }
    }

    #[test]
    fn high_contrast_uses_os_pairs_without_dimming_body_text() {
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
            let expected = resolve(ThemeMode::System, false, Some(&system));
            for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
                for dark in [false, true] {
                    let t = resolve(mode, dark, Some(&system));
                    assert_eq!(t, expected);
                    assert_eq!(
                        [t.canvas, t.sidebar, t.surface, t.surface_hover],
                        [system.canvas; 4]
                    );
                    assert_eq!([t.text_primary, t.text_secondary], [system.text; 2]);
                    assert_ne!(t.text_secondary, system.disabled_text);
                    assert_eq!(t.border, system.border);
                    assert_eq!(t.focus_ring, system.highlight);
                    assert_eq!(
                        [t.action_background, t.selection_background],
                        [system.highlight; 2]
                    );
                    assert_eq!(
                        [t.action_text, t.selection_text],
                        [system.highlight_text; 2]
                    );
                    assert_eq!(
                        [t.error_background, t.warning_background],
                        [system.canvas; 2]
                    );
                    assert_eq!([t.error_text, t.warning_text], [system.text; 2]);
                    assert!(ratio(t.text_primary, t.canvas) >= 4.5);
                    assert!(ratio(t.selection_text, t.selection_background) >= 4.5);
                    assert_eq!(
                        geometry(t),
                        [13., 19.5, 12., 18., 244., 58., 28., 28., 44., 6.]
                    );
                }
            }
        }
    }

    #[test]
    fn theme_and_interaction_colors_do_not_change_density() {
        let light = resolve(ThemeMode::Light, false, None);
        let dark = resolve(ThemeMode::Dark, true, None);
        assert_eq!(geometry(light), geometry(dark));
        assert_eq!(
            geometry(light),
            [13., 19.5, 12., 18., 244., 58., 28., 28., 44., 6.]
        );
    }
}
