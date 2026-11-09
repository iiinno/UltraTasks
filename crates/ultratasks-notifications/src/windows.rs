use crate::{DesiredNotification, Error, NotificationBackend, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::DateTime,
    UI::Notifications::{
        NotificationSetting, ScheduledToastNotification, ToastNotification,
        ToastNotificationManager, ToastNotifier,
    },
    core::HSTRING,
};
use winreg::{RegKey, enums::HKEY_CURRENT_USER};

const GROUP: &str = "reminders";

/// Native scheduled toasts remain owned by Windows after this process exits.
/// A dedicated AUMID keeps this app's queue and history separate from others.
pub struct WindowsBackend {
    notifier: ToastNotifier,
}

impl WindowsBackend {
    pub fn new() -> Result<Self> {
        Self::with_namespace("default")
    }

    /// A separate namespace for each canonical database path prevents a test or
    /// second profile from cancelling the first profile's native notifications.
    pub fn with_namespace(namespace: &str) -> Result<Self> {
        let hash = Sha256::digest(namespace.as_bytes());
        let suffix: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
        let app_id = format!("UltraTasks.Desktop.{suffix}");
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = root
            .create_subkey(format!("Software\\Classes\\AppUserModelId\\{app_id}"))
            .map_err(|e| Error::Platform(format!("register notification identity: {e}")))?;
        key.set_value("DisplayName", &"UltraTasks")
            .map_err(|e| Error::Platform(e.to_string()))?;
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(&app_id))
            .map_err(platform)?;
        Ok(Self { notifier })
    }

    fn permitted(&self) -> Result<()> {
        let setting = self.notifier.Setting().map_err(platform)?;
        if setting != NotificationSetting::Enabled {
            return Err(Error::Permission(format!(
                "Windows notification setting: {}",
                setting.0
            )));
        }
        Ok(())
    }

    fn remove_pending(&self, id: &str) -> Result<()> {
        let scheduled = self
            .notifier
            .GetScheduledToastNotifications()
            .map_err(platform)?;
        for entry in scheduled {
            if entry.Id().map_err(platform)?.to_string() == id {
                self.notifier.RemoveFromSchedule(&entry).map_err(platform)?;
            }
        }
        Ok(())
    }
}

impl NotificationBackend for WindowsBackend {
    fn pending_ids(&mut self) -> Result<BTreeSet<String>> {
        // Reading/cancelling still works when notifications are disabled. This
        // lets turning UltraTasks reminders off remove previously queued items.
        let scheduled = self
            .notifier
            .GetScheduledToastNotifications()
            .map_err(platform)?;
        scheduled
            .into_iter()
            .map(|item| item.Id().map(|id| id.to_string()).map_err(platform))
            .collect()
    }

    fn schedule(&mut self, os_id: &str, notification: &DesiredNotification) -> Result<()> {
        self.permitted()?;
        // Windows IDs and tags are limited to 16 characters. Scheduler uses
        // 16 hex digits and checks collisions before any OS effects.
        self.remove_pending(os_id)?;
        let xml = document(notification)?;
        let ticks = notification
            .fire_at
            .timestamp()
            .checked_add(11_644_473_600)
            .and_then(|seconds| seconds.checked_mul(10_000_000))
            .and_then(|value| {
                value.checked_add(i64::from(
                    notification.fire_at.timestamp_subsec_nanos() / 100,
                ))
            })
            .ok_or_else(|| Error::Invalid("Windows timestamp overflow".into()))?;
        let item = ScheduledToastNotification::CreateScheduledToastNotification(
            &xml,
            DateTime {
                UniversalTime: ticks,
            },
        )
        .map_err(platform)?;
        item.SetId(&HSTRING::from(os_id)).map_err(platform)?;
        item.SetTag(&HSTRING::from(os_id)).map_err(platform)?;
        item.SetGroup(&HSTRING::from(GROUP)).map_err(platform)?;
        self.notifier.AddToSchedule(&item).map_err(platform)
    }

    fn show(&mut self, os_id: &str, notification: &DesiredNotification) -> Result<()> {
        self.permitted()?;
        self.remove_pending(os_id)?;
        let toast = ToastNotification::CreateToastNotification(&document(notification)?)
            .map_err(platform)?;
        toast.SetTag(&HSTRING::from(os_id)).map_err(platform)?;
        toast.SetGroup(&HSTRING::from(GROUP)).map_err(platform)?;
        self.notifier.Show(&toast).map_err(platform)
    }

    fn cancel(&mut self, os_id: &str) -> Result<()> {
        // Catch-up grace limits scheduling, not the lifetime of a notification
        // the user has not read. Delivered Action Center entries stay visible.
        self.remove_pending(os_id)
    }
}

fn document(notification: &DesiredNotification) -> Result<XmlDocument> {
    let xml = XmlDocument::new().map_err(platform)?;
    xml.LoadXml(&HSTRING::from(format!("<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",escape(&notification.title),escape(&notification.body)))).map_err(platform)?;
    Ok(xml)
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn platform(error: windows::core::Error) -> Error {
    Error::Platform(error.to_string())
}
