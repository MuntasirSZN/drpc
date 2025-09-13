use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn ws_rejects_large_payload() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    let _ = ws.next().await; // READY
    // build big > 64KiB payload
    let big = "A".repeat(70 * 1024);
    let txt = format!(
        "{{\"cmd\":\"SET_ACTIVITY\",\"args\":{{\"activity\":{{\"name\":\"{}\"}}}}}}",
        big
    );
    let _ = ws
        .send(tokio_tungstenite::tungstenite::Message::Text(txt.into()))
        .await;
    // After oversize send, connection should close or yield None
    // Expect a Close frame followed by stream end
    let first = ws.next().await;
    match first {
        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {}
        other => panic!("expected Close frame, got {:?}", other),
    }
    let final_msg = ws.next().await; // Should now be None
    assert!(final_msg.is_none(), "connection not terminated after close frame");
}
