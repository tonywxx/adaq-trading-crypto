//! hashkey 适配器(ADR-0017,由 generated promote 为 curated)。
//!
//! 手写完整交易 API。HashKey Exchange 签名:`HMAC-SHA256(secret,
//! method + timestamp + path + (query|body))`,头 `X-HK-APIKEY` /
//! `X-HK-TIMESTAMP` / `X-HK-SIGNATURE`(base `https://api.hashkey.com`)。
//! 端点路径以官方文档为准(此处采用 v1 命名,待核对)。

use std::collections::HashMap;
use std::str::FromStr;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Exchange, Params};
use crate::httpcore::{HttpCore, now_ms, parse_level, query_string};
use crate::types::{
    Balance, Balances, Market, MarketType, Markets, OHLCV, Order, OrderBook, Precision, Ticker,
    Tickers, Trade,
};

pub const ID: &str = "hashkey";
const BASE_URL: &str = "https://api.hashkey.com";
const RATE_LIMIT_MS: u64 = 50;

const COMMON_QUOTES: &[&str] = &[
    "USDT", "USDC", "FDUSD", "TUSD", "BUSD", "DAI", "EUR", "USD", "BTC", "ETH", "BNB", "PAX",
];

pub struct Hashkey {
    config: Config,
    core: HttpCore,
}

