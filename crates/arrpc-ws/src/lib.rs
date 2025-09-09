use arrpc_core::{
    Activity, EventBus, EventKind, MockUser, OutgoingFrame, ReadyConfig, ReadyEvent, RpcCommand,
};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query,
    },
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info, warn};

pub async fn run_ws_server(bus: EventBus) -> anyhow::Result<u16> {
    let port = pick_port().await?;
    let app = Router::new().route("/", get(move |h, q, ws| ws_handler(h, q, ws, bus.clone())));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    info!(port, "WS RPC listening");
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Ok(port)
}

async fn pick_port() -> anyhow::Result<u16> {
    for p in 6463u16..=6472 {
        if tokio::net::TcpListener::bind(("127.0.0.1", p))
            .await
            .is_ok()
        {
            // immediately drop test listener; actual listener created later
            return Ok(p);
        }
    }
    anyhow::bail!("no free port 6463-6472")
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    v: Option<u8>,
    encoding: Option<String>,
    client_id: Option<String>,
}

async fn ws_handler(
    headers: HeaderMap,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
    bus: EventBus,
) -> impl IntoResponse {
    if let Some(orig) = headers.get("origin") {
        let o = orig.to_str().unwrap_or("");
        if std::env::var("ARRPC_DEBUG").ok().as_deref() != Some("1")
            && !matches!(
                o,
                "https://discord.com" | "https://ptb.discord.com" | "https://canary.discord.com"
            )
        {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    if q.v.unwrap_or(1) != 1 {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    if let Some(enc) = &q.encoding {
        if enc.to_lowercase() != "json" {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
    }
    if q.client_id.is_none() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    ws.on_upgrade(|socket| async move { handle_socket(socket, bus).await })
}

async fn handle_socket(mut socket: WebSocket, bus: EventBus) {
    let socket_id = uuid::Uuid::new_v4().to_string();
    let ready = ReadyEvent {
        config: ReadyConfig::default(),
        user: MockUser::default(),
    };
    let frame = OutgoingFrame {
        cmd: RpcCommand::Dispatch,
        evt: Some("READY".into()),
        data: serde_json::to_value(ready).unwrap(),
        nonce: None,
    };
    let json = serde_json::to_string(&frame).unwrap();
    if socket
        .send(axum::extract::ws::Message::Text(json.into()))
        .await
        .is_err()
    {
        return;
    }
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(txt) => {
                debug!(%txt, "recv ws");
                // Placeholder dispatch parsing
                let val: serde_json::Value =
                    serde_json::from_str(&txt).unwrap_or(json!({"_parse": "error"}));
                // Recognize SET_ACTIVITY minimal path
                let maybe_cmd = val
                    .get("cmd")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_uppercase();
                if maybe_cmd == "SET_ACTIVITY" {
                    if let Some(args) = val.get("args").and_then(|a| a.get("activity")) {
                        if let Ok(activity) = serde_json::from_value::<Activity>(args.clone()) {
                            let norm = activity.normalize();
                            let out = json!({"cmd":"DISPATCH","evt":"ACTIVITY_UPDATE","data":{"activity":norm}});
                            if socket
                                .send(Message::Text(out.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                            bus.publish(EventKind::ActivityUpdate {
                                socket_id: socket_id.clone(),
                                payload: serde_json::to_value(norm).unwrap_or(json!({})),
                            });
                            continue;
                        }
                    }
                }
                let out = json!({"cmd":"DISPATCH","evt":"ACK","data":val});
                if socket
                    .send(Message::Text(out.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Message::Close(c) => {
                debug!(?c, "close");
                break;
            }
            Message::Ping(p) => {
                let _ = socket.send(Message::Pong(p)).await;
            }
            Message::Binary(_) => {
                warn!("binary ignored");
            }
            _ => {}
        }
    }
    bus.publish(EventKind::Clear { socket_id });
}
