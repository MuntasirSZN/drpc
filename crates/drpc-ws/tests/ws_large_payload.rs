use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn ws_rejects_large_payload() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc", port)).await.expect("connect");
    let _ = ws.next().await; // READY
    // build big > 64KiB payload
    let big = "A".repeat(70*1024);
    let txt = format!("{{\"cmd\":\"SET_ACTIVITY\",\"args\":{{\"activity\":{{\"name\":\"{}\"}}}}}}", big);
    let _ = ws.send(tokio_tungstenite::tungstenite::Message::Text(txt.into())).await;
    // After oversize send, connection should close or yield None
    let next = ws.next().await;
    assert!(next.is_none(), "expected socket closed after large payload");
}

