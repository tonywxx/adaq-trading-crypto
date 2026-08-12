//! htx(Huobi)现货适配器(Phase B,ADR-0005)。
//!
//! 对齐 ccxt htx(spot v1)语义:
//! - 公共面 `GET /v1/common/...` 与 `GET /market/...`,symbol id 为**小写**
//!   (`btcusdt`);symbols 的 `base-currency/quote-currency` 为小写;
//! - merged ticker `bid/ask` 为 `[price, size]` 数组;
//! - kline 为对象 `{id(秒), open, close, low, high, amount, vol}`,
//!   volume = amount,ts = id×1000;
//! - trades `{id, ts, trade-id, amount, price, direction}`(trade-id 为 i64 内);
//! - depth `{tick: {bids/asks}}`;
//! - 私密面:AWS 风格签名(`SignatureMethod=HmacSHA256&SignatureVersion=2`,
//!   canonical = `METHOD\nhost\npath\nsorted-query`,base64 HMAC),fetch_balance
//!   需先取 spot 账户 id 再查余额;
//! - rateLimit 50ms。
//!
//! 参考实现基于 ccxt(MIT)适配器解析逻辑,见 `NOTICE`。

use std::sync::Mutex;

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::client::Client;
use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Exchange, Params};
use crate::types::{
    Balance, Balances, Level, Market, MarketType, Markets, OHLCV, OrderBook, Precision, Ticker,
    Tickers, Trade,
};

pub const ID: &str = "htx";
const BASE_URL: &str = "https://api.huobi.pro";
const RATE_LIMIT_MS: u64 = 50;

/// 常见计价币(小写,用于无缓存时推导统一 symbol)。
const COMMON_QUOTES: &[&str] = &[
    "usdt", "usdc", "husd", "tusd", "ht", "btc", "eth", "eur", "usd",
];

/// htx 现货适配器。
pub struct Htx {
    config: Config,
    client: Client,
    markets: Mutex<Option<Markets>>,
}

