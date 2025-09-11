use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct ActivityRegistry {
    inner: Arc<RwLock<HashMap<String, Value>>>,
}

impl ActivityRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&self, socket: impl Into<String>, activity: Value) {
        self.inner.write().insert(socket.into(), activity);
    }
    pub fn clear(&self, socket: &str) {
        self.inner.write().insert(socket.to_string(), Value::Null);
    }
    pub fn snapshot(&self) -> HashMap<String, Value> {
        self.inner.read().clone()
    }
    pub fn non_null(&self) -> Vec<(String, Value)> {
        self.inner
            .read()
            .iter()
            .filter_map(|(k, v)| {
                if v.is_null() {
                    None
                } else {
                    Some((k.clone(), v.clone()))
                }
            })
            .collect()
    }
}
