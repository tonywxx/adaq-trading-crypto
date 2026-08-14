//! 转译生成路径的共享 best-effort 解析器(ADR-0013 第三缝 parse 的通用实现)。
//!
//! 本模块从 `generic.rs` 拆分而出,仅承载 generated 路径的 `parse_*` 消费者;
//! ADR-0016 契约锚(`key_matches` / `find_first` / `fill_path` / `sign_*`)保留在
//! `generic.rs` 以保持文本稳定,与本模块无依赖。拆分不影响 `scripts/test_sync.py`
//! 对 `find_first(&[...])` 与 `key_matches` 的扫描。

use serde_json::Value;

use crate::httpcore::{
    dec_f64, iso8601, parse_level, pick_decimal, pick_i64, pick_str, to_i64, value_decimal,
};
use crate::types::{
    Balance, Balances, Currencies, Currency, Level, Limit, Limits, Market, MarketType, Markets,
    OHLCV, Order, OrderBook, Precision, Ticker, Tickers, Trade,
};

pub(crate) fn parse_market_type(s: &str) -> MarketType {
    match s.to_lowercase().as_str() {
        "spot" => MarketType::Spot,
        "margin" => MarketType::Margin,
        "swap" => MarketType::Swap,
        "future" => MarketType::Future,
        "option" => MarketType::Option,
        "delivery" => MarketType::Delivery,
        "index" => MarketType::Index,
        "prediction" => MarketType::Prediction,
        "binary" => MarketType::Binary,
        "categorical" => MarketType::Categorical,
        _ => MarketType::Other,
    }
}

/// 若响应是 `{symbol: ticker}` 映射或单元素映射,取出内层对象。
pub(crate) fn resolve_one<'a>(raw: &'a Value, symbol: &str) -> &'a Value {
    if let Some(o) = raw.as_object() {
        if let Some(v) = o.get(symbol) {
            return v;
        }
        if o.len() == 1 {
            if let Some(v) = o.values().next() {
                return v;
            }
        }
    }
    raw
}

pub(crate) fn parse_ticker(raw: &Value, symbol: &str) -> Ticker {
    let ts = pick_i64(
        raw,
        &["timestamp", "ts", "time", "t", "updated", "updatedAt"],
    );
    Ticker {
        symbol: if symbol.is_empty() {
            pick_str(raw, &["symbol"]).unwrap_or("").to_string()
        } else {
            symbol.to_string()
        },
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        high: pick_decimal(raw, &["high", "highPrice", "h"]),
        low: pick_decimal(raw, &["low", "lowPrice", "l"]),
        bid: pick_decimal(raw, &["bid", "bidPrice", "b"]),
        ask: pick_decimal(raw, &["ask", "askPrice", "a"]),
        bid_volume: pick_decimal(raw, &["bidVolume", "bidQty"]),
        ask_volume: pick_decimal(raw, &["askVolume", "askQty"]),
        open: pick_decimal(raw, &["open", "openPrice", "o"]),
        close: pick_decimal(raw, &["close", "last", "lastPrice", "price", "c"]),
        last: pick_decimal(raw, &["last", "lastPrice", "close", "price", "c"]),
        vwap: pick_decimal(raw, &["vwap"]),
        base_volume: pick_decimal(raw, &["baseVolume", "volume", "vol", "v"]),
        quote_volume: pick_decimal(raw, &["quoteVolume", "quote_volume"]),
        percentage: pick_decimal(raw, &["percentage", "changePercent", "priceChangePercent"]),
        info: raw.clone(),
        ..Default::default()
    }
}

pub(crate) fn parse_tickers(raw: &Value) -> Tickers {
    let mut map = Tickers::new();
    if let Some(o) = raw.as_object() {
        // 形如 { "BTC/USDT": {...}, ... } 或 { "tickers"? }
        for (sym, v) in o {
            if v.is_object() && (v.get("last").is_some() || v.get("price").is_some()) {
                map.insert(sym.clone(), parse_ticker(v, sym));
            }
        }
    }
    if map.is_empty() {
        if let Some(arr) = raw.get("tickers").and_then(|x| x.as_array()) {
            for v in arr {
                let sym = pick_str(v, &["symbol"]).unwrap_or("").to_string();
                map.insert(sym.clone(), parse_ticker(v, &sym));
            }
        }
    }
    map
}

