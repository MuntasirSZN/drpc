use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use serde_json::Value;

#[tokio::test]
async fn ws_set_activity_buttons_over_limit_errors() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    // READY
    let _ = ws.next().await;
    // 3 buttons -> invalid
    let payload = serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "nonce": "nb",
        "args": {"activity": {
            "name": "TooMany",
            "buttons": [
                {"label":"L1","url":"u1"},
                {"label":"L2","url":"u2"},
                {"label":"L3","url":"u3"}
            ]
        }}
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    let resp = ws.next().await.expect("resp").expect("ok");
    let txt = match resp { tokio_tungstenite::tungstenite::Message::Text(t) => t, other => panic!("expected text got {other:?}") };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("ERROR"));
    assert_eq!(v["cmd"].as_str(), Some("SET_ACTIVITY"));
    assert_eq!(v["data"]["code"].as_u64(), Some(4002));
}

