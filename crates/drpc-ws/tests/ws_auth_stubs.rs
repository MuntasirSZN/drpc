use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn ws_authorize_and_authenticate_error_stubs() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    let _ = ws.next().await; // READY
    // AUTHORIZE missing fields
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "{\"cmd\":\"AUTHORIZE\",\"nonce\":\"na\"}".into(),
    ))
    .await
    .unwrap();
    let r1 = ws.next().await.expect("r1").expect("ok");
    let t1 = match r1 {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        _ => panic!("expected text"),
    };
    let v1: Value = serde_json::from_str(&t1).unwrap();
    assert_eq!(v1["evt"].as_str(), Some("ERROR"));
    assert_eq!(v1["cmd"].as_str(), Some("AUTHORIZE"));
    assert_eq!(v1["data"]["code"].as_u64(), Some(4000));
    // AUTHENTICATE missing token
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "{\"cmd\":\"AUTHENTICATE\",\"nonce\":\"nt\"}".into(),
    ))
    .await
    .unwrap();
    let r2 = ws.next().await.expect("r2").expect("ok");
    let t2 = match r2 {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        _ => panic!("expected text"),
    };
    let v2: Value = serde_json::from_str(&t2).unwrap();
    assert_eq!(v2["evt"].as_str(), Some("ERROR"));
    assert_eq!(v2["cmd"].as_str(), Some("AUTHENTICATE"));
    assert_eq!(v2["data"]["code"].as_u64(), Some(4000));
}