pub(crate) fn parse_ohlcv(raw: &Value) -> Vec<OHLCV> {
    let arr = match raw {
        Value::Array(a) => a,
        // 常见 { data:[...] } / { result:[...] }
        Value::Object(o) => o
            .values()
            .find(|v| v.is_array())
            .and_then(|v| v.as_array())
            .map(|v| v.as_slice())
            .unwrap_or(&[]),
        _ => return vec![],
    };
    arr.iter()
        .filter_map(|e| {
            if let Some(a) = e.as_array() {
                if a.len() >= 6 {
                    return Some(OHLCV {
                        timestamp: a[0].as_i64(),
                        open: value_decimal(&a[1]).or_else(|| a[1].as_f64().and_then(dec_f64)),
                        high: value_decimal(&a[2]).or_else(|| a[2].as_f64().and_then(dec_f64)),
                        low: value_decimal(&a[3]).or_else(|| a[3].as_f64().and_then(dec_f64)),
                        close: value_decimal(&a[4]).or_else(|| a[4].as_f64().and_then(dec_f64)),
                        volume: value_decimal(&a[5]).or_else(|| a[5].as_f64().and_then(dec_f64)),
                    });
                }
            }
            e.as_object().map(|_| OHLCV {
                timestamp: pick_i64(e, &["timestamp", "time", "t", "openTime"]),
                open: pick_decimal(e, &["open", "o"]),
                high: pick_decimal(e, &["high", "h"]),
                low: pick_decimal(e, &["low", "l"]),
                close: pick_decimal(e, &["close", "c", "last", "price"]),
                volume: pick_decimal(e, &["volume", "v", "vol"]),
            })
        })
        .collect()
}

pub(crate) fn parse_trades(raw: &Value, symbol: &str) -> Vec<Trade> {
    let arr = match raw {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .values()
            .find(|v| v.is_array())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => vec![],
    };
    arr.iter().map(|v| parse_trade(v, symbol)).collect()
}

pub(crate) fn parse_trade(raw: &Value, symbol: &str) -> Trade {
    let ts = pick_i64(raw, &["timestamp", "ts", "time", "t", "datetime"]);
    Trade {
        id: pick_str(raw, &["id", "tradeId", "tid"]).map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: pick_str(raw, &["symbol"]).map(str::to_string).or_else(|| {
            if symbol.is_empty() {
                None
            } else {
                Some(symbol.to_string())
            }
        }),
        side: pick_str(raw, &["side"]).map(|s| s.to_lowercase()),
        price: pick_decimal(raw, &["price", "p", "avgPrice"]),
        amount: pick_decimal(raw, &["amount", "qty", "quantity", "a", "size", "vol"]),
        cost: pick_decimal(raw, &["cost", "value"]),
        order: pick_str(raw, &["order", "orderId"]).map(str::to_string),
        taker_or_maker: pick_str(raw, &["takerOrMaker", "taker_or_maker"]).map(str::to_string),
        info: raw.clone(),
        ..Default::default()
    }
}

pub(crate) fn parse_levels(v: Option<&Value>) -> Vec<Level> {
    match v {
        Some(Value::Array(a)) => a.iter().map(parse_level).collect(),
        _ => vec![],
    }
}

pub(crate) fn parse_order_book(raw: &Value, symbol: &str) -> OrderBook {
    let (bids, asks) = if let Some(o) = raw.as_object() {
        (parse_levels(o.get("bids")), parse_levels(o.get("asks")))
    } else {
        (vec![], vec![])
    };
    let ts = pick_i64(raw, &["timestamp", "ts", "updated", "E"]);
    OrderBook {
        symbol: symbol.to_string(),
        bids,
        asks,
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        nonce: raw.get("nonce").and_then(to_i64),
        info: raw.clone(),
    }
}

pub(crate) fn parse_orders(raw: &Value, symbol: Option<&str>) -> Vec<Order> {
    let arr = match raw {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .values()
            .find(|v| v.is_array())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default(),
        _ => vec![],
    };
    arr.iter()
        .map(|v| parse_order(v, symbol.unwrap_or("")))
        .collect()
}

pub(crate) fn parse_order(raw: &Value, symbol: &str) -> Order {
    let ts = pick_i64(
        raw,
        &[
            "timestamp",
            "createdAt",
            "created",
            "time",
            "datetime",
            "transactTime",
            "updateTime",
        ],
    );
    Order {
        id: pick_str(raw, &["id", "orderId", "order_id"]).map(str::to_string),
        client_order_id: pick_str(raw, &["clientOrderId", "client_order_id", "clientId"])
            .map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        status: pick_str(raw, &["status"]).map(str::to_string),
        symbol: pick_str(raw, &["symbol"]).map(str::to_string).or_else(|| {
            if symbol.is_empty() {
                None
            } else {
                Some(symbol.to_string())
            }
        }),
        order_type: pick_str(raw, &["type"]).map(str::to_string),
        side: pick_str(raw, &["side"]).map(|s| s.to_lowercase()),
        price: pick_decimal(raw, &["price", "priceAvg", "avgPrice"]),
        amount: pick_decimal(raw, &["amount", "quantity", "qty", "origQty", "vol"]),
        filled: pick_decimal(raw, &["filled", "filledAmount", "executedQty", "filledQty"]),
        remaining: pick_decimal(raw, &["remaining", "remainingAmount", "remainingQty"]),
        cost: pick_decimal(raw, &["cost", "cummulativeQuoteQty", "cumQuote"]),
        info: raw.clone(),
        ..Default::default()
    }
}