impl Htx {
    /// 已实现(非 NotSupported)的 REST 方法面(契约声明,Phase B)。
    pub const IMPLEMENTED: &'static [&'static str] = &[
        "fetch_time",
        "fetch_markets",
        "fetch_ticker",
        "fetch_tickers",
        "fetch_ohlcv",
        "fetch_order_book",
        "fetch_trades",
        "fetch_balance",
    ];

    pub fn new(config: Config) -> Result<Self> {
        let rate_limit_ms = if config.rate_limit_ms > 0 {
            config.rate_limit_ms
        } else {
            RATE_LIMIT_MS
        };
        let client = Client::new(
            config.timeout_ms,
            config.max_retries,
            config.proxy.as_deref(),
            rate_limit_ms,
            config.enable_rate_limit,
        )?;
        Ok(Self {
            config,
            client,
            markets: Mutex::new(None),
        })
    }

    // ================= 内部 HTTP =================

    async fn public_get(&self, path: &str, params: &Params) -> Result<Value> {
        let url = format!("{BASE_URL}{path}{}", query_string(params));
        let headers = HeaderMap::new();
        self.client.request("GET", &url, &headers, None).await
    }

    /// 私密 GET(AWS 风格签名)。
    async fn private_get(&self, path: &str, params: &Params) -> Result<Value> {
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "htx api_key required"))?;
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "htx secret required"))?;
        let timestamp = utc_now_ts();
        let mut p = params.clone();
        p.insert("AccessKeyId".into(), json!(api_key));
        p.insert("SignatureMethod".into(), json!("HmacSHA256"));
        p.insert("SignatureVersion".into(), json!("2"));
        p.insert("Timestamp".into(), json!(timestamp));
        let qs = sorted_query_string(&p);
        let canonical = format!("GET\napi.huobi.pro\n{path}\n{qs}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| Error::new(ErrorKind::Authentication, format!("hmac key: {e}")))?;
        mac.update(canonical.as_bytes());
        let signature =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let url = format!("{BASE_URL}{path}?{qs}&Signature={}", pct_encode(&signature));
        let mut headers = HeaderMap::new();
        headers.insert(
            "Content-Type",
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        self.client.request("GET", &url, &headers, None).await
    }

    // ================= markets 缓存 =================

    async fn load_markets(&self) -> Result<()> {
        if self.markets.lock().unwrap().is_some() {
            return Ok(());
        }
        let resp = self
            .public_get("/v1/common/symbols", &Params::new())
            .await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "symbols not array"))?;
        let mut map = Markets::new();
        for raw in arr {
            let m = self.parse_market(raw);
            map.insert(m.symbol.clone(), m);
        }
        *self.markets.lock().unwrap() = Some(map);
        Ok(())
    }

    /// 统一 symbol → id(`BTC/USDT` → `btcusdt` 小写)。
    fn symbol_id(&self, symbol: &str) -> String {
        symbol.replace('/', "").to_lowercase()
    }

    /// id → 统一 symbol:先查缓存,否则按常见计价币后缀剥离(小写)。
    pub fn market_symbol(&self, id: &str) -> String {
        if let Some(cache) = self.markets.lock().unwrap().as_ref() {
            if let Some(m) = cache.values().find(|m| m.id.eq_ignore_ascii_case(id)) {
                return m.symbol.clone();
            }
        }
        let lower = id.to_lowercase();
        for q in COMMON_QUOTES {
            if let Some(base) = lower.strip_suffix(q) {
                if !base.is_empty() {
                    return format!("{}/{}", base.to_uppercase(), q.to_uppercase());
                }
            }
        }
        id.to_string()
    }

    // ================= parse(公开,供差分测试) =================

    pub fn parse_market(&self, raw: &Value) -> Market {
        let base_id = raw["base-currency"].as_str().unwrap_or_default();
        let quote_id = raw["quote-currency"].as_str().unwrap_or_default();
        let base = base_id.to_uppercase();
        let quote = quote_id.to_uppercase();
        let precision = |scale: Option<&Value>| {
            scale
                .and_then(|v| match v {
                    Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
                    Value::String(s) => s.parse::<u32>().ok(),
                    _ => None,
                })
                .map(|n| rust_decimal::Decimal::new(1, n))
        };
        Market {
            id: format!("{}{}", base_id, quote_id),
            symbol: format!("{base}/{quote}"),
            base: Some(base),
            quote: Some(quote),
            base_id: Some(base_id.to_string()),
            quote_id: Some(quote_id.to_string()),
            active: Some(raw["state"].as_str() == Some("online")),
            market_type: Some(MarketType::Spot),
            spot: Some(true),
            taker: Some("0.002".parse().unwrap()),
            maker: Some("0.002".parse().unwrap()),
            precision: Precision {
                price: precision(raw.get("price-precision")),
                amount: precision(raw.get("amount-precision")),
                cost: precision(raw.get("value-precision")),
            },
            info: raw.clone(),
            ..Market::default()
        }
    }

    pub fn parse_ticker(&self, raw: &Value) -> Ticker {
        let id = raw["symbol"].as_str().unwrap_or_default();
        let ts = raw["ts"].as_i64();
        let close = num(raw.get("close"));
        let (bid, bid_size) = bid_ask(raw.get("bid"));
        let (ask, ask_size) = bid_ask(raw.get("ask"));
        Ticker {
            symbol: self.market_symbol(id),
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            high: num(raw.get("high")),
            low: num(raw.get("low")),
            bid,
            ask,
            bid_volume: bid_size,
            ask_volume: ask_size,
            open: num(raw.get("open")),
            close,
            last: close,
            base_volume: num(raw.get("amount")),
            quote_volume: num(raw.get("vol")),
            info: raw.clone(),
            ..Ticker::default()
        }
    }

    pub fn parse_ohlcv(&self, row: &Value) -> OHLCV {
        // htx kline 为对象:{id(秒), open, close, low, high, amount, vol}
        let ts = row
            .get("id")
            .and_then(|v| match v {
                Value::Number(n) => n.as_i64(),
                Value::String(s) => s.parse().ok(),
                _ => None,
            })
            .map(|s| s * 1000);
        OHLCV {
            timestamp: ts,
            open: num(row.get("open")),
            high: num(row.get("high")),
            low: num(row.get("low")),
            close: num(row.get("close")),
            volume: num(row.get("amount")),
        }
    }

    pub fn parse_trade(&self, raw: &Value) -> Trade {
        let ts = raw["ts"].as_i64();
        let price = num(raw.get("price"));
        let amount = num(raw.get("amount"));
        // trade-id 为数字或字符串(htx 原始响应为数字)
        let id = match raw.get("trade-id") {
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        Trade {
            id,
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            side: raw["direction"].as_str().map(str::to_string),
            price,
            amount,
            cost: match (price, amount) {
                (Some(p), Some(a)) => Some(p * a),
                _ => None,
            },
            info: raw.clone(),
            ..Trade::default()
        }
    }

    pub fn parse_order_book(&self, raw: &Value, symbol: &str) -> OrderBook {
        let tick = raw.get("tick").unwrap_or(raw);
        let mut bids: Vec<Level> = tick
            .get("bids")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(parse_level).collect())
            .unwrap_or_default();
        let mut asks: Vec<Level> = tick
            .get("asks")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(parse_level).collect())
            .unwrap_or_default();
        bids.sort_by_key(|l| std::cmp::Reverse(l.price));
        asks.sort_by_key(|l| l.price);
        OrderBook {
            symbol: symbol.to_string(),
            bids,
            asks,
            timestamp: tick["ts"].as_i64(),
            info: tick.clone(),
            ..OrderBook::default()
        }
    }
}

