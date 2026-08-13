//! myokx 适配器(ADR-0017,由 generated promote 为 curated)。
//!
//! 手写完整交易 API。OKX v5 签名:`base64(HMAC-SHA256(secret,
//! timestamp+method+requestPath+body))` + `OK-ACCESS-KEY/SIGN/TIMESTAMP/
//! PASSPHRASE` 头。endpoint 路径对齐 okx(`/api/v5/...`);base URL 以官方
//! 文档为准(此处用 OKX 主站,待核对 myokx 实体端点)。

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Exchange, Params};
use crate::httpcore::{
    HttpCore, dec, iso8601, iso8601_now, parse_level, query_string, value_decimal,
};
use crate::types::{
    Balance, Balances, Market, MarketType, Markets, OHLCV, Order, OrderBook, Position, Precision,
    Ticker, Tickers, Trade,
};

pub const ID: &str = "myokx";
const BASE_URL: &str = "https://www.okx.com/api/v5";
const RATE_LIMIT_MS: u64 = 110;

pub struct MyOkx {
    config: Config,
    core: HttpCore,
}

impl MyOkx {
    pub const IMPLEMENTED: &'static [&'static str] = &[
        "fetch_markets",
        "fetch_ticker",
        "fetch_tickers",
        "fetch_order_book",
        "fetch_trades",
        "fetch_ohlcv",
        "fetch_balance",
        "fetch_positions",
        "fetch_orders",
        "fetch_open_orders",
        "create_order",
        "cancel_order",
    ];

    pub fn new(config: Config) -> Result<Self> {
        let core = HttpCore::new(&config, BASE_URL, RATE_LIMIT_MS, "myokx")?;
        Ok(Self { config, core })
    }

    fn symbol_id(&self, symbol: &str) -> String {
        symbol.replace('/', "-")
    }

    fn sign_str(&self, timestamp: &str, method: &str, path: &str, body: &str) -> Result<String> {
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "myokx secret required"))?;
        let auth = format!("{timestamp}{method}{path}{body}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| Error::new(ErrorKind::Authentication, format!("hmac key: {e}")))?;
        mac.update(auth.as_bytes());
        Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
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
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "myokx api_key required"))?;
        let passphrase = self
            .config
            .password
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "myokx password required"))?;
        let timestamp = iso8601_now();
        let (request_path, body_json) = if method == "GET" {
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
        let signature = self.sign_str(&timestamp, method, &request_path, &body_str)?;
        let mut headers = HeaderMap::new();
        headers.insert("OK-ACCESS-KEY", HeaderValue::from_str(api_key).unwrap());
        headers.insert("OK-ACCESS-SIGN", HeaderValue::from_str(&signature).unwrap());
        headers.insert(
            "OK-ACCESS-TIMESTAMP",
            HeaderValue::from_str(&timestamp).unwrap(),
        );
        headers.insert(
            "OK-ACCESS-PASSPHRASE",
            HeaderValue::from_str(passphrase).unwrap(),
        );
        self.core
            .request(method, &request_path, &headers, body_json)
            .await
    }

    async fn fetch_markets_raw(&self) -> Result<Markets> {
        let mut p = Params::new();
        p.insert("instType".into(), json!("SPOT"));
        let resp = self.core.public_get("/public/instruments", &p).await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "instruments not array"))?;
        let mut map = Markets::new();
        for raw in arr {
            let m = self.parse_market(raw);
            map.insert(m.symbol.clone(), m);
        }
        Ok(map)
    }

    pub fn parse_market(&self, raw: &Value) -> Market {
        let inst_id = raw["instId"].as_str().unwrap_or_default();
        let (base, quote) = match inst_id.split_once('-') {
            Some((b, q)) => (b, q),
            None => (inst_id, ""),
        };
        Market {
            id: inst_id.to_string(),
            symbol: format!("{base}/{quote}"),
            base: Some(base.to_string()),
            quote: Some(quote.to_string()),
            base_id: Some(base.to_string()),
            quote_id: Some(quote.to_string()),
            active: Some(raw["state"].as_str() == Some("live")),
            market_type: Some(MarketType::Spot),
            spot: Some(true),
            taker: Some("0.0015".parse().unwrap_or_default()),
            maker: Some("0.0010".parse().unwrap_or_default()),
            precision: Precision {
                amount: raw["lotSz"].as_str().and_then(|s| s.parse().ok()),
                price: raw["tickSz"].as_str().and_then(|s| s.parse().ok()),
                cost: None,
            },
            limits: crate::types::Limits::default(),
            info: raw.clone(),
            ..Market::default()
        }
    }

    pub fn parse_ticker(&self, raw: &Value) -> Ticker {
        let ts = raw["ts"].as_str().and_then(|s| s.parse::<i64>().ok());
        Ticker {
            symbol: raw["instId"].as_str().unwrap_or_default().replace('-', "/"),
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            open: dec(raw.get("open24h")),
            high: dec(raw.get("high24h")),
            low: dec(raw.get("low24h")),
            close: dec(raw.get("last")),
            last: dec(raw.get("last")),
            bid: dec(raw.get("bidPx")),
            ask: dec(raw.get("askPx")),
            base_volume: dec(raw.get("vol24h")),
            quote_volume: dec(raw.get("volCcy24h")),
            info: raw.clone(),
            ..Ticker::default()
        }
    }

    pub fn parse_order_book(&self, raw: &Value, symbol: &str) -> OrderBook {
        let data = raw
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .cloned();
        let book = data.as_ref().unwrap_or(raw);
        let mut bids: Vec<_> = book
            .get("bids")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(parse_level).collect())
            .unwrap_or_default();
        let mut asks: Vec<_> = book
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
            timestamp: book["ts"].as_str().and_then(|s| s.parse::<i64>().ok()),
            nonce: None,
            info: book.clone(),
            ..OrderBook::default()
        }
    }

    pub fn parse_trade(&self, raw: &Value) -> Trade {
        let ts = raw["ts"].as_str().and_then(|s| s.parse::<i64>().ok());
        Trade {
            id: raw["tradeId"].as_str().map(String::from),
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            symbol: raw["instId"].as_str().map(|s| s.replace('-', "/")),
            side: raw["side"].as_str().map(|s| s.to_lowercase()),
            price: dec(raw.get("px")),
            amount: dec(raw.get("sz")),
            info: raw.clone(),
            ..Trade::default()
        }
    }

    pub fn parse_ohlcv(&self, row: &Value) -> OHLCV {
        let arr = row.as_array();
        OHLCV {
            timestamp: arr
                .and_then(|a| a.first())
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<i64>().ok()),
            open: arr.and_then(|a| a.get(1)).and_then(value_decimal),
            high: arr.and_then(|a| a.get(2)).and_then(value_decimal),
            low: arr.and_then(|a| a.get(3)).and_then(value_decimal),
            close: arr.and_then(|a| a.get(4)).and_then(value_decimal),
            volume: arr.and_then(|a| a.get(5)).and_then(value_decimal),
        }
    }

    pub fn parse_order(&self, raw: &Value) -> Order {
        let status = raw["state"].as_str().map(|s| match s {
            "live" | "partially_filled" => "open",
            "filled" => "closed",
            "canceled" => "canceled",
            other => other,
        });
        let ts = raw["cTime"].as_str().and_then(|s| s.parse::<i64>().ok());
        let sz = dec(raw.get("sz"));
        let fill = dec(raw.get("accFillSz"));
        Order {
            id: raw["ordId"].as_str().map(String::from),
            client_order_id: raw["clOrdId"].as_str().map(String::from),
            timestamp: ts,
            datetime: ts.and_then(iso8601),
            status: status.map(String::from),
            symbol: raw["instId"].as_str().map(|s| s.replace('-', "/")),
            order_type: raw["ordType"].as_str().map(|s| s.to_lowercase()),
            side: raw["side"].as_str().map(|s| s.to_lowercase()),
            price: dec(raw.get("px")),
            average: dec(raw.get("avgPx")),
            amount: sz,
            filled: fill,
            remaining: match (sz, fill) {
                (Some(s), Some(f)) => Some(s - f),
                _ => None,
            },
            info: raw.clone(),
            ..Order::default()
        }
    }

    pub fn parse_position(&self, raw: &Value) -> Position {
        Position {
            symbol: raw["instId"].as_str().map(|s| s.replace('-', "/")),
            id: raw["posId"].as_str().map(String::from),
            contracts: dec(raw.get("pos")),
            entry_price: dec(raw.get("avgPx")),
            unrealized_pnl: dec(raw.get("upl")),
            notional: dec(raw.get("notionalUsd")),
            info: raw.clone(),
            ..Position::default()
        }
    }
}

