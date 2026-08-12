//! fixtures 回放形状测试(ADR-0010):把真实 ccxt 录制的统一输出
//! 反序列化进我们的统一结构,验证类型契约与 ccxt 输出形状兼容。

mod common;

use adaq_trading_crypto::{Market, OrderBook, Ticker, Trade};
use common::{load_recorded, normalized, parse_level, parse_ohlcv_row};

const EXCHANGE: &str = "binance";
const SYMBOL: &str = "BTC/USDT";

#[test]
fn ticker_shape_matches_ccxt() {
    let fixture = load_recorded(EXCHANGE, "ticker");
    let ticker: Ticker = serde_json::from_value(normalized(&fixture).clone())
        .expect("ticker 反序列化失败(形状与 ccxt 输出不兼容)");
    assert_eq!(ticker.symbol, SYMBOL);
    assert!(ticker.last.is_some(), "last 应为 Some");
    assert!(ticker.timestamp.is_some(), "timestamp 应为 Some");
    assert!(ticker.info.is_object(), "info 应保留原始对象");
    eprintln!("ticker.last = {:?}", ticker.last);
}

#[test]
fn markets_shape_matches_ccxt() {
    let fixture = load_recorded(EXCHANGE, "markets");
    let markets: Vec<Market> =
        serde_json::from_value(normalized(&fixture).clone()).expect("markets 反序列化失败");
    assert!(!markets.is_empty(), "markets 非空");
    let btc = markets
        .iter()
        .find(|m| m.symbol == SYMBOL)
        .expect("markets 中应包含 BTC/USDT");
    assert_eq!(btc.base.as_deref(), Some("BTC"));
    assert_eq!(btc.quote.as_deref(), Some("USDT"));
    assert!(
        btc.id.contains("BTCUSDT"),
        "id 应含 BTCUSDT,实际 {}",
        btc.id
    );
}

#[test]
fn order_book_shape_matches_ccxt() {
    let fixture = load_recorded(EXCHANGE, "order_book");
    let value = normalized(&fixture);
    let bids: Vec<_> = value["bids"]
        .as_array()
        .unwrap()
        .iter()
        .map(parse_level)
        .collect();
    let asks: Vec<_> = value["asks"]
        .as_array()
        .unwrap()
        .iter()
        .map(parse_level)
        .collect();
    assert!(!bids.is_empty() && !asks.is_empty(), "bids/asks 非空");
    assert!(bids.iter().all(|l| l.price.is_some() && l.amount.is_some()));
    // 订单簿有序性:买盘价降序、卖盘价升序(ccxt 惯例)
    for w in bids.windows(2) {
        assert!(w[0].price >= w[1].price, "bids 应为降序");
    }
    for w in asks.windows(2) {
        assert!(w[0].price <= w[1].price, "asks 应为升序");
    }
    // 整表也直接反序列化一次,验证 OrderBook 结构字段齐全
    let book: OrderBook = serde_json::from_value(value.clone()).expect("OrderBook 反序列化失败");
    assert_eq!(book.symbol, SYMBOL);
}

#[test]
fn ohlcv_shape_matches_ccxt() {
    let fixture = load_recorded(EXCHANGE, "ohlcv");
    let rows = normalized(&fixture).as_array().expect("ohlcv 为数组");
    assert_eq!(rows.len(), 5, "录制了 5 根 K 线");
    let candles: Vec<_> = rows.iter().map(parse_ohlcv_row).collect();
    for c in &candles {
        assert!(c.timestamp.is_some(), "timestamp 应为 Some");
        assert!(
            c.open.is_some() && c.close.is_some(),
            "open/close 应为 Some"
        );
    }
    // 时间戳严格递增
    let ts: Vec<i64> = candles.iter().filter_map(|c| c.timestamp).collect();
    assert!(ts.windows(2).all(|w| w[0] < w[1]), "K 线时间戳应递增");
}

#[test]
fn trades_shape_matches_ccxt() {
    let fixture = load_recorded(EXCHANGE, "trades");
    let trades: Vec<Trade> =
        serde_json::from_value(normalized(&fixture).clone()).expect("trades 反序列化失败");
    assert!(!trades.is_empty(), "trades 非空");
    for t in &trades {
        assert!(
            t.price.is_some() && t.amount.is_some(),
            "price/amount 应为 Some"
        );
        assert!(t.side.is_some(), "side 应为 Some");
    }
}