pub(crate) fn parse_balance(raw: &Value) -> Balances {
    let mut b = Balances {
        info: raw.clone(),
        ..Default::default()
    };
    if let Some(o) = raw.as_object() {
        // 形状 1:{ BTC:{free,used,total}, ... }
        for (code, v) in o {
            if let Some(cv) = v.as_object() {
                if cv.contains_key("free")
                    || cv.contains_key("used")
                    || cv.contains_key("total")
                    || cv.contains_key("available")
                {
                    b.accounts.insert(
                        code.clone(),
                        Balance {
                            free: pick_decimal(v, &["free", "available", "avail"]),
                            used: pick_decimal(v, &["used", "locked", "onOrder", "onorder"]),
                            total: pick_decimal(v, &["total", "balance"]),
                            ..Default::default()
                        },
                    );
                }
            }
        }
        // 形状 2:{ balances:[{asset,free,locked}], ... }
        if b.accounts.is_empty() {
            if let Some(arr) = raw.get("balances").and_then(|x| x.as_array()) {
                for v in arr {
                    let code = pick_str(v, &["asset", "currency", "coin", "code"]).unwrap_or("");
                    b.accounts.insert(
                        code.to_string(),
                        Balance {
                            free: pick_decimal(v, &["free", "available"]),
                            used: pick_decimal(v, &["used", "locked"]),
                            total: pick_decimal(v, &["total", "balance"]),
                            ..Default::default()
                        },
                    );
                }
            }
        }
        // 形状 3:{ result:{ list:[...] } }
        if b.accounts.is_empty() {
            if let Some(arr) = raw
                .get("result")
                .and_then(|r| r.get("list"))
                .and_then(|x| x.as_array())
            {
                for v in arr {
                    let code = pick_str(v, &["currency", "asset", "coin", "code"]).unwrap_or("");
                    b.accounts.insert(
                        code.to_string(),
                        Balance {
                            free: pick_decimal(v, &["available", "free"]),
                            used: pick_decimal(v, &["locked", "used"]),
                            total: pick_decimal(v, &["total", "balance"]),
                            ..Default::default()
                        },
                    );
                }
            }
        }
    }
    b
}

pub(crate) fn parse_currencies(raw: &Value) -> Currencies {
    let mut map = Currencies::new();
    match raw {
        // { BTC:{...}, ... }
        Value::Object(o) => {
            for (code, v) in o {
                if v.is_object() {
                    map.insert(code.clone(), parse_currency(v, code));
                }
            }
            if map.is_empty() {
                for (code, v) in o {
                    if let Some(a) = v.as_array() {
                        for item in a {
                            if item.is_object() {
                                let c = pick_str(item, &["currency", "code", "asset"])
                                    .unwrap_or(code)
                                    .to_string();
                                map.insert(c.clone(), parse_currency(item, &c));
                            }
                        }
                    }
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                let code = pick_str(v, &["code", "currency", "id"])
                    .unwrap_or("")
                    .to_string();
                map.insert(code.clone(), parse_currency(v, &code));
            }
        }
        _ => {}
    }
    map
}

pub(crate) fn parse_currency(raw: &Value, code: &str) -> Currency {
    Currency {
        id: pick_str(raw, &["id", "currency"])
            .map(str::to_string)
            .unwrap_or_else(|| code.to_string()),
        code: pick_str(raw, &["code", "currency"])
            .map(str::to_string)
            .unwrap_or_else(|| code.to_string()),
        name: pick_str(raw, &["name"]).map(str::to_string),
        active: raw.get("active").and_then(|x| x.as_bool()),
        precision: raw.get("precision").and_then(|x| x.as_i64()),
        currency_type: pick_str(raw, &["type", "currencyType"]).map(str::to_string),
        deposit: raw.get("deposit").and_then(|x| x.as_bool()),
        withdraw: raw.get("withdraw").and_then(|x| x.as_bool()),
        fee: pick_decimal(raw, &["fee"]),
        info: raw.clone(),
        ..Default::default()
    }
}

pub(crate) fn parse_markets(raw: &Value) -> Markets {
    let mut map = Markets::new();
    let mut items: Vec<&Value> = match raw {
        Value::Array(a) => a.iter().collect(),
        Value::Object(o) => {
            let obj_vals: Vec<&Value> = o.values().filter(|v| v.is_object()).collect();
            if !obj_vals.is_empty() {
                obj_vals
            } else {
                o.values()
                    .find(|v| v.is_array())
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().collect())
                    .unwrap_or_default()
            }
        }
        _ => vec![],
    };
    // 若顶层对象内嵌 { result:[...] } / { data:[...] }
    if items.is_empty() {
        if let Some(inner) = raw.get("result").or_else(|| raw.get("data")) {
            if let Some(a) = inner.as_array() {
                items = a.iter().collect();
            }
        }
    }
    for it in items {
        let m = parse_market(it);
        if !m.symbol.is_empty() {
            map.insert(m.symbol.clone(), m);
        }
    }
    map
}

