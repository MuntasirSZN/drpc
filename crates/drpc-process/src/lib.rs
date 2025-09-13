use drpc_core::{Activity, DetectableEntry, Detectables, EventBus, EventKind};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::time::{Duration, interval};

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
    #[allow(dead_code)] // for now
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
                    drpc_core::metrics::PROCESSES_DETECTED
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

fn match_process<'a>(p: &ProcessInfo, list: &'a [DetectableEntry]) -> Option<&'a DetectableEntry> {
    let exe_base = base_lower(&p.exe);
    let cmd_lower = p.cmdline.to_lowercase();
    // Skip common launchers explicitly
    const LAUNCHERS: &[&str] = &[
        "steam",
        "steam.exe",
        "steamwebhelper",
        "battle.net",
        "epicgameslauncher",
        "riotclientservices",
        "launcher",
    ];
    if LAUNCHERS.contains(&exe_base.as_str()) {
        return None;
    }
    // Try direct executable name match first, skipping detectable launchers
    for d in list {
        for exe in &d.executables {
            if exe.is_launcher {
                continue;
            }
            let det_base = base_lower(&exe.name);
            if names_match(&exe_base, &det_base) {
                return Some(d);
            }
        }
    }
    // Heuristic: Java-based processes — match on JAR or app name in cmdline
    if matches!(
        exe_base.as_str(),
        "java" | "javaw" | "java.exe" | "javaw.exe"
    ) {
        for d in list {
            // prefer executable names if present
            for exe in &d.executables {
                if exe.is_launcher {
                    continue;
                }
                let det_base = base_lower(&exe.name);
                if cmd_lower.contains(&det_base) || cmd_lower.contains(&strip_ext(&det_base)) {
                    return Some(d);
                }
            }
            // fallback: look for detectable name in cmdline
            let dn = d.name.to_lowercase();
            if cmd_lower.contains(&dn) {
                return Some(d);
            }
        }
    }
    None
}

fn base_lower(s: &str) -> String {
    std::path::Path::new(s)
        .file_name()
        .and_then(|o| o.to_str())
        .unwrap_or(s)
        .to_lowercase()
}

fn strip_ext(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => name.to_string(),
    }
}

fn names_match(a: &str, b: &str) -> bool {
    a == b || strip_ext(a) == b || a == strip_ext(b) || strip_ext(a) == strip_ext(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(name: &str, exe: &str, is_launcher: bool) -> DetectableEntry {
        DetectableEntry {
            id: None,
            name: name.into(),
            executables: vec![drpc_core::DetectableExecutable {
                name: exe.into(),
                is_launcher,
            }],
        }
    }

    #[test]
    fn match_by_exe_basename() {
        let list = vec![det("CoolGame", "coolgame", false)];
        let p = ProcessInfo {
            pid: 1,
            exe: "/usr/bin/coolgame".into(),
            cmdline: "coolgame".into(),
        };
        let m = match_process(&p, &list);
        assert!(m.is_some());
        assert_eq!(m.unwrap().name, "CoolGame");
    }

    #[test]
    fn launcher_is_skipped() {
        let list = vec![det("Launcher", "launcher", true)];
        let p = ProcessInfo {
            pid: 2,
            exe: "/opt/launcher".into(),
            cmdline: "launcher".into(),
        };
        let m = match_process(&p, &list);
        assert!(m.is_none());
    }

    #[test]
    fn java_jar_name_in_cmdline() {
        let list = vec![det("JarGame", "JarGame.jar", false)];
        let p = ProcessInfo {
            pid: 3,
            exe: "/usr/bin/java".into(),
            cmdline: "java -jar /home/user/JarGame.jar".into(),
        };
        let m = match_process(&p, &list);
        assert!(m.is_some());
        assert_eq!(m.unwrap().name, "JarGame");
    }

    #[test]
    fn extension_mismatch_still_matches() {
        // coolgame.exe vs coolgame
        let list = vec![det("CoolGame", "coolgame.exe", false)];
        let p = ProcessInfo {
            pid: 4,
            exe: "coolgame".into(),
            cmdline: "coolgame".into(),
        };
        let m = match_process(&p, &list);
        assert!(m.is_some());
        assert_eq!(m.unwrap().name, "CoolGame");
    }

    #[test]
    fn launcher_executable_name_skipped_even_if_detectable_has_other_exes() {
        // Ensure LAUNCHERS exclusion triggers before scanning detectables
        let list = vec![det("SomeGame", "somegame", false)];
        let p = ProcessInfo {
            pid: 5,
            exe: "steam".into(), // explicit launcher in constant list
            cmdline: "steam --run-game somegame".into(),
        };
        let m = match_process(&p, &list);
        assert!(m.is_none());
    }

    #[test]
    fn java_process_detectable_name_fallback() {
        // Detectable name appears in cmdline but no explicit executable entry matches
        let list = vec![det("BlockWorld", "", false)];
        let p = ProcessInfo {
            pid: 6,
            exe: "java".into(),
            cmdline: "java -Xmx2G -cp libs/* com.company.BlockWorldMain".into(),
        };
        let m = match_process(&p, &list);
        assert!(m.is_some());
        assert_eq!(m.unwrap().name, "BlockWorld");
    }
}
