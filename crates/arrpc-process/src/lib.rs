use arrpc_core::{Activity, DetectableEntry, Detectables, EventBus, EventKind};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time::{interval, Duration};

#[derive(Default, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub exe: String,
    pub cmdline: String,
}

#[async_trait::async_trait]
pub trait ProcessBackend: Send + Sync {
    async fn list(&self) -> Vec<ProcessInfo>;
}

pub struct LinuxBackend;
#[async_trait::async_trait]
impl ProcessBackend for LinuxBackend {
    async fn list(&self) -> Vec<ProcessInfo> {
        // placeholder minimal scanning of /proc limited for now
        Vec::new()
    }
}

pub struct Scanner<B: ProcessBackend + 'static> {
    backend: B,
    detectables: Detectables,
    bus: EventBus,
    seen: Arc<RwLock<HashSet<u32>>>,
}

impl<B: ProcessBackend + 'static> Scanner<B> {
    pub fn new(backend: B, detectables: Detectables, bus: EventBus) -> Self {
        Self {
            backend,
            detectables,
            bus,
            seen: Arc::new(RwLock::new(HashSet::new())),
        }
    }
    pub fn spawn(self) {
        tokio::spawn(async move {
            self.run().await;
        });
    }
    async fn run(self) {
        let mut tick = interval(Duration::from_secs(5));
        let mut last_map: HashMap<u32, String> = HashMap::new();
        loop {
            tick.tick().await;
            let procs = self.backend.list().await;
            let mut current: HashSet<u32> = HashSet::new();
            for p in &procs {
                current.insert(p.pid);
            }
            // removed
            for pid in last_map.keys() {
                if !current.contains(pid) {
                    self.bus.publish(EventKind::Clear {
                        socket_id: format!("proc-{}", pid),
                    });
                }
            }
            let detectables = self.detectables.list();
            for p in procs {
                if let Some(d) = match_process(&p, &detectables) {
                    let act = Activity {
                        name: d.name.clone(),
                        ..Default::default()
                    };
                    self.bus.publish(EventKind::ActivityUpdate {
                        socket_id: format!("proc-{}", p.pid),
                        payload: serde_json::to_value(act).unwrap(),
                    });
                    last_map.insert(p.pid, d.name.clone());
                }
            }
        }
    }
}

fn match_process<'a>(
    _p: &ProcessInfo,
    _list: &'a [DetectableEntry],
) -> Option<&'a DetectableEntry> {
    // placeholder (always none)
    None
}
