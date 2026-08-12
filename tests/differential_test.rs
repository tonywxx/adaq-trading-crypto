//! 差分值比对测试(ADR-0010):【同一份原始响应】经我们的解析器与真实 ccxt
//! 的解析器各自解析,逐字段比对。fixtures 由 scripts/differential/* 录制,
//! 离线运行,CI 确定性。
//!
//! 需要交易所 feature(binance / kalshi / polymarket,默认开启)。

#![cfg(any(feature = "binance", feature = "kalshi", feature = "polymarket"))]

mod common;

use std::str::FromStr;

use rust_decimal::Decimal;
use serde_json::Value;

use adaq_trading_crypto::adapters::Binance;
use adaq_trading_crypto::{Config, Level};

use common::{load_ccxt_parsed, load_raw, parsed};

const EXCHANGE: &str = "binance";

fn setup() -> Binance {
    let mut config = Config::new();
    config.enable_rate_limit = false;
    Binance::new(config).expect("binance adapter")
}

/// Value → Option<Decimal>(兼容数字/字符串)。
fn dec(v: &Value) -> Option<Decimal> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        _ => None,
    }
}

/// 断言我们的数值与 ccxt 数值一致。
///
/// ccxt(Python float)在大数上会损失精度(约 15-17 位有效数字),
/// 我们的 Decimal 完全保留原始字符串;故采用相对容差比对。
fn assert_decimal_eq(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
    match (ours, theirs) {
        (Some(a), Some(b)) => {
            let diff = (a - b).abs();
            let scale = a.abs().max(b.abs());
            let within_tol =
                diff.is_zero() || (scale > Decimal::ZERO && diff / scale <= Decimal::new(1, 12));
            assert!(
                within_tol,
                "{label}: ours={a} ccxt={b} (超出相对容差 1e-12)"
            );
        }
        (None, None) => {}
        (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
    }
}

fn obj_dec(v: &Value, key: &str) -> Option<Decimal> {
    v.get(key).and_then(dec)
}

fn obj_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

#[test]
fn ticker_matches_ccxt() {
    let binance = setup();
    let raw = &load_raw(EXCHANGE, "ticker")["raw"];
    let ccxt_fixture = load_ccxt_parsed(EXCHANGE, "ticker");
    let ccxt = parsed(&ccxt_fixture);

    let ours = binance.parse_ticker(raw);

    assert_eq!(ours.symbol, obj_str(ccxt, "symbol").unwrap());
    assert_eq!(ours.timestamp, ccxt["timestamp"].as_i64());
    assert_eq!(ours.datetime, obj_str(ccxt, "datetime"));
    for key in [
        "high",
        "low",
        "bid",
        "ask",
        "bid_volume",
        "ask_volume",
        "vwap",
        "open",
        "close",
        "last",
        "previous_close",
        "change",
        "percentage",
        "average",
        "base_volume",
        "quote_volume",
    ] {
        let our_field = match key {
            "bid_volume" => ours.bid_volume,
            "ask_volume" => ours.ask_volume,
            "base_volume" => ours.base_volume,
            "quote_volume" => ours.quote_volume,
            "previous_close" => ours.previous_close,
            other => match other {
                "high" => ours.high,
                "low" => ours.low,
                "bid" => ours.bid,
                "ask" => ours.ask,
                "vwap" => ours.vwap,
                "open" => ours.open,
                "close" => ours.close,
                "last" => ours.last,
                "change" => ours.change,
                "percentage" => ours.percentage,
                "average" => ours.average,
                _ => unreachable!(),
            },
        };
        assert_decimal_eq(&format!("ticker.{key}"), our_field, obj_dec(ccxt, key));
    }
}

#[test]
fn ohlcv_matches_ccxt() {
    let binance = setup();
    let raw_rows = load_raw(EXCHANGE, "ohlcv")["raw"]
        .as_array()
        .unwrap()
        .clone();
    let ccxt_rows = parsed(&load_ccxt_parsed(EXCHANGE, "ohlcv"))
        .as_array()
        .unwrap()
        .clone();

    assert_eq!(raw_rows.len(), ccxt_rows.len(), "行数一致");
    for (i, (raw, ccxt_row)) in raw_rows.iter().zip(ccxt_rows.iter()).enumerate() {
        // ccxt 的 ohlcv 是 [ts, o, h, l, c, v] 数组
        let ccxt_arr = ccxt_row.as_array().expect("ccxt ohlcv row is array");
        let ours = binance.parse_ohlcv(raw);
        assert_eq!(
            ours.timestamp,
            ccxt_arr.first().and_then(common::value_i64),
            "row {i} ts"
        );
        for (idx, key) in ["open", "high", "low", "close", "volume"]
            .iter()
            .enumerate()
        {
            let our_field = match *key {
                "open" => ours.open,
                "high" => ours.high,
                "low" => ours.low,
                "close" => ours.close,
                _ => ours.volume,
            };
            assert_decimal_eq(
                &format!("ohlcv[{i}].{key}"),
                our_field,
                ccxt_arr.get(idx + 1).and_then(dec),
            );
        }
    }
}

#[test]
fn order_book_matches_ccxt() {
    let binance = setup();
    let raw = &load_raw(EXCHANGE, "order_book")["raw"];
    let ccxt_fixture = load_ccxt_parsed(EXCHANGE, "order_book");
    let ccxt = parsed(&ccxt_fixture);

    let ours = binance.parse_order_book(raw, "BTCUSDT");
    assert_eq!(ours.symbol, obj_str(ccxt, "symbol").unwrap());

    let cmp_levels = |label: &str, ours: &[Level], theirs: &Value| {
        let theirs = theirs.as_array().unwrap();
        assert_eq!(ours.len(), theirs.len(), "{label} 档数一致");
        for (i, (l, t)) in ours.iter().zip(theirs.iter()).enumerate() {
            assert_decimal_eq(
                &format!("{label}[{i}].price"),
                l.price,
                t.as_array().unwrap().first().and_then(dec),
            );
            assert_decimal_eq(
                &format!("{label}[{i}].amount"),
                l.amount,
                t.as_array().unwrap().get(1).and_then(dec),
            );
        }
    };
    cmp_levels("bids", &ours.bids, ccxt.get("bids").unwrap());
    cmp_levels("asks", &ours.asks, ccxt.get("asks").unwrap());
}

#[test]
fn trades_match_ccxt() {
    let binance = setup();
    let raw_trades = load_raw(EXCHANGE, "trades")["raw"]
        .as_array()
        .unwrap()
        .clone();
    let ccxt_trades = parsed(&load_ccxt_parsed(EXCHANGE, "trades"))
        .as_array()
        .unwrap()
        .clone();

    assert_eq!(raw_trades.len(), ccxt_trades.len());
    for (i, (raw, ccxt_t)) in raw_trades.iter().zip(ccxt_trades.iter()).enumerate() {
        let ours = binance.parse_trade(raw);
        assert_eq!(ours.id, obj_str(ccxt_t, "id"), "trade {i} id");
        assert_eq!(ours.timestamp, ccxt_t["timestamp"].as_i64(), "trade {i} ts");
        assert_eq!(ours.side, obj_str(ccxt_t, "side"), "trade {i} side");
        assert_decimal_eq(
            &format!("trade[{i}].price"),
            ours.price,
            obj_dec(ccxt_t, "price"),
        );
        assert_decimal_eq(
            &format!("trade[{i}].amount"),
            ours.amount,
            obj_dec(ccxt_t, "amount"),
        );
        assert_decimal_eq(
            &format!("trade[{i}].cost"),
            ours.cost,
            obj_dec(ccxt_t, "cost"),
        );
    }
}

// ================= kalshi 预测市场差分 =================

#[cfg(feature = "kalshi")]
mod kalshi_diff {
    use super::*;
    use adaq_trading_crypto::adapters::kalshi::{Kalshi, OutcomeCtx};

    fn setup() -> Kalshi {
        let mut config = Config::new();
        config.enable_rate_limit = false;
        Kalshi::new(config).expect("kalshi adapter")
    }

    /// 从 raw market + fixture outcome 构造解析上下文。
    fn ctx_for(ex: &Kalshi, raw_market: &Value, outcome: &str) -> OutcomeCtx {
        let ticker = raw_market["ticker"].as_str().unwrap_or_default();
        let event_ticker = raw_market["event_ticker"].as_str().unwrap_or_default();
        let series = event_ticker
            .rsplit_once('-')
            .map(|(h, _)| h.to_string())
            .unwrap_or_else(|| event_ticker.to_string());
        let m = ex.parse_market(raw_market);
        let label = outcome.rsplit(':').next().unwrap_or("YES").to_string();
        OutcomeCtx {
            market_ticker: ticker.to_string(),
            label,
            market_symbol: m.symbol,
            outcome_id: ticker.to_string(),
            series_ticker: series,
        }
    }

    fn obj_str(v: &Value, key: &str) -> Option<String> {
        v.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn obj_dec(v: &Value, key: &str) -> Option<Decimal> {
        v.get(key).and_then(super::dec)
    }

    fn assert_dec(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                assert_eq!(a.normalize(), b.normalize(), "{label}: ours={a} ccxt={b}")
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    #[test]
    fn markets_match_ccxt() {
        let ex = setup();
        let raw = load_raw("kalshi", "markets");
        let ccxt_fixture = load_ccxt_parsed("kalshi", "markets");
        let ccxt = parsed(&ccxt_fixture);
        let raw_arr = raw["raw"].as_array().unwrap();
        let ccxt_arr = ccxt.as_array().unwrap();
        assert_eq!(raw_arr.len(), ccxt_arr.len());
        for (i, (r, c)) in raw_arr.iter().zip(ccxt_arr.iter()).enumerate() {
            let m = ex.parse_market(r);
            assert_eq!(m.id, c["id"].as_str().unwrap(), "market[{i}].id");
            assert_eq!(
                m.symbol,
                obj_str(c, "market").unwrap_or_default(),
                "market[{i}].symbol"
            );
            assert_eq!(
                m.base.as_deref(),
                obj_str(c, "base").as_deref(),
                "market[{i}].base"
            );
            assert_eq!(
                m.quote.as_deref(),
                obj_str(c, "quote").as_deref(),
                "market[{i}].quote"
            );
            assert_eq!(m.active, c["active"].as_bool(), "market[{i}].active");
            assert_dec(
                &format!("market[{i}].precision.price"),
                m.precision.price,
                obj_dec(c.get("precision").unwrap(), "price"),
            );
        }
    }

    #[test]
    fn ticker_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kalshi", "ticker");
        let raw = raw_fixture["raw"].clone();
        let outcome = raw_fixture["outcome"].as_str().unwrap();
        let ctx = ctx_for(&ex, &raw, outcome);
        let ccxt_fixture = load_ccxt_parsed("kalshi", "ticker");
        let ccxt = parsed(&ccxt_fixture);
        let t = ex.parse_ticker(&raw, &ctx);
        assert_eq!(
            t.symbol,
            obj_str(ccxt, "outcome").unwrap_or_default(),
            "ticker.symbol(== outcome)"
        );
        for key in ["bid", "ask", "close", "last", "average", "base_volume"] {
            let our = match key {
                "bid" => t.bid,
                "ask" => t.ask,
                "close" => t.close,
                "last" => t.last,
                "average" => t.average,
                _ => t.base_volume,
            };
            assert_dec(&format!("ticker.{key}"), our, obj_dec(ccxt, key));
        }
        assert_dec(
            "ticker.bid_volume",
            t.bid_volume,
            obj_dec(ccxt, "bid_volume"),
        );
        assert_dec(
            "ticker.ask_volume",
            t.ask_volume,
            obj_dec(ccxt, "ask_volume"),
        );
    }

    #[test]
    fn order_book_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kalshi", "order_book");
        let raw = raw_fixture["raw"].clone();
        let outcome = raw_fixture["outcome"].as_str().unwrap();
        // ctx 需要 market 对象推导 symbol:用 raw/ticker 的 market
        let raw_market = load_raw("kalshi", "ticker")["raw"].clone();
        let ctx = ctx_for(&ex, &raw_market, outcome);
        let ccxt_fixture = load_ccxt_parsed("kalshi", "order_book");
        let ccxt = parsed(&ccxt_fixture);
        let book = ex.parse_order_book(&raw, &ctx);
        assert_eq!(
            book.bids.len(),
            ccxt["bids"].as_array().unwrap().len(),
            "bids 数量"
        );
        assert_eq!(
            book.asks.len(),
            ccxt["asks"].as_array().unwrap().len(),
            "asks 数量"
        );
        for (i, (o, c)) in book
            .bids
            .iter()
            .zip(ccxt["bids"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec(
                &format!("bids[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec(
                &format!("bids[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
        for (i, (o, c)) in book
            .asks
            .iter()
            .zip(ccxt["asks"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec(
                &format!("asks[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec(
                &format!("asks[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
    }

    #[test]
    fn trades_match_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kalshi", "trades");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let outcome = raw_fixture["outcome"].as_str().unwrap();
        let raw_market = load_raw("kalshi", "ticker")["raw"].clone();
        let ctx = ctx_for(&ex, &raw_market, outcome);
        let ccxt = parsed(&load_ccxt_parsed("kalshi", "trades"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let t = ex.parse_trade(r, &ctx);
            assert_eq!(t.id.as_deref(), c["id"].as_str(), "trade[{i}].id");
            assert_eq!(
                t.symbol.as_deref(),
                obj_str(c, "market").as_deref(),
                "trade[{i}].market"
            );
            assert_eq!(
                t.side.as_deref(),
                obj_str(c, "side").as_deref(),
                "trade[{i}].side"
            );
            assert_dec(&format!("trade[{i}].price"), t.price, obj_dec(c, "price"));
            assert_dec(
                &format!("trade[{i}].amount"),
                t.amount,
                obj_dec(c, "amount"),
            );
            assert_dec(&format!("trade[{i}].cost"), t.cost, obj_dec(c, "cost"));
        }
    }

    #[test]
    fn ohlcv_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kalshi", "ohlcv");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("kalshi", "ohlcv"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let o = ex.parse_ohlcv(r, 60);
            assert_eq!(o.timestamp, c[0].as_i64(), "ohlcv[{i}].ts");
            for (idx, key) in ["open", "high", "low", "close", "volume"]
                .iter()
                .enumerate()
            {
                let our = match *key {
                    "open" => o.open,
                    "high" => o.high,
                    "low" => o.low,
                    "close" => o.close,
                    _ => o.volume,
                };
                assert_dec(
                    &format!("ohlcv[{i}].{key}"),
                    our,
                    common::value_decimal(&c[idx + 1]),
                );
            }
        }
    }

    #[test]
    fn slug_helpers_match_ccxt_fixture() {
        // markets fixture 的 symbol 必须由 shorten_slug 规则生成(与 ccxt 一致)
        let raw = load_raw("kalshi", "markets");
        let ex = setup();
        for r in raw["raw"].as_array().unwrap() {
            let m = ex.parse_market(r);
            assert!(!m.symbol.is_empty(), "symbol 非空");
            assert!(
                m.symbol
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "symbol 应为 UPPER_SNAKE: {}",
                m.symbol
            );
        }
    }
}

// ================= polymarket 预测市场差分 =================

#[cfg(feature = "polymarket")]
mod polymarket_diff {
    use super::*;
    use adaq_trading_crypto::adapters::polymarket::{OutcomeCtx, Polymarket};

    fn setup() -> Polymarket {
        let mut config = Config::new();
        config.enable_rate_limit = false;
        Polymarket::new(config).expect("polymarket adapter")
    }

    fn obj_str(v: &Value, key: &str) -> Option<String> {
        v.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn obj_dec(v: &Value, key: &str) -> Option<Decimal> {
        v.get(key).and_then(super::dec)
    }

    fn assert_dec(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                assert_eq!(a.normalize(), b.normalize(), "{label}: ours={a} ccxt={b}")
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    /// 相对容差 1e-12(ccxt f64 精度损失场景)。
    fn assert_dec_rel(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                let diff = (a - b).abs();
                let scale = a.abs().max(b.abs());
                let within = diff.is_zero()
                    || (scale > Decimal::ZERO && diff / scale <= Decimal::new(1, 12));
                assert!(within, "{label}: ours={a} ccxt={b}");
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    #[test]
    fn markets_match_ccxt() {
        let ex = setup();
        let raw = load_raw("polymarket", "markets");
        let ccxt_fixture = load_ccxt_parsed("polymarket", "markets");
        let ccxt = parsed(&ccxt_fixture);
        let events = raw["raw"].as_array().unwrap().clone();
        let ccxt_arr = ccxt.as_array().unwrap();
        let mut parsed_flat = Vec::new();
        for ev in &events {
            let slug = ev["slug"]
                .as_str()
                .or_else(|| ev["id"].as_str())
                .unwrap_or_default();
            for m in ev["markets"].as_array().unwrap_or(&Vec::new()) {
                parsed_flat.push(ex.parse_market_from_event(m, slug));
            }
        }
        assert_eq!(parsed_flat.len(), ccxt_arr.len(), "market 数量一致");
        for (i, (m, c)) in parsed_flat.iter().zip(ccxt_arr.iter()).enumerate() {
            assert_eq!(m.id, c["id"].as_str().unwrap_or_default(), "market[{i}].id");
            assert_eq!(
                m.symbol,
                obj_str(c, "market").unwrap_or_default(),
                "market[{i}].symbol"
            );
            assert_eq!(
                m.base.as_deref(),
                obj_str(c, "base").as_deref(),
                "market[{i}].base"
            );
            assert_eq!(
                m.quote.as_deref(),
                obj_str(c, "quote").as_deref(),
                "market[{i}].quote"
            );
            assert_eq!(m.active, c["active"].as_bool(), "market[{i}].active");
        }
    }

    #[test]
    fn ticker_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("polymarket", "ticker");
        let raw = raw_fixture["raw"].clone();
        let _outcome = raw_fixture["outcome"].as_str().unwrap();
        let ccxt_fixture = load_ccxt_parsed("polymarket", "ticker");
        let ccxt = parsed(&ccxt_fixture);
        // market_symbol = outcome 去 :LABEL 后缀(即 ccxt ticker 的 market 键)
        let market_symbol = obj_str(ccxt, "market").unwrap_or_default();
        let ctx = OutcomeCtx {
            token_id: obj_str(ccxt, "outcome_id").unwrap_or_default(),
            condition_id: String::new(),
            market_symbol,
            label: obj_str(ccxt, "label").unwrap_or_else(|| "YES".into()),
            quote_volume: None,
        };
        let t = ex.parse_ticker(&raw, &ctx);
        assert_eq!(
            t.symbol,
            obj_str(ccxt, "outcome").unwrap_or_default(),
            "ticker.symbol(== outcome)"
        );
        for key in ["bid", "ask", "close", "last", "average"] {
            let our = match key {
                "bid" => t.bid,
                "ask" => t.ask,
                "close" => t.close,
                "last" => t.last,
                _ => t.average,
            };
            assert_dec(&format!("ticker.{key}"), our, obj_dec(ccxt, key));
        }
        assert_dec(
            "ticker.bid_volume",
            t.bid_volume,
            obj_dec(ccxt, "bid_volume"),
        );
        assert_dec(
            "ticker.ask_volume",
            t.ask_volume,
            obj_dec(ccxt, "ask_volume"),
        );
    }

    #[test]
    fn order_book_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("polymarket", "order_book");
        let raw = raw_fixture["raw"].clone();
        let outcome = raw_fixture["outcome"].as_str().unwrap();
        let ccxt_fixture = load_ccxt_parsed("polymarket", "order_book");
        let ccxt = parsed(&ccxt_fixture);
        let book = ex.parse_order_book(&raw, outcome);
        assert_eq!(
            book.bids.len(),
            ccxt["bids"].as_array().unwrap().len(),
            "bids 数量"
        );
        assert_eq!(
            book.asks.len(),
            ccxt["asks"].as_array().unwrap().len(),
            "asks 数量"
        );
        for (i, (o, c)) in book
            .bids
            .iter()
            .zip(ccxt["bids"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec(
                &format!("bids[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec(
                &format!("bids[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
        for (i, (o, c)) in book
            .asks
            .iter()
            .zip(ccxt["asks"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec(
                &format!("asks[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec(
                &format!("asks[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
    }

    #[test]
    fn trades_match_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("polymarket", "trades");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("polymarket", "trades"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let t = ex.parse_trade(r, "", "");
            assert_eq!(
                t.side.as_deref(),
                obj_str(c, "side").as_deref(),
                "trade[{i}].side"
            );
            assert_dec(&format!("trade[{i}].price"), t.price, obj_dec(c, "price"));
            assert_dec(
                &format!("trade[{i}].amount"),
                t.amount,
                obj_dec(c, "amount"),
            );
            assert_dec_rel(&format!("trade[{i}].cost"), t.cost, obj_dec(c, "cost"));
        }
    }
}

// ================= okx 差分 =================

#[cfg(feature = "okx")]
mod okx_diff {
    use super::*;
    use adaq_trading_crypto::adapters::Okx;

    fn setup() -> Okx {
        let mut config = Config::new();
        config.enable_rate_limit = false;
        Okx::new(config).expect("okx adapter")
    }

    fn obj_str(v: &Value, key: &str) -> Option<String> {
        v.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn obj_dec(v: &Value, key: &str) -> Option<Decimal> {
        v.get(key).and_then(super::dec)
    }

    fn assert_dec(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                assert_eq!(a.normalize(), b.normalize(), "{label}: ours={a} ccxt={b}")
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    #[test]
    fn markets_match_ccxt() {
        let ex = setup();
        let raw = load_raw("okx", "markets");
        let ccxt_fixture = load_ccxt_parsed("okx", "markets");
        let ccxt = parsed(&ccxt_fixture);
        let raw_arr = raw["raw"].as_array().unwrap();
        let ccxt_arr = ccxt.as_array().unwrap();
        assert_eq!(raw_arr.len(), ccxt_arr.len());
        for (i, (r, c)) in raw_arr.iter().zip(ccxt_arr.iter()).enumerate() {
            let m = ex.parse_market(r);
            assert_eq!(m.id, c["id"].as_str().unwrap_or_default(), "market[{i}].id");
            assert_eq!(
                m.symbol,
                c["symbol"].as_str().unwrap_or_default(),
                "market[{i}].symbol"
            );
            assert_eq!(
                m.base.as_deref(),
                obj_str(c, "base").as_deref(),
                "market[{i}].base"
            );
            assert_eq!(
                m.quote.as_deref(),
                obj_str(c, "quote").as_deref(),
                "market[{i}].quote"
            );
            assert_eq!(m.active, c["active"].as_bool(), "market[{i}].active");
        }
    }

    #[test]
    fn ticker_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("okx", "ticker");
        let raw = raw_fixture["raw"].clone();
        let ccxt_fixture = load_ccxt_parsed("okx", "ticker");
        let ccxt = parsed(&ccxt_fixture);
        let t = ex.parse_ticker(&raw);
        assert_eq!(
            t.symbol,
            obj_str(ccxt, "symbol").unwrap_or_default(),
            "ticker.symbol"
        );
        for key in [
            "last",
            "bid",
            "ask",
            "open",
            "high",
            "low",
            "base_volume",
            "quote_volume",
        ] {
            let our = match key {
                "last" => t.last,
                "bid" => t.bid,
                "ask" => t.ask,
                "open" => t.open,
                "high" => t.high,
                "low" => t.low,
                "base_volume" => t.base_volume,
                _ => t.quote_volume,
            };
            // vol 类大数有 ccxt f64 精度损失 → 相对容差
            assert_dec_rel(&format!("ticker.{key}"), our, obj_dec(ccxt, key));
        }
    }

    /// 相对容差 1e-12(ccxt f64 精度损失场景)。
    fn assert_dec_rel(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                let diff = (a - b).abs();
                let scale = a.abs().max(b.abs());
                let within = diff.is_zero()
                    || (scale > Decimal::ZERO && diff / scale <= Decimal::new(1, 12));
                assert!(within, "{label}: ours={a} ccxt={b}");
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    #[test]
    fn order_book_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("okx", "order_book");
        let raw = raw_fixture["raw"].clone();
        let ccxt_fixture = load_ccxt_parsed("okx", "order_book");
        let ccxt = parsed(&ccxt_fixture);
        let book = ex.parse_order_book(&raw, "BTC/USDT");
        assert_eq!(
            book.bids.len(),
            ccxt["bids"].as_array().unwrap().len(),
            "bids 数量"
        );
        assert_eq!(
            book.asks.len(),
            ccxt["asks"].as_array().unwrap().len(),
            "asks 数量"
        );
        for (i, (o, c)) in book
            .bids
            .iter()
            .zip(ccxt["bids"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec(
                &format!("bids[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec(
                &format!("bids[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
        for (i, (o, c)) in book
            .asks
            .iter()
            .zip(ccxt["asks"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec(
                &format!("asks[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec(
                &format!("asks[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
    }

    #[test]
    fn trades_match_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("okx", "trades");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("okx", "trades"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let t = ex.parse_trade(r);
            assert_eq!(t.id.as_deref(), c["id"].as_str(), "trade[{i}].id");
            assert_eq!(
                t.side.as_deref(),
                obj_str(c, "side").as_deref(),
                "trade[{i}].side"
            );
            assert_dec(&format!("trade[{i}].price"), t.price, obj_dec(c, "price"));
            assert_dec(
                &format!("trade[{i}].amount"),
                t.amount,
                obj_dec(c, "amount"),
            );
        }
    }

    #[test]
    fn ohlcv_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("okx", "ohlcv");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("okx", "ohlcv"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let o = ex.parse_ohlcv(r);
            assert_eq!(o.timestamp, c[0].as_i64(), "ohlcv[{i}].ts");
            for (idx, key) in ["open", "high", "low", "close", "volume"]
                .iter()
                .enumerate()
            {
                let our = match *key {
                    "open" => o.open,
                    "high" => o.high,
                    "low" => o.low,
                    "close" => o.close,
                    _ => o.volume,
                };
                assert_dec(
                    &format!("ohlcv[{i}].{key}"),
                    our,
                    common::value_decimal(&c[idx + 1]),
                );
            }
        }
    }
}

// ================= bybit 差分 =================

#[cfg(feature = "bybit")]
mod bybit_diff {
    use super::*;
    use adaq_trading_crypto::adapters::Bybit;

    fn setup() -> Bybit {
        let mut config = Config::new();
        config.enable_rate_limit = false;
        Bybit::new(config).expect("bybit adapter")
    }

    fn obj_str(v: &Value, key: &str) -> Option<String> {
        v.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn obj_dec(v: &Value, key: &str) -> Option<Decimal> {
        v.get(key).and_then(super::dec)
    }

    fn assert_dec_rel(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                let diff = (a - b).abs();
                let scale = a.abs().max(b.abs());
                let within = diff.is_zero()
                    || (scale > Decimal::ZERO && diff / scale <= Decimal::new(1, 12));
                assert!(within, "{label}: ours={a} ccxt={b}");
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    #[test]
    fn ticker_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("bybit", "ticker");
        let raw = raw_fixture["raw"].clone();
        let ccxt_fixture = load_ccxt_parsed("bybit", "ticker");
        let ccxt = parsed(&ccxt_fixture);
        let t = ex.parse_ticker(&raw);
        assert_eq!(
            t.symbol,
            obj_str(ccxt, "symbol").unwrap_or_default(),
            "ticker.symbol"
        );
        for key in [
            "last",
            "bid",
            "ask",
            "open",
            "high",
            "low",
            "base_volume",
            "quote_volume",
        ] {
            let our = match key {
                "last" => t.last,
                "bid" => t.bid,
                "ask" => t.ask,
                "open" => t.open,
                "high" => t.high,
                "low" => t.low,
                "base_volume" => t.base_volume,
                _ => t.quote_volume,
            };
            assert_dec_rel(&format!("ticker.{key}"), our, obj_dec(ccxt, key));
        }
    }

    #[test]
    fn order_book_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("bybit", "order_book");
        let raw = raw_fixture["raw"].clone();
        let ccxt_fixture = load_ccxt_parsed("bybit", "order_book");
        let ccxt = parsed(&ccxt_fixture);
        let book = ex.parse_order_book(&raw, "BTC/USDT");
        assert_eq!(
            book.bids.len(),
            ccxt["bids"].as_array().unwrap().len(),
            "bids 数量"
        );
        assert_eq!(
            book.asks.len(),
            ccxt["asks"].as_array().unwrap().len(),
            "asks 数量"
        );
        for (i, (o, c)) in book
            .bids
            .iter()
            .zip(ccxt["bids"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec_rel(
                &format!("bids[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec_rel(
                &format!("bids[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
        for (i, (o, c)) in book
            .asks
            .iter()
            .zip(ccxt["asks"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec_rel(
                &format!("asks[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec_rel(
                &format!("asks[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
    }

    #[test]
    fn trades_match_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("bybit", "trades");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("bybit", "trades"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let t = ex.parse_trade(r);
            assert_eq!(t.id.as_deref(), c["id"].as_str(), "trade[{i}].id");
            assert_eq!(
                t.side.as_deref(),
                obj_str(c, "side").as_deref(),
                "trade[{i}].side"
            );
            assert_dec_rel(&format!("trade[{i}].price"), t.price, obj_dec(c, "price"));
            assert_dec_rel(
                &format!("trade[{i}].amount"),
                t.amount,
                obj_dec(c, "amount"),
            );
        }
    }

    #[test]
    fn ohlcv_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("bybit", "ohlcv");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("bybit", "ohlcv"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let o = ex.parse_ohlcv(r);
            assert_eq!(o.timestamp, c[0].as_i64(), "ohlcv[{i}].ts");
            for (idx, key) in ["open", "high", "low", "close", "volume"]
                .iter()
                .enumerate()
            {
                let our = match *key {
                    "open" => o.open,
                    "high" => o.high,
                    "low" => o.low,
                    "close" => o.close,
                    _ => o.volume,
                };
                assert_dec_rel(
                    &format!("ohlcv[{i}].{key}"),
                    our,
                    common::value_decimal(&c[idx + 1]),
                );
            }
        }
    }
}

// ================= kraken 差分 =================

#[cfg(feature = "kraken")]
mod kraken_diff {
    use super::*;
    use adaq_trading_crypto::adapters::Kraken;

    fn setup() -> Kraken {
        let mut config = Config::new();
        config.enable_rate_limit = false;
        Kraken::new(config).expect("kraken adapter")
    }

    fn obj_str(v: &Value, key: &str) -> Option<String> {
        v.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn obj_dec(v: &Value, key: &str) -> Option<Decimal> {
        v.get(key).and_then(super::dec)
    }

    fn assert_dec_rel(label: &str, ours: Option<Decimal>, theirs: Option<Decimal>) {
        match (ours, theirs) {
            (Some(a), Some(b)) => {
                let diff = (a - b).abs();
                let scale = a.abs().max(b.abs());
                let within = diff.is_zero()
                    || (scale > Decimal::ZERO && diff / scale <= Decimal::new(1, 12));
                assert!(within, "{label}: ours={a} ccxt={b}");
            }
            (None, None) => {}
            (a, b) => panic!("{label}: ours={a:?} ccxt={b:?}"),
        }
    }

    #[test]
    fn ticker_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kraken", "ticker");
        let raw = raw_fixture["raw"].clone();
        let ccxt_fixture = load_ccxt_parsed("kraken", "ticker");
        let ccxt = parsed(&ccxt_fixture);
        let t = ex.parse_ticker(&raw, "XXBTZUSD");
        // symbol 依赖 markets 缓存(离线测试不比对);数据字段逐项比对
        for key in [
            "last",
            "bid",
            "ask",
            "open",
            "high",
            "low",
            "base_volume",
            "quote_volume",
            "vwap",
        ] {
            let our = match key {
                "last" => t.last,
                "bid" => t.bid,
                "ask" => t.ask,
                "open" => t.open,
                "high" => t.high,
                "low" => t.low,
                "base_volume" => t.base_volume,
                "quote_volume" => t.quote_volume,
                _ => t.vwap,
            };
            assert_dec_rel(&format!("ticker.{key}"), our, obj_dec(ccxt, key));
        }
    }

    #[test]
    fn order_book_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kraken", "order_book");
        let raw = raw_fixture["raw"].clone();
        let ccxt_fixture = load_ccxt_parsed("kraken", "order_book");
        let ccxt = parsed(&ccxt_fixture);
        let book = ex.parse_order_book(&raw, "BTC/USD");
        assert_eq!(
            book.bids.len(),
            ccxt["bids"].as_array().unwrap().len(),
            "bids 数量"
        );
        assert_eq!(
            book.asks.len(),
            ccxt["asks"].as_array().unwrap().len(),
            "asks 数量"
        );
        for (i, (o, c)) in book
            .bids
            .iter()
            .zip(ccxt["bids"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec_rel(
                &format!("bids[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec_rel(
                &format!("bids[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
        for (i, (o, c)) in book
            .asks
            .iter()
            .zip(ccxt["asks"].as_array().unwrap().iter())
            .enumerate()
        {
            assert_dec_rel(
                &format!("asks[{i}].price"),
                o.price,
                common::value_decimal(&c[0]),
            );
            assert_dec_rel(
                &format!("asks[{i}].amount"),
                o.amount,
                common::value_decimal(&c[1]),
            );
        }
    }

    #[test]
    fn trades_match_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kraken", "trades");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("kraken", "trades"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let t = ex.parse_trade(r, "XXBTZUSD");
            assert_eq!(
                t.side.as_deref(),
                obj_str(c, "side").as_deref(),
                "trade[{i}].side"
            );
            assert_dec_rel(&format!("trade[{i}].price"), t.price, obj_dec(c, "price"));
            assert_dec_rel(
                &format!("trade[{i}].amount"),
                t.amount,
                obj_dec(c, "amount"),
            );
        }
    }

    #[test]
    fn ohlcv_matches_ccxt() {
        let ex = setup();
        let raw_fixture = load_raw("kraken", "ohlcv");
        let raw_arr = raw_fixture["raw"].as_array().unwrap().clone();
        let ccxt = parsed(&load_ccxt_parsed("kraken", "ohlcv"))
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(raw_arr.len(), ccxt.len(), "行数一致");
        for (i, (r, c)) in raw_arr.iter().zip(ccxt.iter()).enumerate() {
            let o = ex.parse_ohlcv(r);
            assert_eq!(o.timestamp, c[0].as_i64(), "ohlcv[{i}].ts");
            for (idx, key) in ["open", "high", "low", "close", "volume"]
                .iter()
                .enumerate()
            {
                let our = match *key {
                    "open" => o.open,
                    "high" => o.high,
                    "low" => o.low,
                    "close" => o.close,
                    _ => o.volume,
                };
                assert_dec_rel(
                    &format!("ohlcv[{i}].{key}"),
                    our,
                    common::value_decimal(&c[idx + 1]),
                );
            }
        }
    }
}
