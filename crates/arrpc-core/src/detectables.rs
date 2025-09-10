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

pub async fn load_detectables_async(force_refresh: bool, ttl_hours: u64) -> Detectables {
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
        #[cfg(feature = "network")]
        {
            debug!("fetching detectables from network");
            match fetch_remote_with_retries(3).await {
                Ok(remote) => {
                    list = remote;
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(json) = serde_json::to_string_pretty(&list) {
                        let _ = std::fs::write(&path, json);
                    }
                }
                Err(e) => warn!(error=?e, "failed fetch; using fallback"),
            }
        }
        #[cfg(not(feature = "network"))]
        {
            debug!("network feature disabled; using empty detectables");
        }
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
    let mut age_hours: Option<u64> = None;
    if let Ok(meta) = std::fs::metadata(&path) {
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = SystemTime::now().duration_since(modified) {
                age_hours = Some(age.as_secs() / 3600);
            }
        }
    }
    info!(count = list.len(), stale=need_fetch, age_hours=?age_hours, ttl_hours, "loaded detectables");
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

#[cfg(feature = "network")]
async fn fetch_remote() -> Result<Vec<DetectableEntry>, reqwest::Error> {
    let url = "https://discord.com/api/v9/applications/detectable";
    let resp = reqwest::get(url).await?;
    if !resp.status().is_success() {
        return Ok(Vec::new());
    }
    let val: serde_json::Value = resp.json().await?;
    if let Some(arr) = val.as_array() {
        let mut out = Vec::new();
        for v in arr {
            if let Ok(entry) = serde_json::from_value::<DetectableEntry>(v.clone()) {
                out.push(entry);
            }
        }
        Ok(out)
    } else {
        Ok(Vec::new())
    }
}

#[cfg(feature = "network")]
async fn fetch_remote_with_retries(max: usize) -> Result<Vec<DetectableEntry>, reqwest::Error> {
    let mut attempt = 0;
    loop {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200 * attempt as u64)).await;
        }
        match fetch_remote().await {
            Ok(v) => return Ok(v),
            Err(e) if attempt + 1 < max => {
                warn!(error=?e, attempt, "detectables fetch retry");
                attempt += 1;
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}