impl Hashkey {
    pub const IMPLEMENTED: &'static [&'static str] = &[
        "fetch_time",
        "fetch_markets",
        "fetch_ticker",
        "fetch_tickers",
        "fetch_order_book",
        "fetch_trades",
        "fetch_ohlcv",
        "fetch_balance",
        "create_order",
        "cancel_order",
        "fetch_order",
        "fetch_open_orders",
        "fetch_orders",
        "fetch_my_trades",
    ];

    pub fn new(config: Config) -> Result<Self> {
        let core = HttpCore::new(&config, BASE_URL, RATE_LIMIT_MS, "hashkey")?;
        Ok(Self { config, core })
    }

    fn symbol_id(&self, symbol: &str) -> String {
        if symbol.contains('/') {
            let (base, quote) = symbol.split_once('/').unwrap();
            format!("{base}{quote}")
        } else {
            symbol.to_string()
        }
    }

    fn market_symbol(&self, id: &str) -> String {
        if let Some(m) = self.core.markets_snapshot().values().find(|m| m.id == id) {
            return m.symbol.clone();
        }
        for q in COMMON_QUOTES {
            if let Some(base) = id.strip_suffix(q) {
                if !base.is_empty() {
                    return format!("{base}/{q}");
                }
            }
        }
        id.to_string()
    }

    async fn public_request(&self, path: &str, params: &Params) -> Result<Value> {
        let url = format!("{BASE_URL}{path}{}", query_string(params));
        self.core
            .request_url("GET", &url, &HeaderMap::new(), None)
            .await
    }

    async fn private_request(
        &self,
        method: &str,
        path: &str,
        params: &Params,
        body: Option<Value>,
    ) -> Result<Value> {
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "hashkey api_key required"))?;
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "hashkey secret required"))?;
        let timestamp = now_ms().to_string();
        // GET:path 含 query;POST:path + body
        let (request_path, body_json): (String, Option<Value>) = if method == "GET" {
            (format!("{path}{}", query_string(params)), None)
        } else {
            let b = body.or_else(|| {
                if params.is_empty() {
                    None
                } else {
                    Some(json!(params))
                }
            });
            (path.to_string(), b)
        };
        let body_str = body_json
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_default();
        let presign = format!("{method}{timestamp}{request_path}{body_str}");
        let signature = sign_hmac_sha256(&presign, secret);
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-HK-APIKEY",
            HeaderValue::from_str(api_key)
                .map_err(|e| Error::new(ErrorKind::BadRequest, format!("apikey: {e}")))?,
        );
        headers.insert("X-HK-TIMESTAMP", HeaderValue::from_str(&timestamp).unwrap());
        headers.insert("X-HK-SIGNATURE", HeaderValue::from_str(&signature).unwrap());
        let url = format!("{BASE_URL}{request_path}");
        self.core
            .request_url(method, &url, &headers, body_json)
            .await
    }

    async fn fetch_markets_raw(&self) -> Result<Markets> {
        let resp = self
            .public_request("/api/v1/exchangeInfo", &Params::new())
            .await?;
        let arr = resp["symbols"].as_array().cloned().unwrap_or_default();
        let mut map = Markets::new();
        for raw in arr {
            let m = self.parse_market(&raw);
            map.insert(m.symbol.clone(), m);
        }
        Ok(map)
    }

    pub fn parse_market(&self, raw: &Value) -> Market {
        let id = raw["symbol"].as_str().unwrap_or_default().to_string();
        Market {
            id: id.clone(),
            symbol: self.market_symbol(&id),
            base: raw["baseAsset"].as_str().map(String::from),
            quote: raw["quoteAsset"].as_str().map(String::from),
            base_id: raw["baseAsset"].as_str().map(String::from),
            quote_id: raw["quoteAsset"].as_str().map(String::from),
            active: Some(
                raw["status"]
                    .as_str()
                    .map(|s| s == "TRADING" || s == "1")
                    .unwrap_or(true),
            ),
            market_type: Some(MarketType::Spot),
            spot: Some(true),
            precision: Precision {
                amount: raw["lotSize"].as_str().and_then(|s| s.parse().ok()),
                price: raw["tickSize"].as_str().and_then(|s| s.parse().ok()),
                cost: None,
            },
            limits: crate::types::Limits::default(),
            info: raw.clone(),
            ..Market::default()
        }
    }

    pub fn parse_ticker(&self, raw: &Value) -> Ticker {
        let ts = raw["timestamp"].as_i64().or_else(|| raw["time"].as_i64());
        Ticker {
            symbol: self.market_symbol(raw["symbol"].as_str().unwrap_or_default()),
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            high: num(raw.get("highPrice")),
            low: num(raw.get("lowPrice")),
            bid: num(raw.get("bidPrice")),
            ask: num(raw.get("askPrice")),
            open: num(raw.get("openPrice")),
            close: num(raw.get("lastPrice")),
            last: num(raw.get("lastPrice")),
            base_volume: num(raw.get("volume")),
            quote_volume: num(raw.get("quoteVolume")),
            info: raw.clone(),
            ..Ticker::default()
        }
    }

    pub fn parse_ohlcv(&self, row: &Value) -> OHLCV {
        OHLCV {
            timestamp: row.get(0).and_then(Value::as_i64),
            open: num(row.get(1)),
            high: num(row.get(2)),
            low: num(row.get(3)),
            close: num(row.get(4)),
            volume: num(row.get(5)),
        }
    }

    pub fn parse_trade(&self, raw: &Value) -> Trade {
        let ts = raw["timestamp"].as_i64().or_else(|| raw["time"].as_i64());
        Trade {
            id: raw["id"]
                .as_i64()
                .or_else(|| raw["tradeId"].as_i64())
                .map(|v| v.to_string()),
            info: raw.clone(),
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            symbol: raw["symbol"].as_str().map(|s| self.market_symbol(s)),
            side: raw["side"].as_str().map(|s| s.to_lowercase()),
            price: num(raw.get("price")),
            amount: num(raw.get("quantity")).or_else(|| num(raw.get("amount"))),
            ..Trade::default()
        }
    }

    pub fn parse_order_book(&self, raw: &Value, id: &str) -> OrderBook {
        OrderBook {
            symbol: self.market_symbol(id),
            bids: raw["bids"]
                .as_array()
                .map(|a| a.iter().map(parse_level).collect())
                .unwrap_or_default(),
            asks: raw["asks"]
                .as_array()
                .map(|a| a.iter().map(parse_level).collect())
                .unwrap_or_default(),
            nonce: raw["timestamp"].as_i64(),
            info: raw.clone(),
            ..OrderBook::default()
        }
    }

    pub fn parse_order(&self, raw: &Value) -> Order {
        let status = raw["status"].as_str().map(|s| match s {
            "NEW" | "PARTIALLY_FILLED" => "open",
            "FILLED" => "closed",
            "CANCELED" | "CANCELLED" => "canceled",
            "REJECTED" => "rejected",
            "EXPIRED" => "expired",
            other => other,
        });
        let qty = num(raw.get("quantity")).or_else(|| num(raw.get("origQty")));
        let filled = num(raw.get("filledQuantity")).or_else(|| num(raw.get("executedQty")));
        let remaining = match (qty, filled) {
            (Some(q), Some(f)) => Some(q - f),
            _ => None,
        };
        Order {
            id: raw["orderId"]
                .as_str()
                .or_else(|| raw["id"].as_str())
                .map(String::from),
            client_order_id: raw["clientOrderId"].as_str().map(String::from),
            timestamp: raw["createTime"]
                .as_i64()
                .or_else(|| raw["timestamp"].as_i64()),
            datetime: raw["createTime"]
                .as_i64()
                .or_else(|| raw["timestamp"].as_i64())
                .and_then(iso8601),
            status: status.map(String::from),
            symbol: raw["symbol"].as_str().map(|s| self.market_symbol(s)),
            order_type: raw["type"].as_str().map(|s| s.to_lowercase()),
            side: raw["side"].as_str().map(|s| s.to_lowercase()),
            price: num(raw.get("price")).or_else(|| num(raw.get("avgPrice"))),
            average: num(raw.get("avgPrice")),
            amount: qty,
            filled,
            remaining,
            info: raw.clone(),
            ..Order::default()
        }
    }
}

