use futures::StreamExt;
use tokio_tungstenite::connect_async;

// Negotiating ETF should still succeed (falling back to JSON until implemented)
#[tokio::test]
async fn ws_etf_query_fallbacks() {
    let bus = drpc_core::EventBus::new();
    let port = drpc_ws::run_ws_server(bus).await.expect("start ws");
    let (mut ws, _resp) = connect_async(format!(
        "ws://127.0.0.1:{}/?v=1&encoding=etf&client_id=abc",
        port
    ))
    .await
    .expect("connect");
    let msg = ws.next().await.expect("ready");
    let txt = match msg {
        Ok(tokio_tungstenite::tungstenite::Message::Text(t)) => t,
        other => panic!("expected text got {other:?}"),
    };
    assert!(txt.contains("READY"));
}
