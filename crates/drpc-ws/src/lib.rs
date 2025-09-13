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
    let span = info_span!("ws_connection", %socket_id);
    let _enter = span.enter();
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
                if maybe_cmd == "SET_ACTIVITY"
                    && let Some(args) = val.get("args").and_then(|a| a.get("activity"))
                    && let Ok(activity) = serde_json::from_value::<Activity>(args.clone())
                {
                    let norm = activity.normalize();
                    let out =
                        json!({"cmd":"DISPATCH","evt":"ACTIVITY_UPDATE","data":{"activity":norm}});
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
                if maybe_cmd == "CONNECTIONS_CALLBACK" {
                    let out = json!({"cmd":"DISPATCH","evt":"ERROR","data":{"code":1000}});
                    let _ = socket.send(Message::Text(out.to_string().into())).await;
                    continue;
                }
                if matches!(
                    maybe_cmd.as_str(),
                    "INVITE_BROWSER" | "GUILD_TEMPLATE_BROWSER" | "DEEP_LINK"
                ) {
                    let out = json!({"cmd":"DISPATCH","evt":"ACK","data":{"ok":true}});
                    let _ = socket.send(Message::Text(out.to_string().into())).await;
                    continue;
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
            Message::Binary(bin) => {
                #[cfg(feature = "etf")]
                if use_etf {
                    match decode_frame_etf(&bin) {
                        Ok(val) => {
                            handle_incoming_value(&mut socket, val, &bus, &socket_id).await;
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
}

// shared handler for decoded JSON value (used by ETF path)
async fn handle_incoming_value(
    socket: &mut WebSocket,
    val: serde_json::Value,
    bus: &EventBus,
    socket_id: &str,
) {
    let maybe_cmd = val
        .get("cmd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_uppercase();
    if maybe_cmd == "SET_ACTIVITY" {
        if let Some(args) = val.get("args").and_then(|a| a.get("activity"))
            && let Ok(activity) = serde_json::from_value::<Activity>(args.clone())
        {
            let norm = activity.normalize();
            let out = json!({"cmd":"DISPATCH","evt":"ACTIVITY_UPDATE","data":{"activity":norm}});
            let _ = socket.send(Message::Text(out.to_string().into())).await;
            bus.publish(EventKind::ActivityUpdate {
                socket_id: socket_id.to_string(),
                payload: serde_json::to_value(norm).unwrap_or(json!({})),
            });
        }
        return;
    }
    if maybe_cmd == "CONNECTIONS_CALLBACK" {
        let out = json!({"cmd":"DISPATCH","evt":"ERROR","data":{"code":1000}});
        let _ = socket.send(Message::Text(out.to_string().into())).await;
        return;
    }
    if matches!(
        maybe_cmd.as_str(),
        "INVITE_BROWSER" | "GUILD_TEMPLATE_BROWSER" | "DEEP_LINK"
    ) {
        let out = json!({"cmd":"DISPATCH","evt":"ACK","data":{"ok":true}});
        let _ = socket.send(Message::Text(out.to_string().into())).await;
        return;
    }
    let out = json!({"cmd":"DISPATCH","evt":"ACK","data":val});
    let _ = socket.send(Message::Text(out.to_string().into())).await;
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
