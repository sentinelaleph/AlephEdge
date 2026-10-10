//! Spike: the official ccxt crate reaches Bybit's public USDT perp market.
//! Ignored (network). cargo test --lib ccxt_spike -- --ignored --nocapture

#[test]
#[ignore = "network"]
fn ccxt_spike_bybit_public() {
    tauri::async_runtime::block_on(async {
        let mut ex = ccxt::Bybit::new(None);
        ex.load_markets(false).await;
        let t = ex.fetch_ticker("BTC/USDT:USDT", ccxt::Params::none()).await.expect("ticker");
        println!("bybit BTC/USDT:USDT last {:?} symbol {}", t.last, t.symbol);
        assert!(t.last.unwrap_or_default() > 0.0);
    });
}
