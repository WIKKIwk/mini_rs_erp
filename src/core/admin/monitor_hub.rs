use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::watch;

use super::models::AdminServerMonitorResponse;

#[derive(Clone)]
pub struct SystemMonitorHub {
    sender: watch::Sender<Option<AdminServerMonitorResponse>>,
    started: Arc<AtomicBool>,
}

impl SystemMonitorHub {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(None);
        Self {
            sender,
            started: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn mark_started(&self) -> bool {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn publish(&self, snapshot: AdminServerMonitorResponse) {
        let _ = self.sender.send(Some(snapshot));
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<AdminServerMonitorResponse>> {
        self.sender.subscribe()
    }
}

impl Default for SystemMonitorHub {
    fn default() -> Self {
        Self::new()
    }
}
