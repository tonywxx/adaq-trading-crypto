//! binance 现货适配器 —— 首个参考适配器(ADR-0005)。
//!
//! 实现统一接口的公共方法面(行情/市场数据)与私有签名管道
//! (以 `fetch_balance` 为参考实现)。未实现的方法沿用 trait 默认
//! `NotSupported`。解析逻辑参考 ccxt 的 binance 实现(见仓库 `NOTICE`)。
//!
//! 仅 `binance` feature 下编译。

use std::collections::HashMap;
use std::str::FromStr;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};

use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Exchange, Params};
use crate::httpcore::{HttpCore, now_ms, parse_level, query_string};
use crate::types::{
    Balance, Balances, Market, MarketType, Markets, OHLCV, OrderBook, Precision, Ticker, Tickers,
    Trade,
};

pub const ID: &str = "binance";
const BASE_URL: &str = "https://api.binance.com/api/v3";

/// 常见计价币(用于无缓存时推导统一 symbol)。
const COMMON_QUOTES: &[&str] = &[
    "USDT", "USDC", "FDUSD", "TUSD", "BUSD", "DAI", "EUR", "USD", "BTC", "ETH", "BNB", "PAX",
    "USDS",
];

/// binance 现货适配器。
pub struct Binance {
    config: Config,
    core: HttpCore,
}

