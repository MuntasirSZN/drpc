use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::{
    path::PathBuf,
    time::{Duration, SystemTime},
};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DetectableExecutable {
    pub name: String,
    #[serde(default)]
    pub is_launcher: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DetectableEntry {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub executables: Vec<DetectableExecutable>,
}

#[derive(Clone, Default)]
pub struct Detectables {
    inner: Arc<RwLock<Vec<DetectableEntry>>>,
}

impl Detectables {
    pub fn list(&self) -> Arc<Vec<DetectableEntry>> {
        Arc::new(self.inner.read().clone())
    }
}

pub fn load_detectables(force_refresh: bool, ttl_hours: u64) -> Detectables {
    let path = detectables_path();
    let mut need_fetch = force_refresh || !path.exists();
    if !need_fetch {
        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(modified) = meta.modified() {
                if let Ok(age) = SystemTime::now().duration_since(modified) {
                    if age > Duration::from_secs(ttl_hours * 3600) {
                        need_fetch = true;
                    }
                }
            }
        }
    }
    let mut list: Vec<DetectableEntry> = Vec::new();
    if need_fetch {
        debug!(?path, "(placeholder) would fetch remote detectables");
        // placeholder: fallback to embedded empty list
    } else if let Ok(data) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<Vec<DetectableEntry>>(&data) {
            Ok(v) => list = v,
            Err(e) => warn!(error=?e, "failed to parse detectables; using empty"),
        }
    }
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&list) {
            let _ = std::fs::write(&path, json);
        }
    }
    info!(count = list.len(), "loaded detectables");
    Detectables {
        inner: Arc::new(RwLock::new(list)),
    }
}

fn detectables_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let mut p = PathBuf::from(home);
    p.push(".arrpc");
    p.push("detectables.json");
    p
}
