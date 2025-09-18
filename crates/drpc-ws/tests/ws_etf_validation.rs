use futures::{SinkExt, StreamExt};
use tokio_tungstenite::connect_async;

async fn boot_etf() -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!("ws://127.0.0.1:{}/?v=1&encoding=etf&client_id=abc", port)).await.expect("connect");
    let _ = ws.next().await; // READY (binary)
    ws
}

#[tokio::test]
async fn etf_missing_args_errors_binary() {
    let mut ws = boot_etf().await;
    ws.send(tokio_tungstenite::tungstenite::Message::Text("{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"n1\"}".into())).await.unwrap();
    match ws.next().await.unwrap().unwrap() {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131);
            assert!(bin.windows(5).any(|w| w == b"ERROR"));
            assert!(bin.windows(4).any(|w| w == b"4000"));
        }
        other => panic!("expected binary, got {other:?}"),
    }
}

#[tokio::test]
async fn etf_missing_activity_errors_binary() {
    let mut ws = boot_etf().await;
    let p = serde_json::json!({"cmd":"SET_ACTIVITY","nonce":"n2","args":{}});
    ws.send(tokio_tungstenite::tungstenite::Message::Text(p.to_string().into())).await.unwrap();
    match ws.next().await.unwrap().unwrap() {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131);
            assert!(bin.windows(5).any(|w| w == b"ERROR"));
            assert!(bin.windows(4).any(|w| w == b"4000"));
        }
        other => panic!("expected binary, got {other:?}"),
    }
}

#[tokio::test]
async fn etf_non_object_activity_errors_binary() {
    let mut ws = boot_etf().await;
    let p = serde_json::json!({"cmd":"SET_ACTIVITY","nonce":"n3","args":{"activity":123}});
    ws.send(tokio_tungstenite::tungstenite::Message::Text(p.to_string().into())).await.unwrap();
    match ws.next().await.unwrap().unwrap() {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131);
            assert!(bin.windows(5).any(|w| w == b"ERROR"));
            assert!(bin.windows(4).any(|w| w == b"4000"));
        }
        other => panic!("expected binary, got {other:?}"),
    }
}

#[tokio::test]
async fn etf_invalid_button_shape_errors_binary() {
    let mut ws = boot_etf().await;
    let p = serde_json::json!({
        "cmd":"SET_ACTIVITY","nonce":"n4","args":{"activity":{
            "name":"Bads","buttons":[{"label":1,"url":"u"}]}}
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(p.to_string().into())).await.unwrap();
    match ws.next().await.unwrap().unwrap() {
        tokio_tungstenite::tungstenite::Message::Binary(bin) => {
            assert_eq!(bin[0], 131);
            assert!(bin.windows(5).any(|w| w == b"ERROR"));
            assert!(bin.windows(4).any(|w| w == b"4000"));
        }
        other => panic!("expected binary, got {other:?}"),
    }
}