impl Binance {
    /// 已实现(非 NotSupported)的 REST 方法面(契约声明,M2d)。
    pub const IMPLEMENTED: &'static [&'static str] = &[
        "fetch_time",
        "fetch_markets",
        "fetch_currencies",
        "fetch_ticker",
        "fetch_tickers",
        "fetch_bids_asks",
        "fetch_ohlcv",
        "fetch_order_book",
        "fetch_trades",
        "fetch_balance",
    ];

    /// 构造适配器(默认启用限速 50ms/次,对齐 binance rateLimit)。
    pub fn new(config: Config) -> Result<Self> {
        let core = HttpCore::new(&config, BASE_URL, 50, "binance")?;
        Ok(Self { config, core })
    }

    /// 加载并缓存市集(按 symbol 索引;缓存由核心 `HttpCore::load_markets` 负责)。
    pub async fn load_markets(&self) -> Result<Markets> {
        self.core.load_markets(|| self.fetch_markets_raw()).await?;
        Ok(self.core.markets_snapshot())
    }

    /// 拉取并解析市集(字段映射接缝;缓存由核心负责)。
    async fn fetch_markets_raw(&self) -> Result<Markets> {
        let markets = self.fetch_markets().await?;
        let map: Markets = markets.into_iter().map(|m| (m.symbol.clone(), m)).collect();
        Ok(map)
    }

    /// binance 符号(id,如 `BTCUSDT`)→ 统一 symbol(`BTC/USDT`)。
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
        self.core.public_get(path, params).await
    }

    /// 私有签名请求(binance:HMAC-SHA256,`X-MBX-APIKEY` 头)。
    async fn private_request(&self, method: &str, path: &str, params: &Params) -> Result<Value> {
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "binance api_key required"))?;
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "binance secret required"))?;
        let mut p = params.clone();
        p.insert("timestamp".into(), json!(now_ms()));
        p.insert("recvWindow".into(), json!(5000));
        let qs = query_string(&p);
        let signature = sign_hmac_sha256(&qs, secret);
        let url = format!("{BASE_URL}{path}?{qs}&signature={signature}");
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(api_key).map_err(|e| {
                Error::new(
                    ErrorKind::BadRequest,
                    format!("invalid api key header: {e}"),
                )
            })?,
        );
        self.core.request_url(method, &url, &headers, None).await
    }

    // ---------- 解析 ----------

    pub fn parse_market(&self, raw: &Value) -> Market {
        let id = raw["symbol"].as_str().unwrap_or_default().to_string();
        let status = raw["status"].as_str().unwrap_or_default();
        let mut precision = Precision::default();
        let mut limits = crate::types::Limits::default();
        if let Some(filters) = raw["filters"].as_array() {
            for f in filters {
                match f["filterType"].as_str() {
                    Some("PRICE_FILTER") => {
                        precision.price = num(f.get("tickSize"));
                        limits.price = Some(crate::types::Limit {
                            min: num(f.get("minPrice")),
                            max: num(f.get("maxPrice")),
                        });
                    }
                    Some("LOT_SIZE") => {
                        precision.amount = num(f.get("stepSize"));
                        limits.amount = Some(crate::types::Limit {
                            min: num(f.get("minQty")),
                            max: num(f.get("maxQty")),
                        });
                    }
                    Some("MIN_NOTIONAL") => {
                        limits.cost = Some(crate::types::Limit {
                            min: num(f.get("minNotional")),
                            max: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        Market {
            id,
            symbol: self.market_symbol(raw["symbol"].as_str().unwrap_or_default()),
            base: raw["baseAsset"].as_str().map(String::from),
            quote: raw["quoteAsset"].as_str().map(String::from),
            base_id: raw["baseAsset"].as_str().map(String::from),
            quote_id: raw["quoteAsset"].as_str().map(String::from),
            active: Some(status == "TRADING"),
            market_type: Some(MarketType::Spot),
            spot: Some(true),
            precision,
            limits,
            info: raw.clone(),
            ..Market::default()
        }
    }

    pub fn parse_ticker(&self, raw: &Value) -> Ticker {
        let timestamp = raw["closeTime"].as_i64();
        let open_price = num(raw.get("openPrice"));
        let close_price = num(raw.get("lastPrice"));
        // ccxt 语义:average = (open + close) / 2(Precise 18 位)
        let average = match (open_price, close_price) {
            (Some(o), Some(c)) => {
                let sum = crate::precise::string_add(&o.to_string(), &c.to_string());
                crate::precise::string_div(&sum, "2").parse().ok()
            }
            _ => None,
        };
        Ticker {
            symbol: self.market_symbol(raw["symbol"].as_str().unwrap_or_default()),
            timestamp,
            datetime: timestamp.and_then(iso8601),
            high: num(raw.get("highPrice")),
            low: num(raw.get("lowPrice")),
            bid: num(raw.get("bidPrice")),
            ask: num(raw.get("askPrice")),
            bid_volume: num(raw.get("bidQty")),
            ask_volume: num(raw.get("askQty")),
            vwap: num(raw.get("weightedAvgPrice")),
            open: open_price,
            close: close_price,
            last: close_price,
            previous_close: num(raw.get("prevClosePrice")),
            change: num(raw.get("priceChange")),
            percentage: num(raw.get("priceChangePercent")),
            average,
            quote_volume: num(raw.get("quoteVolume")),
            base_volume: num(raw.get("volume")),
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
        let timestamp = raw["time"].as_i64();
        let buyer_maker = raw["isBuyerMaker"].as_bool().unwrap_or(false);
        Trade {
            id: raw["id"].as_i64().map(|v| v.to_string()),
            info: raw.clone(),
            timestamp,
            datetime: timestamp.and_then(iso8601),
            symbol: Some(self.market_symbol(raw["symbol"].as_str().unwrap_or_default())),
            side: Some(if buyer_maker { "sell" } else { "buy" }.to_string()),
            taker_or_maker: Some("taker".to_string()),
            price: num(raw.get("price")),
            amount: num(raw.get("qty")),
            cost: num(raw.get("quoteQty")),
            ..Trade::default()
        }
    }

    /// 解析订单簿(id 为 binance 符号,如 `BTCUSDT`)。
    pub fn parse_order_book(&self, raw: &Value, id: &str) -> OrderBook {
        let bids = raw["bids"]
            .as_array()
            .map(|a| a.iter().map(parse_level).collect())
            .unwrap_or_default();
        let asks = raw["asks"]
            .as_array()
            .map(|a| a.iter().map(parse_level).collect())
            .unwrap_or_default();
        OrderBook {
            symbol: self.market_symbol(id),
            bids,
            asks,
            nonce: raw["lastUpdateId"].as_i64(),
            info: raw.clone(),
            ..OrderBook::default()
        }
    }

    /// 获取用户数据流 listenKey(私有 WS 认证用),对应 `POST /api/v3/userDataStream`。
    ///
    /// 仅需 `X-MBX-APIKEY` 头、无需签名(与 binance 文档一致)。供实时适配器
    /// 委托认证(ADR-0015)。
    pub async fn fetch_listen_key(&self) -> Result<String> {
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "binance api_key required"))?;
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-MBX-APIKEY",
            HeaderValue::from_str(api_key).map_err(|e| {
                Error::new(
                    ErrorKind::BadRequest,
                    format!("invalid api key header: {e}"),
                )
            })?,
        );
        let resp = self
            .core
            .request_url(
                "POST",
                &format!("{BASE_URL}/userDataStream"),
                &headers,
                None,
            )
            .await?;
        resp["listenKey"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "no listenKey in response"))
    }
}

impl Exchange for Binance {
    fn id(&self) -> &'static str {
        ID
    }

    fn config(&self) -> &Config {
        &self.config
    }

    async fn fetch_time(&self) -> Result<i64> {
        let resp = self.public_request("/time", &Params::new()).await?;
        resp["serverTime"]
            .as_i64()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "missing serverTime"))
    }

    async fn fetch_markets(&self) -> Result<Vec<Market>> {
        let resp = self.public_request("/exchangeInfo", &Params::new()).await?;
        let symbols = resp["symbols"]
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "missing symbols"))?;
        Ok(symbols.iter().map(|s| self.parse_market(s)).collect())
    }

    async fn fetch_currencies(&self) -> Result<crate::types::Currencies> {
        // binance 现货无公开币种端点(ccxt 同样返回空表)
        Ok(HashMap::new())
    }

    async fn fetch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        let id = self.symbol_id(symbol).await?;
        let mut p = Params::new();
        p.insert("symbol".into(), json!(id));
        let resp = self.public_request("/ticker/24hr", &p).await?;
        Ok(self.parse_ticker(&resp))
    }

    async fn fetch_tickers(&self, symbols: Option<&[&str]>, _params: Params) -> Result<Tickers> {
        let resp = self.public_request("/ticker/24hr", &Params::new()).await?;
        let arr = resp
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "tickers not array"))?;
        let mut out = Tickers::new();
        for raw in arr {
            let t = self.parse_ticker(raw);
            if let Some(wanted) = symbols {
                if !wanted.contains(&t.symbol.as_str()) {
                    continue;
                }
            }
            out.insert(t.symbol.clone(), t);
        }
        Ok(out)
    }

    async fn fetch_bids_asks(&self, symbols: Option<&[&str]>, _params: Params) -> Result<Tickers> {
        let resp = self
            .public_request("/ticker/bookTicker", &Params::new())
            .await?;
        let arr = resp
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "bookTicker not array"))?;
        let mut out = Tickers::new();
        for raw in arr {
            let symbol = self.market_symbol(raw["symbol"].as_str().unwrap_or_default());
            if let Some(wanted) = symbols {
                if !wanted.contains(&symbol.as_str()) {
                    continue;
                }
            }
            out.insert(
                symbol.clone(),
                Ticker {
                    symbol,
                    bid: num(raw.get("bidPrice")),
                    bid_volume: num(raw.get("bidQty")),
                    ask: num(raw.get("askPrice")),
                    ask_volume: num(raw.get("askQty")),
                    info: raw.clone(),
                    ..Ticker::default()
                },
            );
        }
        Ok(out)
    }

    async fn fetch_ohlcv(
        &self,
        symbol: &str,
        timeframe: &str,
        since: Option<i64>,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<OHLCV>> {
        let id = self.symbol_id(symbol).await?;
        let mut p = Params::new();
        p.insert("symbol".into(), json!(id));
        p.insert("interval".into(), json!(timeframe));
        if let Some(s) = since {
            p.insert("startTime".into(), json!(s));
        }
        if let Some(l) = limit {
            p.insert("limit".into(), json!(l));
        }
        let resp = self.public_request("/klines", &p).await?;
        let arr = resp
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "klines not array"))?;
        Ok(arr.iter().map(|r| self.parse_ohlcv(r)).collect())
    }

    async fn fetch_order_book(
        &self,
        symbol: &str,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        let id = self.symbol_id(symbol).await?;
        let mut p = Params::new();
        p.insert("symbol".into(), json!(id));
        if let Some(l) = limit {
            p.insert("limit".into(), json!(l));
        }
        let resp = self.public_request("/depth", &p).await?;
        Ok(self.parse_order_book(&resp, &id))
    }

    async fn fetch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        let id = self.symbol_id(symbol).await?;
        let mut p = Params::new();
        p.insert("symbol".into(), json!(id));
        if let Some(l) = limit {
            p.insert("limit".into(), json!(l));
        }
        let resp = self.public_request("/trades", &p).await?;
        let arr = resp
            .as_array()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "trades not array"))?;
        Ok(arr.iter().map(|r| self.parse_trade(r)).collect())
    }

    async fn fetch_balance(&self, _params: Params) -> Result<Balances> {
        let resp = self
            .private_request("GET", "/account", &Params::new())
            .await?;
        let mut accounts = HashMap::new();
        if let Some(balances) = resp["balances"].as_array() {
            for b in balances {
                let asset = b["asset"].as_str().unwrap_or_default().to_string();
                let free = num(b.get("free"));
                let used = num(b.get("locked"));
                let total = match (free, used) {
                    (Some(f), Some(u)) => Some(f + u),
                    (Some(f), None) => Some(f),
                    (None, Some(u)) => Some(u),
                    _ => None,
                };
                accounts.insert(
                    asset,
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
}

// ---------- 工具 ----------

/// 统一 symbol → binance id(`BTC/USDT` → `BTCUSDT`),走缓存否则拼接。
impl Binance {
    async fn symbol_id(&self, symbol: &str) -> Result<String> {
        if symbol.contains('/') {
            let (base, quote) = symbol
                .split_once('/')
                .ok_or_else(|| Error::new(ErrorKind::BadSymbol, symbol))?;
            Ok(format!("{base}{quote}"))
        } else {
            Ok(symbol.to_string())
        }
    }
}

/// `"0.001"` 等字符串 → Decimal(缺失/null → None)。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_string_encodes() {
        let mut p = Params::new();
        p.insert("symbol".into(), json!("BTC/USDT"));
        p.insert("limit".into(), json!(5));
        let qs = query_string(&p);
        assert!(qs.starts_with("?"));
        assert!(qs.contains("symbol=BTC%2FUSDT"));
        assert!(qs.contains("limit=5"));
    }

    #[test]
    fn hmac_matches_known_vector() {
        // binance 文档示例:secret="NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j"
        let sig = sign_hmac_sha256(
            "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559",
            "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j",
        );
        // 期望值由 Python hmac 独立验证
        assert_eq!(
            sig,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }

    #[test]
    fn market_symbol_fallback() {
        let binance = Binance::new(Config::new()).unwrap();
        assert_eq!(binance.market_symbol("BTCUSDT"), "BTC/USDT");
        assert_eq!(binance.market_symbol("ETHBTC"), "ETH/BTC");
    }
}
