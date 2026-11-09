//! The only mapping between design tokens and GPUI. Changing layout never changes storage.
use std::time::Duration;

use anyhow::{Context, Result};
use gpui_kit::{Hsla, rgb};
use serde_json::Value;

#[derive(Clone, Copy)]
pub struct ThemeTokens {
    pub canvas: Hsla,
    pub surface: Hsla,
    pub border: Hsla,
    pub primary: Hsla,
    pub secondary: Hsla,
    pub selected: Hsla,
    pub error: Hsla,
    pub fade: Duration,
    pub move_block: Duration,
}

impl ThemeTokens {
    pub fn load() -> Result<Self> {
        let tokens: Value = serde_json::from_str(include_str!("../../../design/tokens.json"))?;
        let palettes: Value = serde_json::from_str(include_str!("../../../design/palettes.json"))?;
        let color = |name: &str| -> Result<Hsla> {
            let reference = tokens["themes"]["dark"][name]
                .as_str()
                .context("missing semantic token")?;
            let (palette, step) = reference
                .split_once('.')
                .context("invalid palette reference")?;
            let value = palettes["dark"][palette][step]
                .as_str()
                .context("missing palette color")?;
            Ok(rgb(u32::from_str_radix(value.trim_start_matches('#'), 16)?).into())
        };
        Ok(Self {
            canvas: color("canvas")?,
            surface: color("surface")?,
            border: color("border")?,
            primary: color("text_primary")?,
            secondary: color("text_secondary")?,
            selected: color("selection_background")?,
            error: color("error_text")?,
            fade: Duration::from_millis(
                tokens["motion_proposal"]["popover_ms"]
                    .as_u64()
                    .context("missing fade duration")?,
            ),
            move_block: Duration::from_millis(
                tokens["motion_proposal"]["reposition_ms"]
                    .as_u64()
                    .context("missing move duration")?,
            ),
        })
    }
}
