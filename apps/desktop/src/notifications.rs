//! OS notification effects run on the storage worker, never on a GPUI callback.
//! Scheduling failures are reported separately from successful database commits.
use std::path::Path;

use chrono_tz::Tz;
use ultratasks_core::Engine;

pub struct Notifications {
    #[cfg(windows)]
    state_path: std::path::PathBuf,
    #[cfg(windows)]
    database: std::path::PathBuf,
    #[cfg(windows)]
    policy: ultratasks_notifications::DevicePolicy,
    #[cfg(windows)]
    scheduler: Option<ultratasks_notifications::Scheduler>,
    #[cfg(windows)]
    backend: Option<ultratasks_notifications::WindowsBackend>,
}

impl Notifications {
    pub fn new(database: &Path, timezone: Tz, enabled: bool) -> Self {
        #[cfg(windows)]
        {
            let mut filename = database.file_name().unwrap_or_default().to_os_string();
            filename.push(".notifications.db");
            Self {
                state_path: database.with_file_name(filename),
                database: database.to_path_buf(),
                policy: ultratasks_notifications::DevicePolicy {
                    enabled,
                    timezone: timezone.to_string(),
                    ..Default::default()
                },
                scheduler: None,
                backend: None,
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (database, timezone, enabled);
            Self {}
        }
    }

    pub fn reconcile(&mut self, engine: &Engine) -> Option<String> {
        #[cfg(windows)]
        {
            self.reconcile_windows(engine)
                .err()
                .map(|error| format!("保存済み。通知の登録に失敗しました: {error}"))
        }
        #[cfg(not(windows))]
        {
            let _ = engine;
            None
        }
    }

    #[cfg(windows)]
    fn reconcile_windows(&mut self, engine: &Engine) -> anyhow::Result<()> {
        use ultratasks_notifications::{Scheduler, WindowsBackend, plan_notifications};
        if self.scheduler.is_none() {
            self.scheduler = Some(Scheduler::open(&self.state_path, "windows-desktop")?);
        }
        if self.backend.is_none() {
            self.backend = Some(WindowsBackend::with_namespace(
                &std::fs::canonicalize(&self.database)?.to_string_lossy(),
            )?);
        }
        let now = chrono::Utc::now();
        let desired = plan_notifications(engine, &self.policy, now)?;
        self.scheduler
            .as_mut()
            .expect("initialized above")
            .reconcile(
                &desired,
                now,
                self.backend.as_mut().expect("initialized above"),
            )?;
        Ok(())
    }
}
