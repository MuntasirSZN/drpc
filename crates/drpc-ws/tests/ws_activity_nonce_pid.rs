use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn ws_set_activity_nonce_pid_json() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=json&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    // READY
    let _ = ws.next().await.expect("ready");
    let nonce = "n-123";
    let pid = 4242u32;
    let msg = format!(
        "{{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"{}\",\"args\":{{\"pid\":{},\"activity\":{{\"name\":\"GameX\"}}}}}}",
        nonce, pid
    );
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
        .await
        .unwrap();
    let resp = ws.next().await.expect("update").expect("ok");
    let txt = match resp {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        other => panic!("expected text got {other:?}"),
    };
    let v: Value = serde_json::from_str(&txt).unwrap();
    assert_eq!(
        v.get("evt").and_then(|e| e.as_str()),
        Some("ACTIVITY_UPDATE")
    );
    assert_eq!(v.get("nonce").and_then(|n| n.as_str()), Some(nonce));
    assert_eq!(v.get("pid").and_then(|p| p.as_u64()), Some(pid as u64));
    assert_eq!(v["data"]["activity"]["name"].as_str(), Some("GameX"));
}

#[tokio::test]
async fn ws_set_activity_nonce_pid_etf() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=etf&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    // READY (binary ETF) discard
    let _ = ws.next().await.expect("ready");
    let nonce = "n-999";
    let pid = 777u32;
    let msg = format!(
        "{{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"{}\",\"args\":{{\"pid\":{},\"activity\":{{\"name\":\"ETFGame\"}}}}}}",
        nonce, pid
    );
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
        .await
        .unwrap();
    // Expect ETF binary ACTIVITY_UPDATE
    let resp = ws.next().await.expect("etf update").expect("ok");
    match resp {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131, "ETF version tag");
            // crude check: ensure nonce bytes appear
            let needle = nonce.as_bytes();
            assert!(
                bin.windows(needle.len()).any(|w| w == needle),
                "nonce missing in ETF payload"
            );
            // ensure activity name appears
            assert!(
                bin.windows(7).any(|w| w == b"ETFGame"),
                "activity name missing"
            );
        }
        other => panic!("expected binary ETF frame got {other:?}"),
    }
}
