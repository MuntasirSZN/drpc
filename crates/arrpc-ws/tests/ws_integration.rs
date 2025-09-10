use tokio_tungstenite::connect_async;
use url::Url;

#[tokio::test]
async fn ws_ready_and_set_activity() {
    // Start a temporary server instance by invoking library run function.
    let bus = arrpc_core::EventBus::new();
    let port = arrpc_ws::run_ws_server(bus).await.expect("start ws");
    let url = Url::parse(&format!("ws://127.0.0.1:{}/?v=1&encoding=json&client_id=123", port)).unwrap();
    let (mut ws, _resp) = connect_async(url).await.expect("connect");
    // Read READY
    let msg = ws.next().await.expect("ready").expect("ok");
    let txt = match msg { tokio_tungstenite::tungstenite::Message::Text(t) => t, _ => panic!("expected text") };
    assert!(txt.contains("READY"));
    // Send SET_ACTIVITY
    ws.send(tokio_tungstenite::tungstenite::Message::Text("{\"cmd\":\"SET_ACTIVITY\",\"args\":{\"activity\":{\"name\":\"Test\"}}}".into())).await.unwrap();
    // Receive ACTIVITY_UPDATE
    let update = ws.next().await.expect("activity").expect("ok");
    let txt = match update { tokio_tungstenite::tungstenite::Message::Text(t) => t, _ => panic!("expected text") };
    assert!(txt.contains("ACTIVITY_UPDATE"));
}

