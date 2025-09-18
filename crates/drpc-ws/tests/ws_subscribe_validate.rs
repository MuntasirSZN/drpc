use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn ws_subscribe_requires_event_and_valid_name() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    let _ = ws.next().await; // READY
    // missing args.event
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "{\"cmd\":\"SUBSCRIBE\",\"nonce\":\"n1\",\"args\":{}}".into(),
    ))
    .await
    .unwrap();
    let v1: Value = serde_json::from_str(
        match ws.next().await.unwrap().unwrap() {
            tokio_tungstenite::tungstenite::Message::Text(t) => t,
            _ => panic!(),
        }
        .as_str(),
    )
    .unwrap();
    assert_eq!(v1["evt"].as_str(), Some("ERROR"));
    // unknown event name
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "{\"cmd\":\"SUBSCRIBE\",\"nonce\":\"n2\",\"args\":{\"event\":\"XYZ\"}}".into(),
    ))
    .await
    .unwrap();
    let v2: Value = serde_json::from_str(
        match ws.next().await.unwrap().unwrap() {
            tokio_tungstenite::tungstenite::Message::Text(t) => t,
            _ => panic!(),
        }
        .as_str(),
    )
    .unwrap();
    assert_eq!(v2["evt"].as_str(), Some("ERROR"));
    // valid event name
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "{\"cmd\":\"SUBSCRIBE\",\"nonce\":\"n3\",\"args\":{\"event\":\"GUILD_STATUS\"}}".into(),
    ))
    .await
    .unwrap();
    let v3: Value = serde_json::from_str(
        match ws.next().await.unwrap().unwrap() {
            tokio_tungstenite::tungstenite::Message::Text(t) => t,
            _ => panic!(),
        }
        .as_str(),
    )
    .unwrap();
    assert_eq!(v3["evt"].as_str(), Some("ACK"));
}