// ================= Exchange trait =================

impl Exchange for Htx {
    fn id(&self) -> &'static str {
        ID
    }
    fn config(&self) -> &Config {
        &self.config
    }

    async fn fetch_time(&self) -> Result<i64> {
        let resp = self
            .public_get("/v1/common/timestamp", &Params::new())
            .await?;
        resp["data"]
            .as_i64()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "missing timestamp"))
    }

    async fn fetch_markets(&self) -> Result<Vec<Market>> {
        self.load_markets().await?;
        Ok(self
            .markets
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default()
            .into_values()
            .collect())
    }

    async fn fetch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        let p = params1("symbol", &self.symbol_id(symbol));
        let resp = self.public_get("/market/detail/merged", &p).await?;
        let tick = resp
            .get("tick")
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "missing tick"))?;
        let mut t = self.parse_ticker(tick);
        // merged 响应的 ts 在顶层
        if t.timestamp.is_none() {
            t.timestamp = resp["ts"].as_i64();
            t.datetime = t.timestamp.and_then(iso8601);
        }
        Ok(t)
    }

    async fn fetch_tickers(&self, _symbols: Option<&[&str]>, _params: Params) -> Result<Tickers> {
        let resp = self.public_get("/market/tickers", &Params::new()).await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut out = Tickers::new();
        for raw in arr {
            let t = self.parse_ticker(&raw);
            out.insert(t.symbol.clone(), t);
        }
        Ok(out)
    }

    async fn fetch_ohlcv(
        &self,
        symbol: &str,
        timeframe: &str,
        _since: Option<i64>,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<OHLCV>> {
        let period = match timeframe {
            "1m" => "1min",
            "5m" => "5min",
            "15m" => "15min",
            "30m" => "30min",
            "1h" => "60min",
            "4h" => "4hour",
            "1d" => "1day",
            other => {
                return Err(Error::new(
                    ErrorKind::BadRequest,
                    format!("unsupported timeframe {other}"),
                ));
            }
        };
        let mut p = Params::new();
        p.insert("symbol".into(), json!(self.symbol_id(symbol)));
        p.insert("period".into(), json!(period));
        if let Some(l) = limit {
            p.insert("size".into(), json!(l.min(2000)));
        }
        let resp = self.public_get("/market/history/kline", &p).await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().map(|r| self.parse_ohlcv(r)).collect())
    }

    async fn fetch_order_book(
        &self,
        symbol: &str,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        let mut p = Params::new();
        p.insert("symbol".into(), json!(self.symbol_id(symbol)));
        p.insert("type".into(), json!("step0"));
        if let Some(l) = limit {
            p.insert("depth".into(), json!(l.min(150)));
        }
        let resp = self.public_get("/market/depth", &p).await?;
        Ok(self.parse_order_book(&resp, symbol))
    }

    async fn fetch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        let p = params1("symbol", &self.symbol_id(symbol));
        let resp = self.public_get("/market/trade", &p).await?;
        let arr = resp
            .get("tick")
            .and_then(|t| t.get("data"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().map(|t| self.parse_trade(t)).collect())
    }

    async fn fetch_balance(&self, _params: Params) -> Result<Balances> {
        // 1) 取 spot 账户 id
        let accounts = self
            .private_get("/v1/account/accounts", &Params::new())
            .await?;
        let account_id = accounts
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter()
                    .find(|x| {
                        x["type"].as_str() == Some("spot") || x["subtype"].as_str() == Some("spot")
                    })
                    .or_else(|| a.first())
            })
            .and_then(|x| x["id"].as_i64())
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "no spot account"))?;
        // 2) 查余额
        let path = format!("/v1/account/accounts/{account_id}/balance");
        let resp = self.private_get(&path, &Params::new()).await?;
        let mut out = Balances {
            info: resp.clone(),
            ..Balances::default()
        };
        if let Some(list) = resp
            .get("data")
            .and_then(|d| d.get("list"))
            .and_then(Value::as_array)
        {
            for entry in list {
                if let Some(code) = entry["currency"].as_str() {
                    let bal = num(entry.get("balance"));
                    let is_frozen = entry["type"].as_str() == Some("frozen");
                    let acc = out
                        .accounts
                        .entry(code.to_string())
                        .or_insert_with(|| Balance {
                            ..Balance::default()
                        });
                    if is_frozen {
                        acc.used = bal;
                    } else {
                        acc.free = bal;
                    }
                }
            }
        }
        for acc in out.accounts.values_mut() {
            acc.total = match (acc.free, acc.used) {
                (Some(f), Some(u)) => Some(f + u),
                (Some(f), None) => Some(f),
                (None, Some(u)) => Some(u),
                _ => None,
            };
        }
        Ok(out)
    }
}

// ================= 静态助手 =================

