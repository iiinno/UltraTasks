use std::{path::PathBuf, time::Duration};
use ultratasks_core::Engine;
use ultratasks_notifications::{DevicePolicy, plan_notifications};

struct Options {
    database: PathBuf,
    state: PathBuf,
    timezone: String,
    disabled: bool,
    plan: bool,
    watch: bool,
    install: bool,
    remove: bool,
}
fn options() -> Result<Options, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut database = None;
    let mut state = None;
    let mut timezone = "UTC".to_owned();
    let mut disabled = false;
    let mut plan = false;
    let mut watch = false;
    let mut install = false;
    let mut remove = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "ultratasks-notifications --database PATH [--state PATH] [--timezone IANA] [--once|--watch|--plan|--install-worker|--remove-worker] [--disable-notifications]\nWindows: native scheduled notifications. --install-worker explicitly registers an hourly per-user Task Scheduler task. Other systems support --plan only."
                );
                std::process::exit(0)
            }
            "--database" => {
                database = Some(PathBuf::from(args.next().ok_or("missing database path")?))
            }
            "--state" => state = Some(PathBuf::from(args.next().ok_or("missing state path")?)),
            "--timezone" => timezone = args.next().ok_or("missing timezone")?,
            "--disable-notifications" => disabled = true,
            "--plan" => plan = true,
            "--watch" => watch = true,
            "--once" => {}
            "--install-worker" => install = true,
            "--remove-worker" => remove = true,
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if [plan, watch, install, remove]
        .into_iter()
        .filter(|v| *v)
        .count()
        > 1
    {
        return Err("choose one execution mode".into());
    }
    let database = database.ok_or("--database is required")?;
    let state = state.unwrap_or_else(|| {
        let mut filename = database.file_name().unwrap_or_default().to_os_string();
        filename.push(".notifications.db");
        database.with_file_name(filename)
    });
    Ok(Options {
        database,
        state,
        timezone,
        disabled,
        plan,
        watch,
        install,
        remove,
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = options()?;
    let engine = Engine::open(&args.database)?;
    let policy = DevicePolicy {
        enabled: !args.disabled,
        timezone: args.timezone.clone(),
        ..DevicePolicy::default()
    };
    if args.plan {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan_notifications(
                &engine,
                &policy,
                chrono::Utc::now()
            )?)?
        );
        return Ok(());
    }
    #[cfg(windows)]
    {
        use ultratasks_notifications::{Scheduler, WindowsBackend};
        let database = std::fs::canonicalize(&args.database)?;
        if args.install || args.remove {
            return worker_task(&args, &database);
        }
        let mut scheduler = Scheduler::open(&args.state, "windows-desktop")?;
        let mut backend = WindowsBackend::with_namespace(&database.to_string_lossy())?;
        loop {
            let now = chrono::Utc::now();
            let desired = plan_notifications(&engine, &policy, now)?;
            println!(
                "{}",
                serde_json::to_string(&scheduler.reconcile(&desired, now, &mut backend)?)?
            );
            if !args.watch {
                break;
            }
            std::thread::sleep(Duration::from_secs(30));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (
            args.state,
            args.install,
            args.remove,
            args.watch,
            Duration::ZERO,
        );
        Err("native delivery CLI currently supports Windows; --plan works on every platform".into())
    }
}

#[cfg(windows)]
fn worker_task(
    args: &Options,
    database: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(database.to_string_lossy().as_bytes());
    let suffix: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    let name = format!("UltraTasks Reminders {suffix}");
    let mut command = std::process::Command::new("schtasks.exe");
    let mut task_file = None;
    if args.remove {
        command.args(["/Delete", "/TN", &name, "/F"]);
    } else {
        let exe = std::env::current_exe()?;
        let state = if args.state.is_absolute() {
            args.state.clone()
        } else {
            std::env::current_dir()?.join(&args.state)
        };
        let mut fields = vec![
            "--database".into(),
            database.to_string_lossy().into_owned(),
            "--state".into(),
            state.to_string_lossy().into_owned(),
            "--timezone".into(),
            args.timezone.clone(),
            "--once".into(),
        ];
        if args.disabled {
            fields.push("--disable-notifications".into());
        }
        let account = std::process::Command::new("whoami.exe")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()?;
        if !account.status.success() {
            return Err("cannot identify the current Windows user".into());
        }
        // SID is ASCII even when the username is emitted in a non-UTF8 OEM code
        // page. Use the stable SID, not localized account-name decoding.
        let output = String::from_utf8_lossy(&account.stdout);
        let account = output
            .trim()
            .rsplit(',')
            .next()
            .unwrap_or_default()
            .trim_matches('"');
        if !account.starts_with("S-1-")
            || !account
                .chars()
                .all(|ch| ch == 'S' || ch == '-' || ch.is_ascii_digit())
        {
            return Err("could not read the current Windows user's SID".into());
        }
        let xml = task_xml(
            &exe.to_string_lossy(),
            &fields,
            account,
            &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
        let mut file = tempfile::NamedTempFile::new()?;
        use std::io::Write;
        file.write_all(xml.as_bytes())?;
        file.flush()?;
        command
            .args(["/Create", "/TN", &name, "/XML"])
            .arg(file.path())
            .arg("/F");
        task_file = Some(file);
    }
    if !command.status()?.success() {
        return Err("Windows Task Scheduler rejected the operation".into());
    }
    drop(task_file);
    Ok(())
}

#[cfg(any(windows, test))]
fn windows_quote(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        if ch == '"' {
            result.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            result.extend(std::iter::repeat_n('\\', slashes));
        }
        slashes = 0;
        result.push(ch);
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

#[cfg(any(windows, test))]
fn task_xml(executable: &str, arguments: &[String], account: &str, start: &str) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    let arguments = arguments
        .iter()
        .map(|value| windows_quote(value))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<Triggers><TimeTrigger><Repetition><Interval>PT1H</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition><StartBoundary>{}</StartBoundary><Enabled>true</Enabled></TimeTrigger><LogonTrigger><Enabled>true</Enabled><UserId>{}</UserId></LogonTrigger></Triggers>
<Principals><Principal id="User"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><StartWhenAvailable>true</StartWhenAvailable><ExecutionTimeLimit>PT5M</ExecutionTimeLimit><Enabled>true</Enabled></Settings>
<Actions Context="User"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions></Task>"#,
        escape(start),
        escape(account),
        escape(account),
        escape(executable),
        escape(&arguments)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_definition_preserves_long_unicode_paths_and_separates_executable() {
        let executable = format!(
            "C:\\Users\\日本語 & space\\{}\\worker.exe",
            "folder\\".repeat(45)
        );
        let xml = task_xml(
            &executable,
            &["--database".into(), "C:\\data & 日本語\\db.sqlite".into()],
            "DOMAIN\\利用者",
            "2026-11-09T00:00:00Z",
        );
        assert!(xml.contains(&format!(
            "<Command>{}</Command>",
            executable.replace('&', "&amp;")
        )));
        assert!(xml.contains("<Arguments>&quot;--database&quot; &quot;C:\\data &amp; 日本語\\db.sqlite&quot;</Arguments>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<Interval>PT1H</Interval>"));
    }
    #[test]
    fn windows_arguments_quote_empty_quotes_and_trailing_slashes() {
        assert_eq!(windows_quote(""), "\"\"");
        assert_eq!(windows_quote("a b\\"), "\"a b\\\\\"");
        assert_eq!(windows_quote("a\"b"), "\"a\\\"b\"");
    }
}
