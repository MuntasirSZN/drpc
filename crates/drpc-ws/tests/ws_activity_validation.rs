use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;
use serde_json::Value;

async fn boot() -> (tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, String) {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc", port)).await.expect("connect");
    let _ = ws.next().await; // READY
    (ws, port.to_string())
}

#[tokio::test]
async fn missing_args_errors() {
    let (mut ws, _) = boot().await;
    ws.send(tokio_tungstenite::tungstenite::Message::Text("{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"n1\"}".into())).await.unwrap();
    let msg = ws.next().await.unwrap().unwrap();
    let txt = match msg { tokio_tungstenite::tungstenite::Message::Text(t)=>t,_=>panic!("expected text")};
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("ERROR"));
    assert_eq!(v["data"]["code"].as_u64(), Some(4000));
}

#[tokio::test]
async fn missing_activity_errors() {
    let (mut ws, _) = boot().await;
    let p = serde_json::json!({"cmd":"SET_ACTIVITY","nonce":"n2","args":{}});
    ws.send(tokio_tungstenite::tungstenite::Message::Text(p.to_string().into())).await.unwrap();
    let txt = match ws.next().await.unwrap().unwrap() { tokio_tungstenite::tungstenite::Message::Text(t)=>t,_=>panic!() };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("ERROR"));
    assert_eq!(v["data"]["code"].as_u64(), Some(4000));
}

#[tokio::test]
async fn non_object_activity_errors() {
    let (mut ws, _) = boot().await;
    let p = serde_json::json!({"cmd":"SET_ACTIVITY","nonce":"n3","args":{"activity":123}});
    ws.send(tokio_tungstenite::tungstenite::Message::Text(p.to_string().into())).await.unwrap();
    let txt = match ws.next().await.unwrap().unwrap() { tokio_tungstenite::tungstenite::Message::Text(t)=>t,_=>panic!() };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("ERROR"));
    assert_eq!(v["data"]["code"].as_u64(), Some(4000));
}

#[tokio::test]
async fn invalid_button_shape_errors() {
    let (mut ws, _) = boot().await;
    let p = serde_json::json!({
        "cmd":"SET_ACTIVITY","nonce":"n4","args":{"activity":{
            "name":"Bads","buttons":[{"label":1,"url":"u"}]}}
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(p.to_string().into())).await.unwrap();
    let txt = match ws.next().await.unwrap().unwrap() { tokio_tungstenite::tungstenite::Message::Text(t)=>t,_=>panic!() };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(v["evt"].as_str(), Some("ERROR"));
    assert_eq!(v["data"]["code"].as_u64(), Some(4000));
}

