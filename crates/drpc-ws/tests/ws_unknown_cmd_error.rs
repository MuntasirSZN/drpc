use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use serde_json::Value;

#[tokio::test]
async fn ws_unknown_command_returns_error() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc", port)).await.expect("connect");
    let _ = ws.next().await; // READY
    let payload = serde_json::json!({"cmd":"DOES_NOT_EXIST","nonce":"nx","args":{}});
    ws.send(tokio_tungstenite::tungstenite::Message::Text(payload.to_string().into())).await.unwrap();
    let resp = ws.next().await.expect("resp").expect("ok");
    let txt = match resp { tokio_tungstenite::tungstenite::Message::Text(t) => t, _ => panic!("expected text") };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("ERROR"));
    assert_eq!(v["data"]["code"].as_u64(), Some(4000));
    assert_eq!(v["nonce"].as_str(), Some("nx"));
}

