use axum::{
    Router,
    extract::{
        Query,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
};
use drpc_core::{
    Activity, EventBus, EventKind, MockUser, OutgoingFrame, ReadyConfig, ReadyEvent, RpcCommand,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use tracing::{debug, info, info_span, warn};

pub async fn run_ws_server(bus: EventBus) -> anyhow::Result<u16> {
    // Bind atomically to avoid test races selecting the same port
    let mut listener_opt = None;
    let mut chosen_port = 0u16;
    for p in 6463u16..=6472 {
        match tokio::net::TcpListener::bind(("127.0.0.1", p)).await {
            Ok(l) => {
                listener_opt = Some(l);
                chosen_port = p;
                break;
            }
            Err(_) => continue,
        }
    }
    let listener = if let Some(l) = listener_opt {
        l
    } else {
        // As a last resort (tests), let OS assign a free port
        let l = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        chosen_port = l.local_addr()?.port();
        l
    };
    let app = Router::new().route("/", get(move |h, q, ws| ws_handler(h, q, ws, bus.clone())));
    info!(port = chosen_port, "WS RPC listening");
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Ok(chosen_port)
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
        if std::env::var("DRPC_DEBUG").ok().as_deref() != Some("1")
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
    let encoding = q.encoding.as_deref().unwrap_or("json").to_lowercase();
    if encoding != "json" && encoding != "etf" {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    if q.client_id.is_none() {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    let use_etf = encoding == "etf";
    ws.on_upgrade(move |socket| async move { handle_socket(socket, bus, use_etf).await })
}

async fn handle_socket(mut socket: WebSocket, bus: EventBus, use_etf: bool) {
    let socket_id = uuid::Uuid::new_v4().to_string();
    drpc_core::metrics::ACTIVE_CONNECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let span = info_span!("ws_connection", %socket_id);
    let _enter = span.enter();
    let ready = ReadyEvent {
        v: 1,
        config: ReadyConfig::default(),
        user: MockUser::default(),
    };
    let frame = OutgoingFrame {
        cmd: RpcCommand::Dispatch,
        evt: Some("READY".into()),
        data: serde_json::to_value(ready).unwrap(),
        nonce: None,
        pid: None,
    };
    if use_etf {
        #[cfg(feature = "etf")]
        {
            match encode_frame_etf(&frame) {
                Ok(bin) => {
                    if socket.send(Message::Binary(bin.into())).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    warn!(error=?e, "etf encode failed; fallback json");
                    let json_txt = serde_json::to_string(&frame).unwrap();
                    if socket.send(Message::Text(json_txt.into())).await.is_err() {
                        return;
                    }
                }
            }
        }
        #[cfg(not(feature = "etf"))]
        {
            debug!("ETF requested but feature disabled; fallback json");
            let json_txt = serde_json::to_string(&frame).unwrap();
            if socket.send(Message::Text(json_txt.into())).await.is_err() {
                return;
            }
        }
    } else {
        let json_txt = serde_json::to_string(&frame).unwrap();
        if socket.send(Message::Text(json_txt.into())).await.is_err() {
            return;
        }
    }
    while let Some(Ok(msg)) = socket.next().await {
        match msg {
            Message::Text(txt) => {
                if txt.len() > 64 * 1024 {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
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
                    if let Some(err) = validate_activity_payload(&val) {
                        let _ = send_json_or_etf(&mut socket, &err, use_etf).await;
                        continue;
                    }
                    if let Some(resp) = build_activity_update(&val) {
                        if !send_json_or_etf(&mut socket, &resp, use_etf).await {
                            break;
                        }
                        if let Some(activity) = resp.get("data").and_then(|d| d.get("activity")) {
                            bus.publish(EventKind::ActivityUpdate {
                                socket_id: socket_id.clone(),
                                payload: activity.clone(),
                            });
                        }
                        continue;
                    }
                }
                if maybe_cmd == "PING" {
                    let nonce = val.get("nonce").cloned();
                    let pong = json!({"cmd":"DISPATCH","evt":"PONG","data":{},"nonce":nonce});
                    let _ = send_json_or_etf(&mut socket, &pong, use_etf).await;
                    continue;
                }
                if maybe_cmd == "AUTHORIZE" {
                    let nonce = val.get("nonce").cloned();
                    let args = val.get("args");
                    let client_id_ok = args
                        .and_then(|a| a.get("client_id").and_then(|v| v.as_str()))
                        .is_some();
                    let scopes_ok = args
                        .and_then(|a| a.get("scopes").and_then(|v| v.as_array()))
                        .is_some();
                    let (code, message) = if client_id_ok && scopes_ok {
                        (1000, "Authorization not supported in drpc")
                    } else {
                        (4000, "Invalid payload: missing client_id or scopes")
                    };
                    let out = json!({"cmd":"AUTHORIZE","evt":"ERROR","data":{"code":code,"message":message},"nonce":nonce});
                    let _ = send_json_or_etf(&mut socket, &out, use_etf).await;
                    continue;
                }
                if maybe_cmd == "AUTHENTICATE" {
                    let nonce = val.get("nonce").cloned();
                    let args = val.get("args");
                    let token_ok = args
                        .and_then(|a| a.get("access_token").and_then(|v| v.as_str()))
                        .is_some();
                    let (code, message) = if token_ok {
                        (1000, "Authentication not supported in drpc")
                    } else {
                        (4000, "Invalid payload: missing access_token")
                    };
                    let out = json!({"cmd":"AUTHENTICATE","evt":"ERROR","data":{"code":code,"message":message},"nonce":nonce});
                    let _ = send_json_or_etf(&mut socket, &out, use_etf).await;
                    continue;
                }
                if matches!(maybe_cmd.as_str(), "SUBSCRIBE" | "UNSUBSCRIBE") {
                    let nonce = val.get("nonce").cloned();
                    let evt_name = val
                        .get("args")
                        .and_then(|a| a.get("event"))
                        .and_then(|e| e.as_str());
                    if evt_name.is_none() {
                        let err = json!({"cmd": maybe_cmd, "evt":"ERROR","data":{"code":4000, "message":"Invalid payload: missing args.event"}, "nonce": nonce});
                        let _ = send_json_or_etf(&mut socket, &err, use_etf).await;
                        continue;
                    }
                    // Minimal validation: allow known events else error
                    let allowed = matches!(
                        evt_name.unwrap(),
                        "GUILD_STATUS"
                            | "GUILD_CREATE"
                            | "CHANNEL_CREATE"
                            | "VOICE_CHANNEL_SELECT"
                            | "VOICE_STATE_CREATE"
                            | "VOICE_STATE_UPDATE"
                            | "VOICE_STATE_DELETE"
                            | "VOICE_SETTINGS_UPDATE"
                            | "VOICE_CONNECTION_STATUS"
                            | "SPEAKING_START"
                            | "SPEAKING_STOP"
                            | "MESSAGE_CREATE"
                            | "MESSAGE_UPDATE"
                            | "MESSAGE_DELETE"
                            | "NOTIFICATION_CREATE"
                            | "ACTIVITY_JOIN"
                            | "ACTIVITY_SPECTATE"
                            | "ACTIVITY_JOIN_REQUEST"
                    );
                    if !allowed {
                        let err = json!({"cmd": maybe_cmd, "evt":"ERROR","data":{"code":4000, "message":"Invalid payload: unknown event"}, "nonce": nonce});
                        let _ = send_json_or_etf(&mut socket, &err, use_etf).await;
                        continue;
                    }
                    let ack = json!({"cmd": maybe_cmd, "evt":"ACK","data":{},"nonce":nonce});
                    let _ = send_json_or_etf(&mut socket, &ack, use_etf).await;
                    continue;
                }
                if maybe_cmd == "CONNECTIONS_CALLBACK" {
                    let nonce = val.get("nonce").cloned();
                    let out = json!({
                        "cmd": "CONNECTIONS_CALLBACK",
                        "evt": "ERROR",
                        "data": {"code": 1000, "message": "Connections callback not supported"},
                        "nonce": nonce,
                    });
                    let _ = send_json_or_etf(&mut socket, &out, use_etf).await;
                    continue;
                }
                if matches!(
                    maybe_cmd.as_str(),
                    "INVITE_BROWSER" | "GUILD_TEMPLATE_BROWSER" | "DEEP_LINK"
                ) {
                    let nonce = val.get("nonce").cloned();
                    let out =
                        json!({"cmd": maybe_cmd, "evt":"ACK","data":{"ok":true}, "nonce": nonce});
                    let _ = send_json_or_etf(&mut socket, &out, use_etf).await;
                    continue;
                }
                // Unknown command -> ERROR
                let nonce = val.get("nonce").cloned();
                let err = json!({
                    "cmd": maybe_cmd,
                    "evt": "ERROR",
                    "data": {"code": 4000, "message": "Invalid payload or unknown command"},
                    "nonce": nonce,
                });
                let _ = send_json_or_etf(&mut socket, &err, use_etf).await;
            }
            Message::Close(c) => {
                debug!(?c, "close");
                break;
            }
            Message::Ping(p) => {
                let _ = socket.send(Message::Pong(p)).await;
            }
            Message::Binary(bin) => {
                #[cfg(feature = "etf")]
                if use_etf {
                    match decode_frame_etf(&bin) {
                        Ok(val) => {
                            handle_incoming_value(&mut socket, val, &bus, &socket_id, use_etf)
                                .await;
                            continue;
                        }
                        Err(e) => warn!(error=?e, "etf decode failed; ignoring frame"),
                    }
                }
                warn!("binary ignored");
            }
            _ => {}
        }
    }
    bus.publish(EventKind::Clear { socket_id });
    drpc_core::metrics::ACTIVE_CONNECTIONS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

// shared handler for decoded JSON value (used by ETF path)
async fn handle_incoming_value(
    socket: &mut WebSocket,
    val: serde_json::Value,
    bus: &EventBus,
    socket_id: &str,
    use_etf: bool,
) {
    let maybe_cmd = val
        .get("cmd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_uppercase();
    if maybe_cmd == "SET_ACTIVITY" {
        if let Some(err) = validate_activity_payload(&val) {
            let _ = send_json_or_etf(socket, &err, use_etf).await;
        } else if let Some(resp) = build_activity_update(&val) {
            let _ = send_json_or_etf(socket, &resp, use_etf).await;
            if let Some(activity) = resp.get("data").and_then(|d| d.get("activity")) {
                bus.publish(EventKind::ActivityUpdate {
                    socket_id: socket_id.to_string(),
                    payload: activity.clone(),
                });
            }
        }
        return;
    }
    if maybe_cmd == "CONNECTIONS_CALLBACK" {
        let out = json!({"cmd":"CONNECTIONS_CALLBACK","evt":"ERROR","data":{"code":1000, "message":"Connections callback not supported"}});
        let _ = send_json_or_etf(socket, &out, use_etf).await;
        return;
    }
    if matches!(
        maybe_cmd.as_str(),
        "INVITE_BROWSER" | "GUILD_TEMPLATE_BROWSER" | "DEEP_LINK" | "SUBSCRIBE" | "UNSUBSCRIBE"
    ) {
        let out = json!({"cmd": maybe_cmd, "evt":"ACK","data":{}});
        let _ = send_json_or_etf(socket, &out, use_etf).await;
        return;
    }
    if maybe_cmd == "AUTHORIZE" {
        let args = val.get("args");
        let client_id_ok = args
            .and_then(|a| a.get("client_id").and_then(|v| v.as_str()))
            .is_some();
        let scopes_ok = args
            .and_then(|a| a.get("scopes").and_then(|v| v.as_array()))
            .is_some();
        let (code, message) = if client_id_ok && scopes_ok {
            (1000, "Authorization not supported in drpc")
        } else {
            (4000, "Invalid payload: missing client_id or scopes")
        };
        let out = json!({"cmd":"AUTHORIZE","evt":"ERROR","data":{"code":code,"message":message}});
        let _ = send_json_or_etf(socket, &out, use_etf).await;
        return;
    }
    if maybe_cmd == "AUTHENTICATE" {
        let args = val.get("args");
        let token_ok = args
            .and_then(|a| a.get("access_token").and_then(|v| v.as_str()))
            .is_some();
        let (code, message) = if token_ok {
            (1000, "Authentication not supported in drpc")
        } else {
            (4000, "Invalid payload: missing access_token")
        };
        let out =
            json!({"cmd":"AUTHENTICATE","evt":"ERROR","data":{"code":code,"message":message}});
        let _ = send_json_or_etf(socket, &out, use_etf).await;
        return;
    }
    let out =
        json!({"cmd": maybe_cmd, "evt":"ERROR","data":{"code":4000, "message":"Unknown command"}});
    let _ = send_json_or_etf(socket, &out, use_etf).await;
}
#[cfg(feature = "etf")]
fn encode_frame_etf(frame: &OutgoingFrame) -> anyhow::Result<Vec<u8>> {
    use eetf::*;
    let mut map = HashMap::new();
    map.insert(
        Term::from(Atom::from("cmd")),
        Term::from(String::from(frame.cmd_as_str())),
    );
    if let Some(evt) = &frame.evt {
        map.insert(Term::from(Atom::from("evt")), Term::from(evt.clone()));
    }
    map.insert(Term::from(Atom::from("data")), json_to_term(&frame.data)?);
    if let Some(n) = &frame.nonce {
        map.insert(Term::from(Atom::from("nonce")), Term::from(n.to_string()));
    }
    let term = Term::from(Map::from(map));
    let mut buf = Vec::new();
    term.encode(&mut buf)?;
    Ok(buf)
}

#[cfg(feature = "etf")]
fn decode_frame_etf(bin: &[u8]) -> anyhow::Result<serde_json::Value> {
    use eetf::Term;
    let term = Term::decode(bin)?;
    term_to_json(&term)
}

#[cfg(feature = "etf")]
fn json_to_term(v: &serde_json::Value) -> anyhow::Result<eetf::Term> {
    use eetf::*;
    Ok(match v {
        serde_json::Value::Null => Term::from(Atom::from("nil")),
        serde_json::Value::Bool(b) => Term::from(if *b {
            Atom::from("true")
        } else {
            Atom::from("false")
        }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Term::from(FixInteger::from(i as i32))
            } else if let Some(u) = n.as_u64() {
                Term::from(FixInteger::from(u as i32))
            } else {
                Term::from(Float::try_from(n.as_f64().unwrap())?)
            }
        }
        serde_json::Value::String(s) => Term::from(s.clone()),
        serde_json::Value::Array(arr) => {
            let elements: Vec<Term> = arr.iter().map(|x| json_to_term(x).unwrap()).collect();
            Term::from(List::from(elements))
        }
        serde_json::Value::Object(obj) => {
            let mut map: HashMap<Term, Term> = HashMap::new();
            for (k, val) in obj.iter() {
                map.insert(Term::from(k.clone()), json_to_term(val)?);
            }
            Term::from(Map::from(map))
        }
    })
}

#[cfg(feature = "etf")]
fn term_to_json(t: &eetf::Term) -> anyhow::Result<serde_json::Value> {
    use eetf::*;
    use serde_json::{Map as JsonMap, Value};
    Ok(match t {
        Term::Atom(a) => Value::String(a.name.clone()),
        Term::FixInteger(i) => Value::Number(i.value.into()),
        Term::BigInteger(bi) => Value::String(bi.value.to_string()),
        Term::Float(f) => serde_json::Number::from_f64(f.value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Term::Binary(b) => Value::String(String::from_utf8_lossy(&b.bytes).to_string()),
        Term::ByteList(bl) => Value::String(String::from_utf8_lossy(&bl.bytes).to_string()),
        Term::List(l) => Value::Array(
            l.elements
                .iter()
                .map(|e| term_to_json(e).unwrap())
                .collect(),
        ),
        Term::Tuple(tup) => Value::Array(
            tup.elements
                .iter()
                .map(|e| term_to_json(e).unwrap())
                .collect(),
        ),
        Term::Map(m) => {
            let mut jm = JsonMap::new();
            for (k, v) in &m.map {
                let key = match k {
                    Term::Atom(a) => a.name.clone(),
                    Term::Binary(b) => String::from_utf8_lossy(&b.bytes).to_string(),
                    Term::ByteList(bl) => String::from_utf8_lossy(&bl.bytes).to_string(),
                    other => other.to_string(),
                };
                jm.insert(key, term_to_json(v)?);
            }
            Value::Object(jm)
        }
        other => Value::String(format!("{:?}", other)),
    })
}

// Provide a string conversion for RpcCommand for ETF map key value.
trait RpcCommandAsStr {
    fn cmd_as_str(&self) -> &'static str;
}
impl RpcCommandAsStr for OutgoingFrame {
    fn cmd_as_str(&self) -> &'static str {
        match self.cmd {
            RpcCommand::Dispatch => "DISPATCH",
            RpcCommand::SetActivity => "SET_ACTIVITY",
            RpcCommand::InviteBrowser => "INVITE_BROWSER",
            RpcCommand::GuildTemplateBrowser => "GUILD_TEMPLATE_BROWSER",
            RpcCommand::DeepLink => "DEEP_LINK",
            RpcCommand::ConnectionsCallback => "CONNECTIONS_CALLBACK",
        }
    }
}
// Build a unified ACTIVITY_UPDATE dispatch frame from raw incoming value
fn build_activity_update(raw: &serde_json::Value) -> Option<serde_json::Value> {
    let args = raw.get("args")?;
    let act = args.get("activity")?;
    let activity: Activity = serde_json::from_value(act.clone()).ok()?;
    let norm = activity.normalize();
    let pid = args.get("pid").and_then(|p| p.as_u64()).map(|p| p as u32);
    let nonce = raw.get("nonce").cloned();
    Some(json!({
        "cmd": "DISPATCH",
        "evt": "ACTIVITY_UPDATE",
        "data": {"activity": norm},
        "pid": pid,
        "nonce": nonce,
    }))
}

async fn send_json_or_etf(
    socket: &mut WebSocket,
    frame: &serde_json::Value,
    use_etf: bool,
) -> bool {
    if use_etf {
        #[cfg(feature = "etf")]
        {
            match encode_value_etf(frame) {
                Ok(bin) => return socket.send(Message::Binary(bin.into())).await.is_ok(),
                Err(e) => warn!(error=?e, "etf encode failed; falling back to json"),
            }
        }
    }
    socket
        .send(Message::Text(frame.to_string().into()))
        .await
        .is_ok()
}

// Validate activity payload per docs (partial): max 2 buttons
fn validate_activity_payload(raw: &serde_json::Value) -> Option<serde_json::Value> {
    let nonce = raw.get("nonce").cloned();
    let args = match raw.get("args") {
        Some(a) => a,
        None => {
            return Some(json!({
                "cmd": "SET_ACTIVITY",
                "evt": "ERROR",
                "data": {"code": 4000, "message": "Invalid payload: missing args"},
                "nonce": nonce,
            }));
        }
    };
    let act = match args.get("activity") {
        Some(a) => a,
        None => {
            return Some(json!({
                "cmd": "SET_ACTIVITY",
                "evt": "ERROR",
                "data": {"code": 4000, "message": "Invalid payload: missing activity"},
                "nonce": nonce,
            }));
        }
    };
    if !act.is_object() {
        return Some(json!({
            "cmd": "SET_ACTIVITY",
            "evt": "ERROR",
            "data": {"code": 4000, "message": "Invalid payload: activity must be object"},
            "nonce": nonce,
        }));
    }
    if let Some(btns) = act.get("buttons").and_then(|b| b.as_array()) {
        if btns.len() > 2 {
            return Some(json!({
                "cmd": "SET_ACTIVITY",
                "evt": "ERROR",
                "data": {"code": 4002, "message": "Invalid payload: max 2 buttons"},
                "nonce": nonce,
            }));
        }
        // ensure each button has string label/url
        for b in btns {
            let label_ok = b.get("label").and_then(|v| v.as_str()).is_some();
            let url_ok = b.get("url").and_then(|v| v.as_str()).is_some();
            if !label_ok || !url_ok {
                return Some(json!({
                    "cmd": "SET_ACTIVITY",
                    "evt": "ERROR",
                    "data": {"code": 4000, "message": "Invalid payload: button requires label and url strings"},
                    "nonce": nonce,
                }));
            }
        }
    }
    None
}

#[cfg(feature = "etf")]
fn encode_value_etf(v: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
    let term = json_to_term(v)?;
    let mut buf = Vec::new();
    term.encode(&mut buf)?;
    Ok(buf)
}
