use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::watch;

use super::models::{AdminServerMonitorBackups, AdminServerMonitorResponse};

#[derive(Clone)]
pub struct SystemMonitorHub {
    sender: watch::Sender<Option<AdminServerMonitorResponse>>,
    started: Arc<AtomicBool>,
    backups: watch::Sender<Option<AdminServerMonitorBackups>>,
    backup_started: Arc<AtomicBool>,
}

impl SystemMonitorHub {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(None);
        let (backups, _) = watch::channel(None);
        Self {
            sender,
            started: Arc::new(AtomicBool::new(false)),
            backups,
            backup_started: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn mark_started(&self) -> bool {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn publish(&self, snapshot: AdminServerMonitorResponse) {
        // The always-on doctor must retain its latest sample even with no UI subscribed.
        self.sender.send_replace(Some(snapshot));
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<AdminServerMonitorResponse>> {
        self.sender.subscribe()
    }

    pub fn mark_backup_started(&self) -> bool {
        self.backup_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn publish_backups(&self, snapshot: AdminServerMonitorBackups) {
        self.backups.send_replace(Some(snapshot));
    }

    pub fn subscribe_backups(&self) -> watch::Receiver<Option<AdminServerMonitorBackups>> {
        self.backups.subscribe()
    }
}

impl Default for SystemMonitorHub {
    fn default() -> Self {
        Self::new()
    }
}
