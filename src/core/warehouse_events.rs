use serde::Serialize;
use tokio::sync::broadcast;

const WAREHOUSE_EVENT_CAPACITY: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WarehouseEvent {
    pub event: String,
    pub warehouse: String,
    pub reason: String,
}

#[derive(Clone)]
pub struct WarehouseEventHub {
    tx: broadcast::Sender<WarehouseEvent>,
}

impl Default for WarehouseEventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl WarehouseEventHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(WAREHOUSE_EVENT_CAPACITY);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WarehouseEvent> {
        self.tx.subscribe()
    }

    pub fn notify_updated(&self, warehouse: &str, reason: &str) {
        let warehouse = warehouse.trim();
        if warehouse.is_empty() {
            return;
        }
        let _ = self.tx.send(WarehouseEvent {
            event: "warehouse.updated".to_string(),
            warehouse: warehouse.to_string(),
            reason: reason.trim().to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn warehouse_event_hub_broadcasts_updates() {
        let hub = WarehouseEventHub::new();
        let mut rx = hub.subscribe();

        hub.notify_updated("Kalidor", "raw_material_stock");

        let event = rx.recv().await.expect("event");
        assert_eq!(event.event, "warehouse.updated");
        assert_eq!(event.warehouse, "Kalidor");
        assert_eq!(event.reason, "raw_material_stock");
    }
}