pub(crate) fn parse_market(raw: &Value) -> Market {
    let id = pick_str(raw, &["id", "market"]).unwrap_or("").to_string();
    let base_id = pick_str(raw, &["baseId", "base_id", "base"]).map(str::to_string);
    let quote_id = pick_str(raw, &["quoteId", "quote_id", "quote"]).map(str::to_string);
    let symbol = pick_str(raw, &["symbol"])
        .map(str::to_string)
        .or_else(|| {
            let b = base_id.clone().unwrap_or_default();
            let q = quote_id.clone().unwrap_or_default();
            if b.is_empty() && q.is_empty() {
                None
            } else {
                Some(format!("{b}/{q}"))
            }
        })
        .unwrap_or_default();
    Market {
        id,
        symbol,
        base: base_id.clone(),
        quote: quote_id.clone(),
        base_id,
        quote_id,
        active: raw.get("active").and_then(|x| x.as_bool()).or(Some(true)),
        market_type: raw
            .get("type")
            .and_then(|x| x.as_str())
            .map(parse_market_type),
        spot: raw.get("spot").and_then(|x| x.as_bool()),
        swap: raw.get("swap").and_then(|x| x.as_bool()),
        future: raw.get("future").and_then(|x| x.as_bool()),
        option: raw.get("option").and_then(|x| x.as_bool()),
        precision: Precision {
            amount: raw
                .get("precision")
                .and_then(|p| p.get("amount"))
                .and_then(value_decimal),
            price: raw
                .get("precision")
                .and_then(|p| p.get("price"))
                .and_then(value_decimal),
            ..Default::default()
        },
        limits: Limits {
            amount: raw
                .get("limits")
                .and_then(|l| l.get("amount"))
                .map(|a| Limit {
                    min: a.get("min").and_then(value_decimal),
                    max: a.get("max").and_then(value_decimal),
                }),
            price: raw
                .get("limits")
                .and_then(|l| l.get("price"))
                .map(|a| Limit {
                    min: a.get("min").and_then(value_decimal),
                    max: a.get("max").and_then(value_decimal),
                }),
            ..Default::default()
        },
        info: raw.clone(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_ticker_common_shape() {
        let raw = json!({
            "symbol": "BTC/USDT",
            "last": "50000",
            "high": "51000",
            "low": "49000",
            "bid": "49900",
            "ask": "50100",
            "volume": "123.4",
            "timestamp": 1700000000000_i64
        });
        let t = parse_ticker(&raw, "BTC/USDT");
        assert_eq!(t.symbol, "BTC/USDT");
        assert_eq!(t.last.unwrap().to_string(), "50000");
        assert_eq!(t.high.unwrap().to_string(), "51000");
        assert_eq!(t.base_volume.unwrap().to_string(), "123.4");
        assert_eq!(t.timestamp, Some(1700000000000_i64));
    }

    #[test]
    fn parse_order_book_levels() {
        let raw = json!({
            "bids": [["50000", "1.5"], ["49900", "2.0"]],
            "asks": [["50100", "1.2"]]
        });
        let ob = parse_order_book(&raw, "BTC/USDT");
        assert_eq!(ob.bids.len(), 2);
        assert_eq!(ob.asks.len(), 1);
        assert_eq!(ob.bids[0].price.unwrap().to_string(), "50000");
    }

    #[test]
    fn parse_balance_two_shapes() {
        let a = json!({"BTC": {"free":"1.0","used":"0.5","total":"1.5"}});
        let b = parse_balance(&a);
        assert_eq!(b.accounts["BTC"].free.unwrap().to_string(), "1.0");

        let c = json!({"balances":[{"asset":"ETH","free":"2","locked":"1"}]});
        let d = parse_balance(&c);
        assert_eq!(d.accounts["ETH"].free.unwrap().to_string(), "2");
        assert_eq!(d.accounts["ETH"].used.unwrap().to_string(), "1");
    }
}