/// bid/ask 兼容标量或 `[price, size]` 数组(htx merged ticker)。
fn bid_ask(v: Option<&Value>) -> (Option<rust_decimal::Decimal>, Option<rust_decimal::Decimal>) {
    match v {
        Some(Value::Array(arr)) => (
            arr.first().and_then(value_decimal),
            arr.get(1).and_then(value_decimal),
        ),
        Some(other) => (value_decimal(other), None),
        None => (None, None),
    }
}

fn params1(k: &str, v: &str) -> Params {
    let mut p = Params::new();
    p.insert(k.into(), json!(v));
    p
}

fn num(v: Option<&Value>) -> Option<rust_decimal::Decimal> {
    v.and_then(value_decimal)
}

pub fn value_decimal(v: &Value) -> Option<rust_decimal::Decimal> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.to_string().parse().ok(),
        _ => None,
    }
}

/// `[price, amount]` → Level。
fn parse_level(raw: &Value) -> Level {
    let arr = raw.as_array();
    Level {
        price: arr.and_then(|a| a.first()).and_then(value_decimal),
        amount: arr.and_then(|a| a.get(1)).and_then(value_decimal),
    }
}

fn query_string(params: &Params) -> String {
    if params.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(&val_str(v))))
        .collect();
    format!("?{}", pairs.join("&"))
}

/// 签名用 query:按 key 排序(htx/AWS 要求)。
fn sorted_query_string(params: &Params) -> String {
    let mut keys: Vec<&String> = params.keys().collect();
    keys.sort();
    let pairs: Vec<String> = keys
        .iter()
        .map(|k| format!("{}={}", pct_encode(k), pct_encode(&val_str(&params[*k]))))
        .collect();
    pairs.join("&")
}

fn val_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn utc_now_ts() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_id_lowercases() {
        let ex = Htx::new(Config::new()).unwrap();
        assert_eq!(ex.symbol_id("BTC/USDT"), "btcusdt");
        assert_eq!(ex.market_symbol("btcusdt"), "BTC/USDT");
    }

    #[test]
    fn parse_market_spot() {
        let ex = Htx::new(Config::new()).unwrap();
        let raw = json!({
            "base-currency": "btc",
            "quote-currency": "usdt",
            "symbol": "btcusdt",
            "price-precision": 2,
            "amount-precision": 6,
            "value-precision": 8,
            "state": "online"
        });
        let m = ex.parse_market(&raw);
        assert_eq!(m.id, "btcusdt");
        assert_eq!(m.symbol, "BTC/USDT");
        assert_eq!(m.active, Some(true));
        assert_eq!(m.precision.price, Some("0.01".parse().unwrap()));
        assert_eq!(m.precision.amount, Some("0.000001".parse().unwrap()));
    }

    #[test]
    fn parse_ticker_bid_ask_arrays() {
        let ex = Htx::new(Config::new()).unwrap();
        let raw = json!({
            "symbol": "btcusdt",
            "bid": [63788.52, 0.02325],
            "ask": [63788.53, 1.198073],
            "open": 64068.21,
            "close": 63788.52,
            "high": 64473.14,
            "low": 63266.78,
            "amount": 1745.8431843241613,
            "vol": 111435329.40185648
        });
        let t = ex.parse_ticker(&raw);
        assert_eq!(t.symbol, "BTC/USDT");
        assert_eq!(t.bid, Some("63788.52".parse().unwrap()));
        assert_eq!(t.bid_volume, Some("0.02325".parse().unwrap()));
        assert_eq!(t.last, Some("63788.52".parse().unwrap()));
    }

    #[test]
    fn parse_ohlcv_object_shape() {
        let ex = Htx::new(Config::new()).unwrap();
        let row = json!({
            "id": 1786522380,
            "open": 63799.95,
            "close": 63788.53,
            "low": 63788.52,
            "high": 63799.95,
            "amount": 0.008462757613545781,
            "vol": 539.85010348,
            "count": 9
        });
        let c = ex.parse_ohlcv(&row);
        assert_eq!(c.timestamp, Some(1786522380000));
        assert_eq!(c.volume, Some("0.008462757613545781".parse().unwrap()));
        assert_eq!(c.close, Some("63788.53".parse().unwrap()));
    }

    #[test]
    fn parse_trade_fields() {
        let ex = Htx::new(Config::new()).unwrap();
        let raw = json!({
            "id": "1925696197891664602204966497",
            "ts": 1786522431787_i64,
            "trade-id": 103627269709_i64,
            "amount": 0.00622,
            "price": 63788.53,
            "direction": "buy"
        });
        let t = ex.parse_trade(&raw);
        assert_eq!(t.id.as_deref(), Some("103627269709"));
        assert_eq!(t.side.as_deref(), Some("buy"));
        assert_eq!(t.timestamp, Some(1786522431787));
    }
}
