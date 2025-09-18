use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use serde_json::Value;

#[tokio::test]
async fn ws_ready_has_version_and_ping_pong() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc", port)).await.expect("connect");
    // READY
    let msg = ws.next().await.expect("ready").expect("ok");
    let txt = match msg { tokio_tungstenite::tungstenite::Message::Text(t) => t, other => panic!("expected text got {other:?}") };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("READY"));
    assert_eq!(v["data"]["v"].as_u64(), Some(1));
    // PING
    ws.send(tokio_tungstenite::tungstenite::Message::Text("{\"cmd\":\"PING\",\"nonce\":\"n1\"}".into())).await.unwrap();
    let pong = ws.next().await.expect("pong").expect("ok");
    let txt = match pong { tokio_tungstenite::tungstenite::Message::Text(t) => t, _ => panic!("expected text") };
    let p: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(p["evt"].as_str(), Some("PONG"));
    assert_eq!(p["nonce"].as_str(), Some("n1"));
}

