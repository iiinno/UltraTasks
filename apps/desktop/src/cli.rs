use std::path::PathBuf;
use ultratasks_core::{Date, ZoneId, fixtures};

#[derive(Clone)]
pub struct Options {
    pub db: PathBuf,
    pub zone: ZoneId,
    pub day: Date,
    pub demo: Option<fixtures::Config>,
    pub diagnostics: Option<PathBuf>,
    pub smoke: bool,
    pub window_size: [u16; 2],
}

pub const USAGE: &str = "UltraTasks --db <絶対パス> --zone <IANA地域名> [--date YYYY-MM-DD] [--demo none|small|1000|10000] [--seed 42] [--diagnostics-dir <絶対パス>] [--window-size WIDTHxHEIGHT] [--smoke-test]";

impl Options {
    pub fn parse(args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let mut args = args.peekable();
        let (mut db, mut zone, mut day, mut diagnostics) = (None, None, None, None);
        let mut demo = None;
        let mut seed = 42;
        let mut smoke = false;
        let mut window_size = [1280, 800];
        let mut seen = std::collections::HashSet::new();
        while let Some(arg) = args.next() {
            if arg == "--help" || arg == "-h" {
                return Ok(None);
            }
            if !seen.insert(arg.clone()) {
                return Err(format!("引数が重複しています: {arg}"));
            }
            if arg == "--smoke-test" {
                smoke = true;
                continue;
            }
            let value = args
                .next()
                .ok_or_else(|| format!("{arg} の値がありません"))?;
            match arg.as_str() {
                "--db" => db = Some(absolute(value)?),
                "--zone" => {
                    zone = Some(
                        ZoneId::parse(&value)
                            .map_err(|e| crate::presentation::feedback::user_error(&e))?,
                    )
                }
                "--date" => {
                    day = Some(
                        Date::parse(&value)
                            .map_err(|e| crate::presentation::feedback::user_error(&e))?,
                    )
                }
                "--demo" => {
                    demo = match value.as_str() {
                        "none" => None,
                        "small" => Some(fixtures::Profile::Small),
                        "1000" => Some(fixtures::Profile::Thousand),
                        "10000" => Some(fixtures::Profile::TenThousand),
                        _ => return Err("--demo は none / small / 1000 / 10000 です".into()),
                    }
                }
                "--seed" => seed = value.parse().map_err(|_| "--seed は符号なし整数です")?,
                "--diagnostics-dir" => diagnostics = Some(absolute(value)?),
                "--window-size" => window_size = parse_window_size(&value)?,
                _ => return Err(format!("不明な引数: {arg}")),
            }
        }
        let db = db.ok_or("--db が必要です")?;
        let zone = zone.ok_or("--zone が必要です")?;
        let day = match day {
            Some(day) => day,
            None => {
                Date::today(&zone).map_err(|e| crate::presentation::feedback::user_error(&e))?
            }
        };
        let demo = demo.map(|profile| fixtures::Config {
            profile,
            distribution: fixtures::Distribution::ThirtyDays,
            seed,
            day,
            zone: zone.clone(),
        });
        Ok(Some(Self {
            db,
            zone,
            day,
            demo,
            diagnostics,
            smoke,
            window_size,
        }))
    }
}

fn parse_window_size(value: &str) -> Result<[u16; 2], String> {
    let invalid = "--window-size は 900x800 の形式で、幅620〜8192・高さ480〜8192を指定してください";
    let (w, h) = value.split_once('x').ok_or(invalid)?;
    if ![w, h]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid.into());
    }
    let w: u16 = w.parse().map_err(|_| invalid)?;
    let h: u16 = h.parse().map_err(|_| invalid)?;
    if !(620..=8192).contains(&w) || !(480..=8192).contains(&h) {
        return Err(invalid.into());
    }
    Ok([w, h])
}

fn absolute(value: String) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("パスは絶対パスで指定してください".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_dimensions_keep_default_and_validate_review_sizes() {
        let args = [
            "--db",
            "C:/test-only/unused.sqlite",
            "--zone",
            "Asia/Tokyo",
            "--date",
            "2026-11-11",
        ];
        let defaults = Options::parse(args.into_iter().map(str::to_owned))
            .unwrap()
            .unwrap();
        assert_eq!(defaults.window_size, [1280, 800]);
        for (value, size) in [("900x800", [900, 800]), ("1280x800", [1280, 800])] {
            let options = Options::parse(
                args.into_iter()
                    .chain(["--window-size", value])
                    .map(str::to_owned),
            )
            .unwrap()
            .unwrap();
            assert_eq!(options.window_size, size);
        }
        for value in [
            "0x800",
            "900x0",
            "619x800",
            "900x479",
            "900x999999",
            "NaNx800",
            "９００x８００",
            "900x800x1",
            "-900x800",
            "900.5x800",
        ] {
            assert!(
                Options::parse(
                    args.into_iter()
                        .chain(["--window-size", value])
                        .map(str::to_owned)
                )
                .is_err(),
                "{value}"
            );
        }
    }
    #[test]
    fn rejects_relative_database_before_touching_disk() {
        assert!(
            Options::parse(["--db", "relative.sqlite"].into_iter().map(str::to_owned)).is_err()
        );
    }
    #[test]
    fn help_does_not_open_database() {
        assert!(
            Options::parse(["--help"].into_iter().map(str::to_owned))
                .unwrap()
                .is_none()
        );
    }
}
