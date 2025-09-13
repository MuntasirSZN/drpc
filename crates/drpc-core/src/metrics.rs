use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};

pub static ACTIVE_CONNECTIONS: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
pub static ACTIVITIES_SET: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
pub static PROCESSES_DETECTED: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
pub static DETECTABLES_COUNT: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

pub fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "active_connections": ACTIVE_CONNECTIONS.load(Ordering::Relaxed),
        "activities_set": ACTIVITIES_SET.load(Ordering::Relaxed),
        "processes_detected": PROCESSES_DETECTED.load(Ordering::Relaxed),
        "detectables_count": DETECTABLES_COUNT.load(Ordering::Relaxed),
    })
}
