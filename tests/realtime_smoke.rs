//! WS 冒烟测试(ADR-0009):真实连接公开流验证。
//!
//! 需要网络,默认 `#[ignore]`;本地验证用:
//! `cargo test --features realtime --test realtime_smoke -- --ignored --nocapture`
//!
//! CI 不跑这些用例(ADR-0010:差分/回放优先,避免 flaky)。

#![cfg(feature = "realtime")]

use adaq_trading_crypto::Config;
use adaq_trading_crypto::error::ErrorKind;
use adaq_trading_crypto::exchange::Realtime;
use adaq_trading_crypto::realtime::BinanceWs;

#[tokio::test]
#[ignore = "needs network"]
async fn binance_watch_ticker_smoke() {
    let ws = BinanceWs::new(Config::new()).expect("adapter");
    let t = ws
        .watch_ticker("BTC/USDT", adaq_trading_crypto::Params::new())
        .await
        .expect("watch_ticker");
    // WS 消息的 s 字段是交易所格式(BTCUSDT);仅断言数据可用
    assert!(t.last.is_some(), "ticker 应有 last 价格");
    println!("ticker: {:?}", t.last);
}

#[tokio::test]
#[ignore = "needs network"]
async fn binance_watch_order_book_smoke() {
    let ws = BinanceWs::new(Config::new()).expect("adapter");
    let book = ws
        .watch_order_book("BTC/USDT", None, adaq_trading_crypto::Params::new())
        .await
        .expect("watch_order_book");
    assert!(!book.bids.is_empty(), "bids 非空");
    assert!(!book.asks.is_empty(), "asks 非空");
    // 有序性:bids 降序、asks 升序
    assert!(
        book.bids.windows(2).all(|w| w[0].price >= w[1].price),
        "bids 降序"
    );
    assert!(
        book.asks.windows(2).all(|w| w[0].price <= w[1].price),
        "asks 升序"
    );
    println!("book: bids={} asks={}", book.bids.len(), book.asks.len());
}

#[tokio::test]
#[ignore = "needs network"]
async fn binance_watch_trades_smoke() {
    let ws = BinanceWs::new(Config::new()).expect("adapter");
    let trades = ws
        .watch_trades("BTC/USDT", None, None, adaq_trading_crypto::Params::new())
        .await
        .expect("watch_trades");
    assert!(!trades.is_empty(), "有成交");
    assert!(trades[0].price.is_some(), "成交价存在");
    println!("trade: {:?} @ {:?}", trades[0].amount, trades[0].price);
}

#[tokio::test]
#[ignore = "needs network"]
async fn binance_unsupported_positions_is_not_supported() {
    let ws = BinanceWs::new(Config::new()).expect("adapter");
    let err = ws
        .watch_positions(None, adaq_trading_crypto::Params::new())
        .await
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotSupported);
}