impl Exchange for MyOkx {
    fn id(&self) -> &'static str {
        ID
    }
    fn config(&self) -> &Config {
        &self.config
    }

    async fn fetch_markets(&self) -> Result<Vec<Market>> {
        self.core.load_markets(|| self.fetch_markets_raw()).await?;
        Ok(self.core.markets_snapshot().into_values().collect())
    }

    async fn fetch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        let mut p = Params::new();
        p.insert("instId".into(), json!(self.symbol_id(symbol)));
        let resp = self.core.public_get("/market/ticker", &p).await?;
        let raw = resp
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "ticker empty"))?;
        Ok(self.parse_ticker(raw))
    }

    async fn fetch_tickers(&self, _symbols: Option<&[&str]>, _params: Params) -> Result<Tickers> {
        let mut p = Params::new();
        p.insert("instType".into(), json!("SPOT"));
        let resp = self.core.public_get("/market/tickers", &p).await?;
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

    async fn fetch_order_book(
        &self,
        symbol: &str,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        let mut p = Params::new();
        p.insert("instId".into(), json!(self.symbol_id(symbol)));
        p.insert("sz".into(), json!(limit.unwrap_or(100).min(400)));
        let resp = self.core.public_get("/market/books", &p).await?;
        Ok(self.parse_order_book(&resp, &symbol.replace('-', "/")))
    }

    async fn fetch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        let mut p = Params::new();
        p.insert("instId".into(), json!(self.symbol_id(symbol)));
        p.insert("limit".into(), json!(limit.unwrap_or(100).min(500)));
        let resp = self.core.public_get("/market/trades", &p).await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
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
        let bar = match timeframe {
            "1m" => "1m",
            "5m" => "5m",
            "15m" => "15m",
            "1h" => "1H",
            "4h" => "4H",
            "1d" => "1D",
            other => {
                return Err(Error::new(
                    ErrorKind::BadRequest,
                    format!("unsupported timeframe {other}"),
                ));
            }
        };
        let mut p = Params::new();
        p.insert("instId".into(), json!(self.symbol_id(symbol)));
        p.insert("bar".into(), json!(bar));
        p.insert("limit".into(), json!(limit.unwrap_or(100).min(300)));
        let resp = self.core.public_get("/market/candles", &p).await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().map(|r| self.parse_ohlcv(r)).collect())
    }

    async fn fetch_balance(&self, _params: Params) -> Result<Balances> {
        let resp = self
            .private_request("GET", "/account/balance", &Params::new(), None)
            .await?;
        let mut out = Balances {
            info: resp.clone(),
            ..Balances::default()
        };
        if let Some(data) = resp
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
        {
            if let Some(details) = data.get("details").and_then(Value::as_array) {
                for d in details {
                    if let Some(code) = d["ccy"].as_str() {
                        let cash = dec(d.get("cashBal"));
                        let avail = dec(d.get("availBal"));
                        out.accounts.insert(
                            code.to_string(),
                            Balance {
                                free: avail.or(cash),
                                total: cash,
                                ..Balance::default()
                            },
                        );
                    }
                }
            }
        }
        Ok(out)
    }

    async fn fetch_positions(
        &self,
        _symbols: Option<&[&str]>,
        _params: Params,
    ) -> Result<Vec<Position>> {
        let resp = self
            .private_request("GET", "/account/positions", &Params::new(), None)
            .await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr
            .iter()
            .filter(|p| p["pos"].as_str().map(|s| s != "0").unwrap_or(false))
            .map(|p| self.parse_position(p))
            .collect())
    }

    async fn fetch_orders(
        &self,
        symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Order>> {
        let mut p = Params::new();
        p.insert("instType".into(), json!("SPOT"));
        if let Some(s) = symbol {
            p.insert("instId".into(), json!(self.symbol_id(s)));
        }
        let resp = self
            .private_request("GET", "/trade/orders-history", &p, None)
            .await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().map(|o| self.parse_order(o)).collect())
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
            p.insert("instId".into(), json!(self.symbol_id(s)));
        }
        let resp = self
            .private_request("GET", "/trade/orders-pending", &p, None)
            .await?;
        let arr = resp
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().map(|o| self.parse_order(o)).collect())
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
        let ord_type = match order_type {
            "limit" => "limit",
            "market" => "market",
            other => {
                return Err(Error::new(
                    ErrorKind::BadRequest,
                    format!("unsupported ordType {other}"),
                ));
            }
        };
        let mut body = serde_json::Map::new();
        body.insert("instId".into(), json!(self.symbol_id(symbol)));
        body.insert("tdMode".into(), json!("cash"));
        body.insert("side".into(), json!(side));
        body.insert("ordType".into(), json!(ord_type));
        body.insert("sz".into(), json!(amount));
        if let Some(px) = price {
            body.insert("px".into(), json!(px));
        }
        let resp = self
            .private_request(
                "POST",
                "/trade/order",
                &Params::new(),
                Some(Value::Object(body)),
            )
            .await?;
        let data = resp
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "order empty"))?;
        let mut order = self.parse_order(&data);
        if order.id.is_none() {
            order.id = data["ordId"].as_str().map(String::from);
        }
        Ok(order)
    }

    async fn cancel_order(&self, id: &str, symbol: &str, _params: Params) -> Result<Order> {
        let body = json!({ "instId": self.symbol_id(symbol), "ordId": id });
        let resp = self
            .private_request("POST", "/trade/cancel-order", &Params::new(), Some(body))
            .await?;
        let data = resp
            .get("data")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "cancel empty"))?;
        Ok(Order {
            id: data["ordId"]
                .as_str()
                .map(String::from)
                .or_else(|| Some(id.to_string())),
            status: Some("canceled".into()),
            info: data,
            ..Order::default()
        })
    }
}