impl Exchange for Hashkey {
    fn id(&self) -> &'static str {
        ID
    }
    fn config(&self) -> &Config {
        &self.config
    }

    async fn fetch_time(&self) -> Result<i64> {
        Ok(now_ms())
    }

    async fn fetch_markets(&self) -> Result<Vec<Market>> {
        self.core.load_markets(|| self.fetch_markets_raw()).await?;
        Ok(self.core.markets_snapshot().into_values().collect())
    }

    async fn fetch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        let resp = self
            .public_request(
                &format!("/api/v1/tickers/{}", self.symbol_id(symbol)),
                &Params::new(),
            )
            .await?;
        Ok(self.parse_ticker(&resp))
    }

    async fn fetch_tickers(&self, symbols: Option<&[&str]>, _params: Params) -> Result<Tickers> {
        let resp = self
            .public_request("/api/v1/tickers", &Params::new())
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        let mut out = Tickers::new();
        for raw in arr {
            let t = self.parse_ticker(&raw);
            if let Some(wanted) = symbols {
                if !wanted.contains(&t.symbol.as_str()) {
                    continue;
                }
            }
            out.insert(t.symbol.clone(), t);
        }
        Ok(out)
    }

    async fn fetch_order_book(
        &self,
        symbol: &str,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        let mut p = Params::new();
        if let Some(l) = limit {
            p.insert("limit".into(), json!(l));
        }
        let resp = self
            .public_request(&format!("/api/v1/depth/{}", self.symbol_id(symbol)), &p)
            .await?;
        Ok(self.parse_order_book(&resp, &self.symbol_id(symbol)))
    }

    async fn fetch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        let mut p = Params::new();
        if let Some(l) = limit {
            p.insert("limit".into(), json!(l));
        }
        let resp = self
            .public_request(&format!("/api/v1/trades/{}", self.symbol_id(symbol)), &p)
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        Ok(arr.iter().map(|t| self.parse_trade(t)).collect())
    }

    async fn fetch_ohlcv(
        &self,
        symbol: &str,
        timeframe: &str,
        _since: Option<i64>,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<OHLCV>> {
        let mut p = Params::new();
        p.insert("period".into(), json!(timeframe));
        if let Some(l) = limit {
            p.insert("size".into(), json!(l));
        }
        let resp = self
            .public_request(&format!("/api/v1/klines/{}", self.symbol_id(symbol)), &p)
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        Ok(arr.iter().map(|r| self.parse_ohlcv(r)).collect())
    }

    async fn fetch_balance(&self, _params: Params) -> Result<Balances> {
        let resp = self
            .private_request("GET", "/api/v1/account/balances", &Params::new(), None)
            .await?;
        let mut accounts = HashMap::new();
        let arr = resp
            .as_array()
            .or_else(|| resp.get("list").and_then(Value::as_array))
            .cloned()
            .unwrap_or_default();
        for b in arr {
            if let Some(asset) = b["asset"].as_str() {
                let free = num(b.get("free")).or_else(|| num(b.get("available")));
                let used = num(b.get("locked")).or_else(|| num(b.get("frozen")));
                let total = match (free, used) {
                    (Some(f), Some(u)) => Some(f + u),
                    (Some(f), None) => Some(f),
                    (None, Some(u)) => Some(u),
                    _ => None,
                };
                accounts.insert(
                    asset.to_string(),
                    Balance {
                        free,
                        used,
                        total,
                        ..Balance::default()
                    },
                );
            }
        }
        Ok(Balances {
            info: resp.clone(),
            accounts,
            ..Balances::default()
        })
    }

    async fn create_order(
        &self,
        symbol: &str,
        order_type: &str,
        side: &str,
        amount: &str,
        price: Option<&str>,
        _params: Params,
    ) -> Result<Order> {
        let mut body = serde_json::Map::new();
        body.insert("symbol".into(), json!(self.symbol_id(symbol)));
        body.insert("side".into(), json!(side.to_uppercase()));
        body.insert("type".into(), json!(order_type.to_uppercase()));
        body.insert("quantity".into(), json!(amount));
        if order_type.eq_ignore_ascii_case("limit") {
            body.insert(
                "price".into(),
                json!(price.ok_or_else(|| Error::new(
                    ErrorKind::BadRequest,
                    "limit order requires price"
                ))?),
            );
            body.insert("timeInForce".into(), json!("GTC"));
        }
        let resp = self
            .private_request(
                "POST",
                "/api/v1/orders",
                &Params::new(),
                Some(Value::Object(body)),
            )
            .await?;
        Ok(self.parse_order(&resp))
    }

    async fn cancel_order(&self, id: &str, symbol: &str, _params: Params) -> Result<Order> {
        let mut p = Params::new();
        p.insert("symbol".into(), json!(self.symbol_id(symbol)));
        let resp = self
            .private_request("DELETE", &format!("/api/v1/orders/{}", id), &p, None)
            .await?;
        let mut order = self.parse_order(&resp);
        if order.id.is_none() {
            order.id = Some(id.to_string());
        }
        Ok(order)
    }

    async fn fetch_order(&self, id: &str, symbol: &str, _params: Params) -> Result<Order> {
        let mut p = Params::new();
        p.insert("symbol".into(), json!(self.symbol_id(symbol)));
        p.insert("orderId".into(), json!(id));
        let resp = self
            .private_request("GET", "/api/v1/orders", &p, None)
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        let raw = arr
            .into_iter()
            .find(|o| o["orderId"].as_str() == Some(id))
            .unwrap_or(resp);
        Ok(self.parse_order(&raw))
    }

    async fn fetch_open_orders(
        &self,
        symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Order>> {
        let mut p = Params::new();
        if let Some(s) = symbol {
            p.insert("symbol".into(), json!(self.symbol_id(s)));
        }
        let resp = self
            .private_request("GET", "/api/v1/openOrders", &p, None)
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        Ok(arr.iter().map(|o| self.parse_order(o)).collect())
    }

    async fn fetch_orders(
        &self,
        symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Order>> {
        let mut p = Params::new();
        if let Some(s) = symbol {
            p.insert("symbol".into(), json!(self.symbol_id(s)));
        }
        let resp = self
            .private_request("GET", "/api/v1/orders", &p, None)
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        Ok(arr.iter().map(|o| self.parse_order(o)).collect())
    }

    async fn fetch_my_trades(
        &self,
        symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        let mut p = Params::new();
        if let Some(s) = symbol {
            p.insert("symbol".into(), json!(self.symbol_id(s)));
        }
        let resp = self
            .private_request("GET", "/api/v1/myTrades", &p, None)
            .await?;
        let arr = resp.as_array().cloned().unwrap_or_default();
        Ok(arr.iter().map(|t| self.parse_trade(t)).collect())
    }
}

fn num(v: Option<&Value>) -> Option<rust_decimal::Decimal> {
    v.and_then(|x| match x {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => rust_decimal::Decimal::from_str(&n.to_string()).ok(),
        _ => None,
    })
}

fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn sign_hmac_sha256(data: &str, secret: &str) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
