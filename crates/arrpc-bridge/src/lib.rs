use arrpc_core::{EventBus, EventKind};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

pub struct Bridge {
    port: u16,
}

impl Bridge {
    pub async fn run(bus: EventBus, port: Option<u16>) -> anyhow::Result<Self> {
        let port = port.unwrap_or(1337);
        let state = Arc::new(BridgeState {
            activities: RwLock::new(HashMap::new()),
            bus: bus.clone(),
            clients: RwLock::new(Vec::new()),
        });
        // subscriber task
        let sub_state = state.clone();
        tokio::spawn(async move {
            bridge_subscriber(bus, sub_state).await;
        });
        let app = Router::new().route(
            "/",
            get(move |ws: WebSocketUpgrade| bridge_handler(ws, state.clone())),
        );
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
        info!(port, "Bridge listening");
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Ok(Self { port })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
}

struct BridgeState {
    activities: RwLock<HashMap<String, serde_json::Value>>, // socket_id -> activity json
    bus: EventBus,
    clients: RwLock<Vec<tokio::sync::mpsc::UnboundedSender<String>>>,
}

async fn bridge_handler(ws: WebSocketUpgrade, state: Arc<BridgeState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| async move { handle_socket(socket, state).await })
}

async fn handle_socket(mut socket: WebSocket, state: Arc<BridgeState>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    state.clients.write().push(tx);
    let snapshot: Vec<_> = state
        .activities
        .read()
        .iter()
        .filter_map(|(k, v)| {
            if v.is_null() {
                None
            } else {
                Some((k.clone(), v.clone()))
            }
        })
        .collect();
    for (sid, act) in snapshot {
        let msg = serde_json::json!({"socketId": sid, "activity": act});
        if socket.send(Message::Text(msg.to_string())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            Some(Ok(msg)) = socket.next() => { if matches!(msg, Message::Close(_)) { break; } }
            Some(out) = rx.recv() => { if socket.send(Message::Text(out)).await.is_err() { break; } }
            else => break,
        }
    }
}

async fn bridge_subscriber(bus: EventBus, state: Arc<BridgeState>) {
    let mut rx = bus.subscribe();
    while let Some(evt) = rx.recv().await {
        match evt {
            EventKind::ActivityUpdate { socket_id, payload } => {
                state
                    .activities
                    .write()
                    .insert(socket_id.clone(), payload.clone());
                broadcast(
                    &state,
                    serde_json::json!({"socketId": socket_id, "activity": payload}),
                )
                .await;
            }
            EventKind::Clear { socket_id } => {
                state
                    .activities
                    .write()
                    .insert(socket_id.clone(), serde_json::Value::Null);
                broadcast(
                    &state,
                    serde_json::json!({"socketId": socket_id, "activity": serde_json::Value::Null}),
                )
                .await;
            }
        }
    }
}

async fn broadcast(state: &Arc<BridgeState>, msg: serde_json::Value) {
    let text = msg.to_string();
    for sender in state.clients.read().iter() {
        let _ = sender.send(text.clone());
    }
}
