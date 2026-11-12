use std::{collections::HashSet, error::Error, path::PathBuf, process::ExitCode};
use ultratasks_core::{
    Date, ZoneId,
    fixtures::{self, Config, Distribution, Profile},
};

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let mut seen = HashSet::new();
    let mut db = None;
    let mut profile = Profile::Small;
    let mut distribution = Distribution::ThirtyDays;
    let mut seed = 42_u64;
    let mut day = "2026-11-11".to_owned();
    let mut zone = "Asia/Tokyo".to_owned();
    while let Some(key) = args.next() {
        if key == "--help" || key == "-h" {
            println!(
                "seed --db <absolute-path> [--profile small|1000|10000] [--seed 42] \
                 [--date 2026-11-11] [--zone Asia/Tokyo] [--distribution spread|dense]"
            );
            return Ok(());
        }
        if !seen.insert(key.clone()) {
            return Err(format!("duplicate argument: {key}").into());
        }
        let value = args.next().ok_or_else(|| format!("missing value: {key}"))?;
        match key.as_str() {
            "--db" => db = Some(PathBuf::from(value)),
            "--profile" => {
                profile = match value.as_str() {
                    "small" => Profile::Small,
                    "1000" => Profile::Thousand,
                    "10000" => Profile::TenThousand,
                    _ => return Err("profile must be small, 1000 or 10000".into()),
                }
            }
            "--distribution" => {
                distribution = match value.as_str() {
                    "spread" => Distribution::ThirtyDays,
                    "dense" => Distribution::DenseDay,
                    _ => return Err("distribution must be spread or dense".into()),
                }
            }
            "--seed" => seed = value.parse()?,
            "--date" => day = value,
            "--zone" => zone = value,
            _ => return Err(format!("unknown argument: {key}").into()),
        }
    }
    let db = db.ok_or("--db is required")?;
    if !db.is_absolute() {
        return Err("--db must be an absolute path".into());
    }
    match std::fs::symlink_metadata(&db) {
        Ok(metadata) if !metadata.is_file() || metadata.len() != 0 => {
            return Err("refusing to seed an existing nonempty database or non-file".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let config = Config {
        profile,
        distribution,
        seed,
        day: Date::parse(&day)?,
        zone: ZoneId::parse(&zone)?,
    };
    if let Some(parent) = db.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let report = fixtures::seed_empty(&db, config)?;
    println!(
        "FIXTURE_OK tasks={} events={} time_blocks={} seed={}",
        report.tasks, report.events, report.time_blocks, report.seed
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FIXTURE_FAILED: {error}");
            ExitCode::FAILURE
        }
    }
}
