use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use drpc_core::{Activity, ActivityRegistry, Detectables, EventBus, EventKind};
use parking_lot::RwLock;
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct RestState {
    pub bus: EventBus,
    pub registry: Arc<ActivityRegistry>,
    pub detectables: Option<Detectables>,
    pub detectables_ttl: u64,
    pub allow: Arc<RwLock<Option<Vec<String>>>>,
    pub deny: Arc<RwLock<Option<Vec<String>>>>,
}

#[derive(Deserialize)]
struct SetActivityPayload {
    socket_id: Option<String>,
    activity: Activity,
}

pub async fn run_rest(
    bus: EventBus,
    registry: Arc<ActivityRegistry>,
    detectables: Option<Detectables>,
    detectables_ttl: u64,
    port: u16,
) -> anyhow::Result<u16> {
    let state = RestState {
        bus,
        registry,
        detectables,
        detectables_ttl,
        allow: Arc::new(RwLock::new(None)),
        deny: Arc::new(RwLock::new(None)),
    };
    let app_state = state.clone();
    let app = Router::new()
        .route("/health", get(health))
        .route("/activities", get(list_activities).post(set_activity))
        .route("/activities/{socket_id}", delete(clear_activity))
        .route("/detectables/refresh", post(refresh_detectables))
        .route("/metrics", get(metrics))
        .route("/privacy", get(get_privacy).post(set_privacy))
        .with_state(app_state);
    // Internal subscription to update registry for standalone REST usage
    let bus_clone = state.bus.clone();
    let reg_clone = state.registry.clone();
    tokio::spawn(async move {
        let mut rx = bus_clone.subscribe();
        while let Some(evt) = rx.recv().await {
            match evt {
                EventKind::ActivityUpdate { socket_id, payload } => {
                    reg_clone.set(socket_id, payload)
                }
                EventKind::Clear { socket_id } => reg_clone.clear(&socket_id),
                EventKind::PrivacyRefresh => {}
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let actual = listener.local_addr()?.port();
    info!(port = actual, "REST listening");
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    Ok(actual)
}

async fn health() -> &'static str {
    "ok"
}

async fn list_activities(State(s): State<RestState>) -> Json<serde_json::Value> {
    let list = s.registry.non_null();
    Json(serde_json::json!({"activities": list}))
}

async fn set_activity(
    State(s): State<RestState>,
    Json(body): Json<SetActivityPayload>,
) -> Json<serde_json::Value> {
    let sid = body
        .socket_id
        .unwrap_or_else(|| format!("rest-{}", uuid::Uuid::new_v4()));
    let norm = body.activity.normalize();
    if allowed(&s, &norm.name) {
        s.bus.publish(EventKind::ActivityUpdate {
            socket_id: sid.clone(),
            payload: serde_json::to_value(&norm).unwrap(),
        });
    }
    Json(serde_json::json!({"ok": true, "socket_id": sid}))
}

#[derive(Deserialize)]
struct PrivacyUpdate {
    allow: Option<Vec<String>>,
    deny: Option<Vec<String>>,
}

async fn set_privacy(
    State(s): State<RestState>,
    Json(p): Json<PrivacyUpdate>,
) -> Json<serde_json::Value> {
    if let Some(a) = p.allow {
        *s.allow.write() = Some(a);
    }
    if let Some(d) = p.deny {
        *s.deny.write() = Some(d);
    }
    s.bus.publish(EventKind::PrivacyRefresh);
    Json(serde_json::json!({"ok": true}))
}

async fn get_privacy(State(s): State<RestState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "allow": s.allow.read().clone(),
        "deny": s.deny.read().clone(),
    }))
}

fn allowed(s: &RestState, name: &str) -> bool {
    let name_lower = name.to_lowercase();
    if let Some(deny) = s.deny.read().as_ref() {
        if deny.iter().any(|d| name_lower.contains(&d.to_lowercase())) {
            return false;
        }
    }
    if let Some(allow) = s.allow.read().as_ref() {
        return allow.iter().any(|a| name_lower.contains(&a.to_lowercase()));
    }
    true
}

async fn clear_activity(
    State(s): State<RestState>,
    Path(socket_id): Path<String>,
) -> Json<serde_json::Value> {
    s.bus.publish(EventKind::Clear {
        socket_id: socket_id.clone(),
    });
    Json(serde_json::json!({"ok": true}))
}

#[derive(Deserialize)]
struct RefreshQuery {
    force: Option<bool>,
}

async fn refresh_detectables(
    State(s): State<RestState>,
    axum::extract::Query(q): axum::extract::Query<RefreshQuery>,
) -> Json<serde_json::Value> {
    if let Some(det) = &s.detectables {
        let force = q.force.unwrap_or(true); // default force
        match drpc_core::load_detectables_async(force, s.detectables_ttl).await {
            Ok(new_det) => {
                // replace inner list without swapping Arc
                let new_list = new_det.list();
                let mut write_guard = det.inner.write();
                *write_guard = (*new_list).clone();
                return Json(serde_json::json!({"ok": true, "count": write_guard.len()}));
            }
            Err(e) => return Json(serde_json::json!({"ok": false, "error": format!("{e}")})),
        }
    }
    Json(serde_json::json!({"ok": false, "error": "detectables_not_enabled"}))
}

async fn metrics() -> Json<serde_json::Value> {
    Json(drpc_core::metrics::snapshot())
}
