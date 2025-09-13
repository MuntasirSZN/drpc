use futures::StreamExt;
use tokio_tungstenite::connect_async;

// Negotiating ETF should return a binary READY frame when feature enabled.
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
    match msg {
        Ok(tokio_tungstenite::tungstenite::Message::Binary(bin)) => {
            // Basic sanity: ETF version tag 131 and contains atom 'evt'
            assert_eq!(bin[0], 131, "expected ETF version tag");
            assert!(
                bin.windows(3).any(|w| w == b"evt"),
                "evt key missing in etf frame"
            );
        }
        Ok(other) => panic!("expected binary etf frame got {other:?}"),
        Err(e) => panic!("ws error {e}"),
    }
}
