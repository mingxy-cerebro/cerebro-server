use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::debug;

const CHANNEL_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub struct ServerEvent {
    pub event_type: String,
    pub tenant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    pub timestamp: String,
}

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<ServerEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { sender }
    }

    pub fn publish(&self, event: ServerEvent) {
        debug!(event_type = %event.event_type, tenant_id = %event.tenant_id, "publishing event");
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.sender.subscribe()
    }

    pub fn filtered_stream(&self, tenant_id: String) -> broadcast::Receiver<ServerEvent> {
        let rx = self.sender.subscribe();
        let _ = tenant_id;
        rx
    }
}

pub type SharedEventBus = Arc<EventBus>;
