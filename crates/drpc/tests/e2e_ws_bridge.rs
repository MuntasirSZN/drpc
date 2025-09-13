use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

// Full end-to-end: start main binary components in-process (ws + bridge) and ensure
// activity set over WS is replayed to a later Bridge client.
#[tokio::test]
async fn e2e_ws_activity_replay() {
    // Initialize tracing minimal once
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let bus = drpc_core::EventBus::new();
    // Start WS server
    let ws_port = drpc_ws::run_ws_server(bus.clone()).await.expect("ws start");
    // Start Bridge
    let bridge = drpc_bridge::Bridge::run(bus.clone(), Some(0u16))
        .await
        .expect("bridge start");
    let bridge_port = bridge.port();

    // Connect WS client & receive READY
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc",
        ws_port
    ))
    .await
    .expect("connect ws");
    let _ready = ws.next().await.expect("ready").expect("ws text");

    // Send SET_ACTIVITY
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "{\"cmd\":\"SET_ACTIVITY\",\"args\":{\"activity\":{\"name\":\"ReplayTest\"}}}".into(),
    ))
    .await
    .unwrap();
    // Wait for ACTIVITY_UPDATE
    let update = ws.next().await.expect("activity").expect("ok");
    let txt = match update {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        _ => panic!("expected text"),
    };
    assert!(txt.contains("ACTIVITY_UPDATE"));

    // Now connect a Bridge client and ensure it replays the activity
    // Retry connect briefly in case listener not fully ready
    let mut attempt = 0;
    let mut bridge_ws_opt = None;
    while attempt < 10 {
        match connect_async(format!("ws://127.0.0.1:{}/", bridge_port)).await {
            Ok(pair) => {
                bridge_ws_opt = Some(pair.0);
                break;
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                attempt += 1;
            }
        }
    }
    let mut bridge_ws = bridge_ws_opt.expect("bridge connect after retries");
    // First message should contain our activity
    let replay = bridge_ws
        .next()
        .await
        .expect("bridge msg")
        .expect("bridge text");
    let replay_txt = match replay {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        _ => panic!("expected text"),
    };
    assert!(
        replay_txt.contains("ReplayTest"),
        "replay missing activity: {replay_txt}"
    );
}
