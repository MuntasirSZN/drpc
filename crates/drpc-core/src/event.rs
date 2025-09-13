use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum EventKind {
    ActivityUpdate { socket_id: String, payload: Value },
    Clear { socket_id: String },
    PrivacyRefresh,
}

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<RwLock<Vec<tokio::sync::mpsc::UnboundedSender<EventKind>>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Vec::new())),
        }
    }
    pub fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<EventKind> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.inner.write().push(tx);
        rx
    }
    pub fn publish(&self, evt: EventKind) {
        let list = self.inner.read();
        for sub in list.iter() {
            let _ = sub.send(evt.clone());
        }
        if let EventKind::ActivityUpdate { .. } = &evt {
            crate::metrics::ACTIVITIES_SET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}
