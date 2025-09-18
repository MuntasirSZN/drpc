use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

#[tokio::test]
async fn ws_etf_unknown_command_binary_error() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=etf&client_id=abc", port)).await.expect("connect");
    // READY (binary) discard
    let _ = ws.next().await;
    // Send unknown command
    ws.send(tokio_tungstenite::tungstenite::Message::Text("{\"cmd\":\"WHAT\",\"nonce\":\"n\"}".into())).await.unwrap();
    let resp = ws.next().await.expect("resp").expect("ok");
    match resp {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131, "ETF tag");
            // crude markers
            assert!(bin.windows(5).any(|w| w == b"ERROR"), "missing ERROR");
            assert!(bin.windows(4).any(|w| w == b"4000"), "missing code 4000");
        }
        other => panic!("expected binary, got {other:?}"),
    }
}

#[tokio::test]
async fn ws_etf_buttons_over_limit_binary_error() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=etf&client_id=abc", port)).await.expect("connect");
    let _ = ws.next().await; // READY
    let payload = serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "nonce": "nb",
        "args": {"activity": {"name":"TooMany","buttons":[{"label":"a","url":"u"},{"label":"b","url":"v"},{"label":"c","url":"w"}]}}
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(payload.to_string().into())).await.unwrap();
    let resp = ws.next().await.expect("resp").expect("ok");
    match resp {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131);
            assert!(bin.windows(5).any(|w| w == b"ERROR"));
            assert!(bin.windows(4).any(|w| w == b"4002"));
        }
        other => panic!("expected binary, got {other:?}"),
    }
}

