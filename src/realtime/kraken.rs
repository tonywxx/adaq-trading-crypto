//! kraken WebSocket 适配器(Phase C,ADR-0009)。
//!
//! 对齐 kraken WS 协议(消息为**数组**格式):
//! - 公开流 `wss://ws.kraken.com`,订阅帧
//!   `{"event":"subscribe","pair":["XBT/USDT"],"subscription":{"name":"ticker"|"book"|"trade"|"ohlc"}}`;
//! - 消息 `[channelId, data, channelName, pair]`:ticker 的 `a[0]/b[0]/c[0]/h[0]/l[0]/o[0]/v[0]/p[0]`;
//!   book 快照 `{as, bs}` 与增量 `{a, b}`(`[price, vol, ts]`,vol=0 删除档位);
//!   trade 数组 `[[price, vol, ts, side, ...], ...]`;ohlc 数组 `[ts,o,h,l,c,vwap,vol,count]`;
//! - 私密流 `wss://ws-auth.kraken.com` 需 REST 取 token:
//!   POST /0/private/GetWebSocketsToken(复用 kraken 签名),订阅带 `"token"`;
//! - watch_positions 保持 NotSupported(kraken 无持仓 WS 频道)。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use tokio::sync::watch;

use crate::client::Client;
use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Params, Realtime};
use crate::realtime::orderbook::{OrderBookStore, PriceChange};
use crate::realtime::ws::{SubscriptionSet, WsSession};
use crate::types::{Balances, OHLCV, Order, OrderBook, Position, Ticker, Trade};

const WS_PUBLIC: &str = "wss://ws.kraken.com";
const WS_PRIVATE: &str = "wss://ws-auth.kraken.com";
const REST_BASE: &str = "https://api.kraken.com";

type TickerChannel = Arc<Mutex<HashMap<String, watch::Sender<Ticker>>>>;
type BookChannel = Arc<Mutex<HashMap<String, watch::Sender<OrderBook>>>>;
type BookStoreMap = Arc<Mutex<HashMap<String, OrderBookStore>>>;
type TradeChannel = Arc<Mutex<HashMap<String, watch::Sender<Vec<Trade>>>>>;
type OhlcvChannel = Arc<Mutex<HashMap<(String, String), watch::Sender<Vec<OHLCV>>>>>;

/// kraken WS 适配器。
pub struct KrakenWs {
    config: Config,
    client: Client,
    pub_connected: Mutex<Option<watch::Receiver<bool>>>,
    priv_connected: Mutex<Option<watch::Receiver<bool>>>,
    tickers: TickerChannel,
    books: BookChannel,
    book_stores: BookStoreMap,
    trades: TradeChannel,
    ohlcvs: OhlcvChannel,
    balances: Mutex<Option<watch::Sender<Balances>>>,
    orders: Mutex<Option<watch::Sender<Vec<Order>>>>,
    my_trades: Mutex<Option<watch::Sender<Vec<Trade>>>>,
    subs: Mutex<SubscriptionSet>,
    sub_tx: Mutex<Option<watch::Sender<Vec<String>>>>,
    priv_sub_tx: Mutex<Option<watch::Sender<Vec<String>>>>,
}

impl KrakenWs {
    pub fn new(config: Config) -> Result<Self> {
        let client = Client::new(
            config.timeout_ms,
            config.max_retries,
            config.proxy.as_deref(),
            config.rate_limit_ms.max(1000),
            config.enable_rate_limit,
        )?;
        Ok(Self {
            config,
            client,
            pub_connected: Mutex::new(None),
            priv_connected: Mutex::new(None),
            tickers: Arc::new(Mutex::new(HashMap::new())),
            books: Arc::new(Mutex::new(HashMap::new())),
            book_stores: Arc::new(Mutex::new(HashMap::new())),
            trades: Arc::new(Mutex::new(HashMap::new())),
            ohlcvs: Arc::new(Mutex::new(HashMap::new())),
            balances: Mutex::new(None),
            orders: Mutex::new(None),
            my_trades: Mutex::new(None),
            subs: Mutex::new(SubscriptionSet::new()),
            sub_tx: Mutex::new(None),
            priv_sub_tx: Mutex::new(None),
        })
    }

    fn symbol_id(&self, symbol: &str) -> String {
        // kraken 用 XBT 表示 BTC;pair 用斜杠格式(XBT/USDT)
        symbol.replace("BTC/", "XBT/")
    }

    fn ensure_public(&self) -> watch::Receiver<bool> {
        if let Some(rx) = self.pub_connected.lock().unwrap().clone() {
            return rx;
        }
        let tickers = self.tickers.clone();
        let books = self.books.clone();
        let book_stores = self.book_stores.clone();
        let trades = self.trades.clone();
        let ohlcvs = self.ohlcvs.clone();
        let headers = HeaderMap::new();
        let (sub_tx, sub_rx) = tokio::sync::watch::channel(Vec::new());
        let rx = WsSession::spawn(WS_PUBLIC.to_string(), headers, sub_rx, move |msg| {
            dispatch_public(msg, &tickers, &books, &book_stores, &trades, &ohlcvs)
        });
        *self.pub_connected.lock().unwrap() = Some(rx.clone());
        *self.sub_tx.lock().unwrap() = Some(sub_tx);
        rx
    }

    async fn subscribe(&self, name: &str, pair: &str) -> Result<()> {
        let key = format!("{name}:{pair}");
        let first = self.subs.lock().unwrap().register(&key);
        if !first {
            return Ok(());
        }
        let tx = self
            .sub_tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::new(ErrorKind::NetworkError, "ws not started"))?;
        let frame = json!({
            "event": "subscribe",
            "pair": [pair],
            "subscription": {"name": name}
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    async fn subscribe_private(&self, name: &str, token: &str) -> Result<()> {
        let key = format!("priv:{name}");
        let first = self.subs.lock().unwrap().register(&key);
        if !first {
            return Ok(());
        }
        let tx = self
            .priv_sub_tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::new(ErrorKind::NetworkError, "private ws not started"))?;
        let frame = json!({
            "event": "subscribe",
            "subscription": {"name": name, "token": token}
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    async fn fetch_ws_token(&self) -> Result<String> {
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "kraken api_key required"))?;
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "kraken secret required"))?;
        let nonce = now_ms().to_string();
        let body = format!("nonce={nonce}");
        // 签名:base64(HMAC-SHA512(b64dec(secret), path + sha256(nonce+body)))
        let mut hasher = Sha256::new();
        hasher.update(format!("{nonce}{body}").as_bytes());
        let sha = hasher.finalize();
        let mut mac = Hmac::<Sha512>::new_from_slice(
            &base64::engine::general_purpose::STANDARD
                .decode(secret)
                .map_err(|e| Error::new(ErrorKind::Authentication, format!("bad secret: {e}")))?,
        )
        .map_err(|e| Error::new(ErrorKind::Authentication, format!("hmac key: {e}")))?;
        mac.update(b"/0/private/GetWebSocketsToken");
        mac.update(&sha);
        let signature =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let mut headers = HeaderMap::new();
        headers.insert("API-Key", HeaderValue::from_str(api_key).unwrap());
        headers.insert("API-Sign", HeaderValue::from_str(&signature).unwrap());
        let resp = self
            .client
            .request(
                "POST",
                &format!("{REST_BASE}/0/private/GetWebSocketsToken"),
                &headers,
                Some(Value::String(body)),
            )
            .await?;
        resp["result"]["token"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "no ws token"))
    }

    async fn ensure_private(&self) -> Result<()> {
        if self.priv_connected.lock().unwrap().is_some() {
            return Ok(());
        }
        let token = self.fetch_ws_token().await?;
        let balances = self.balances.lock().unwrap().clone();
        let orders = self.orders.lock().unwrap().clone();
        let my_trades = self.my_trades.lock().unwrap().clone();
        let headers = HeaderMap::new();
        let (sub_tx, sub_rx) = tokio::sync::watch::channel(Vec::new());
        let token_clone = token.clone();
        let rx = WsSession::spawn(WS_PRIVATE.to_string(), headers, sub_rx, move |msg| {
            dispatch_private(msg, &balances, &orders, &my_trades, &token_clone)
        });
        *self.priv_connected.lock().unwrap() = Some(rx);
        *self.priv_sub_tx.lock().unwrap() = Some(sub_tx);
        Ok(())
    }
}

fn dispatch_public(
    msg: Value,
    tickers: &TickerChannel,
    books: &BookChannel,
    book_stores: &BookStoreMap,
    trades: &TradeChannel,
    ohlcvs: &OhlcvChannel,
) {
    // kraken 公开消息为数组:[channelId, data, channelName, pair]
    let arr = match msg.as_array() {
        Some(a) => a,
        None => return, // 系统消息(event/heartbeat)
    };
    let channel = arr.get(2).and_then(Value::as_str).unwrap_or_default();
    let pair = arr.get(3).and_then(Value::as_str).unwrap_or_default();
    let data = arr.get(1);
    match channel {
        "ticker" => {
            if let (Some(d), Some(tx)) = (data, tickers.lock().unwrap().get(pair).cloned()) {
                let _ = tx.send(parse_ticker(d, pair));
            }
        }
        channel if channel.starts_with("book") => {
            if let (Some(d), Some(tx)) = (data, books.lock().unwrap().get(pair).cloned()) {
                let store = book_stores.lock().unwrap().remove(pair);
                let is_snapshot = d.get("as").is_some() || d.get("bs").is_some();
                let mut store = store.unwrap_or_else(|| OrderBookStore::new(0));
                if is_snapshot {
                    store.reset(&collect_levels(d.get("bs")), &collect_levels(d.get("as")));
                } else {
                    // 增量:a/b 的 [price, vol, ts],vol=0 删除(复用 polymarket 语义)
                    store.apply_polymarket(&parse_changes(d.get("b")), "BUY");
                    store.apply_polymarket(&parse_changes(d.get("a")), "SELL");
                }
                let snap = store.snapshot(pair, None, None, d.clone());
                book_stores.lock().unwrap().insert(pair.to_string(), store);
                let _ = tx.send(snap);
            }
        }
        "trade" => {
            if let (Some(d), Some(tx)) = (data, trades.lock().unwrap().get(pair).cloned()) {
                let rows = d.as_array().cloned().unwrap_or_default();
                let parsed: Vec<Trade> = rows.iter().map(parse_trade).collect();
                let _ = tx.send(parsed);
            }
        }
        "ohlc" => {
            // ohlc 频道的 interval 在 data 内最后一个元素
            let tf = d_interval(data);
            if let (Some(d), Some(tx)) = (
                data,
                ohlcvs.lock().unwrap().get(&(pair.to_string(), tf)).cloned(),
            ) {
                let _ = tx.send(vec![parse_ohlcv(d)]);
            }
        }
        _ => {}
    }
}

fn dispatch_private(
    msg: Value,
    balances: &Option<watch::Sender<Balances>>,
    orders: &Option<watch::Sender<Vec<Order>>>,
    my_trades: &Option<watch::Sender<Vec<Trade>>>,
    token: &str,
) {
    if let Some(data) = msg.get("channelName").and_then(Value::as_str) {
        let payload = msg.get("data").unwrap_or(&Value::Null);
        match data {
            "balance" => {
                if let Some(tx) = balances.as_ref() {
                    let mut out = Balances {
                        info: msg.clone(),
                        ..Balances::default()
                    };
                    if let Some(obj) = payload.as_object() {
                        for (code, bal) in obj {
                            out.accounts.insert(
                                code.clone(),
                                crate::types::Balance {
                                    free: crate::realtime::ws::value_decimal(bal),
                                    total: crate::realtime::ws::value_decimal(bal),
                                    ..crate::types::Balance::default()
                                },
                            );
                        }
                    }
                    let _ = tx.send(out);
                }
            }
            "openOrders" => {
                if let Some(tx) = orders.as_ref() {
                    let mut parsed = Vec::new();
                    if let Some(open) = payload.get("open").and_then(Value::as_object) {
                        for (id, o) in open {
                            let mut order = parse_order(o);
                            if order.id.is_none() {
                                order.id = Some(id.clone());
                            }
                            parsed.push(order);
                        }
                    }
                    let _ = tx.send(parsed);
                }
            }
            "ownTrades" => {
                if let Some(tx) = my_trades.as_ref() {
                    let mut parsed = Vec::new();
                    if let Some(obj) = payload.as_object() {
                        for (id, t) in obj {
                            let mut trade = parse_own_trade(t);
                            if trade.id.is_none() {
                                trade.id = Some(id.clone());
                            }
                            parsed.push(trade);
                        }
                    }
                    let _ = tx.send(parsed);
                }
            }
            _ => {}
        }
    }
    // token 后续订阅需等待认证完成;系统订阅在 watch_* 方法内带 token 发送
    let _ = token;
}

fn parse_ticker(raw: &Value, pair: &str) -> Ticker {
    let ts = now_ms() as i64;
    Ticker {
        symbol: pair.to_string(),
        timestamp: Some(ts),
        datetime: Some(iso8601(ts).unwrap_or_default()),
        ask: raw["a"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        bid: raw["b"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        last: raw["c"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        close: raw["c"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        high: raw["h"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        low: raw["l"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        open: raw["o"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        vwap: raw["p"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        base_volume: raw["v"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        info: raw.clone(),
        ..Ticker::default()
    }
}

fn parse_trade(raw: &Value) -> Trade {
    let arr = raw.as_array();
    let ts = arr
        .and_then(|a| a.get(2))
        .and_then(|v| v.as_f64())
        .map(|f| f as i64 * 1000);
    Trade {
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        side: arr
            .and_then(|a| a.get(3))
            .and_then(Value::as_str)
            .map(str::to_string),
        price: arr
            .and_then(|a| a.first())
            .and_then(crate::realtime::ws::value_decimal),
        amount: arr
            .and_then(|a| a.get(1))
            .and_then(crate::realtime::ws::value_decimal),
        info: raw.clone(),
        ..Trade::default()
    }
}

fn parse_ohlcv(raw: &Value) -> OHLCV {
    let arr = raw.as_array();
    OHLCV {
        timestamp: arr
            .and_then(|a| a.first())
            .and_then(|v| v.as_f64())
            .map(|f| f as i64 * 1000),
        open: arr
            .and_then(|a| a.get(1))
            .and_then(crate::realtime::ws::value_decimal),
        high: arr
            .and_then(|a| a.get(2))
            .and_then(crate::realtime::ws::value_decimal),
        low: arr
            .and_then(|a| a.get(3))
            .and_then(crate::realtime::ws::value_decimal),
        close: arr
            .and_then(|a| a.get(4))
            .and_then(crate::realtime::ws::value_decimal),
        volume: arr
            .and_then(|a| a.get(6))
            .and_then(crate::realtime::ws::value_decimal),
    }
}

fn parse_order(raw: &Value) -> Order {
    let status = raw["status"].as_str().map(|s| match s {
        "open" | "partially_filled" => "open",
        "closed" | "filled" => "closed",
        "canceled" => "canceled",
        other => other,
    });
    let ts = raw["opentm"].as_f64().map(|f| f as i64 * 1000);
    Order {
        id: raw["userref"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        status: status.map(str::to_string),
        symbol: raw["descr"]
            .as_str()
            .and_then(|d| d.split(' ').next())
            .map(str::to_string),
        order_type: raw["descr"]
            .as_str()
            .and_then(|d| d.split(' ').nth(1))
            .map(str::to_string),
        side: raw["descr"]
            .as_str()
            .and_then(|d| d.split(' ').nth(2))
            .map(str::to_string),
        price: raw["price"].as_str().and_then(|s| s.parse().ok()),
        amount: raw["vol"].as_str().and_then(|s| s.parse().ok()),
        filled: raw["vol_exec"].as_str().and_then(|s| s.parse().ok()),
        info: raw.clone(),
        ..Order::default()
    }
}

fn parse_own_trade(raw: &Value) -> Trade {
    let ts = raw["time"].as_f64().map(|f| f as i64 * 1000);
    Trade {
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        side: raw["type"].as_str().map(str::to_string),
        price: raw["price"].as_str().and_then(|s| s.parse().ok()),
        amount: raw["vol"].as_str().and_then(|s| s.parse().ok()),
        cost: raw["cost"].as_str().and_then(|s| s.parse().ok()),
        fee: raw["fee"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .map(|f| crate::types::Fee {
                cost: Some(f),
                ..crate::types::Fee::default()
            }),
        info: raw.clone(),
        ..Trade::default()
    }
}

/// kraken ohlc 消息的 interval:data 数组最后一个元素。
fn d_interval(data: Option<&Value>) -> String {
    data.and_then(Value::as_array)
        .and_then(|a| a.last())
        .and_then(|v| match v {
            Value::Number(n) => n.as_i64().map(|i| i.to_string()),
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "1".to_string())
}

fn parse_changes(v: Option<&Value>) -> Vec<PriceChange> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|row| {
                    let arr = row.as_array()?;
                    Some(PriceChange {
                        price: crate::realtime::ws::value_decimal(arr.first()?)?,
                        size: crate::realtime::ws::value_decimal(arr.get(1)?)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn collect_levels(v: Option<&Value>) -> Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|row| {
                    let arr = row.as_array()?;
                    Some((
                        crate::realtime::ws::value_decimal(arr.first()?)?,
                        crate::realtime::ws::value_decimal(arr.get(1)?)?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Realtime for KrakenWs {
    async fn watch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        self.ensure_public();
        let pair = self.symbol_id(symbol);
        self.subscribe("ticker", &pair).await?;
        let rx = {
            let mut map = self.tickers.lock().unwrap();
            if !map.contains_key(&pair) {
                let (tx, _) = watch::channel(Ticker::default());
                map.insert(pair.clone(), tx.clone());
            }
            map.get(&pair).cloned().unwrap().subscribe()
        };
        wait_first(rx).await
    }

    async fn watch_order_book(
        &self,
        symbol: &str,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        self.ensure_public();
        let pair = self.symbol_id(symbol);
        self.subscribe("book", &pair).await?;
        if !self.book_stores.lock().unwrap().contains_key(&pair) {
            self.book_stores
                .lock()
                .unwrap()
                .insert(pair.clone(), OrderBookStore::new(0));
            let (tx, _) = watch::channel(OrderBook::default());
            self.books.lock().unwrap().insert(pair.clone(), tx);
        }
        let rx = self
            .books
            .lock()
            .unwrap()
            .get(&pair)
            .cloned()
            .unwrap()
            .subscribe();
        wait_first(rx).await
    }

    async fn watch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        self.ensure_public();
        let pair = self.symbol_id(symbol);
        self.subscribe("trade", &pair).await?;
        let rx = {
            let mut map = self.trades.lock().unwrap();
            if !map.contains_key(&pair) {
                let (tx, _) = watch::channel(vec![]);
                map.insert(pair.clone(), tx.clone());
            }
            map.get(&pair).cloned().unwrap().subscribe()
        };
        wait_first(rx).await
    }

    async fn watch_ohlcv(
        &self,
        symbol: &str,
        timeframe: &str,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<OHLCV>> {
        self.ensure_public();
        let pair = self.symbol_id(symbol);
        self.subscribe("ohlc", &pair).await?;
        let key = (pair, timeframe.to_string());
        let rx = {
            let mut map = self.ohlcvs.lock().unwrap();
            if !map.contains_key(&key) {
                let (tx, _) = watch::channel(vec![]);
                map.insert(key.clone(), tx.clone());
            }
            map.get(&key).cloned().unwrap().subscribe()
        };
        wait_first(rx).await
    }

    async fn watch_balance(&self, _params: Params) -> Result<Balances> {
        self.ensure_private().await?;
        let token = self.fetch_ws_token().await?;
        self.subscribe_private("balance", &token).await?;
        let tx = {
            let mut b = self.balances.lock().unwrap();
            if b.is_none() {
                let (tx, _) = watch::channel(Balances::default());
                *b = Some(tx);
            }
            b.clone().unwrap()
        };
        let mut rx = tx.subscribe();
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_orders(
        &self,
        _symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Order>> {
        self.ensure_private().await?;
        let token = self.fetch_ws_token().await?;
        self.subscribe_private("openOrders", &token).await?;
        let tx = {
            let mut o = self.orders.lock().unwrap();
            if o.is_none() {
                let (tx, _) = watch::channel(vec![]);
                *o = Some(tx);
            }
            o.clone().unwrap()
        };
        let mut rx = tx.subscribe();
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_my_trades(
        &self,
        _symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        self.ensure_private().await?;
        let token = self.fetch_ws_token().await?;
        self.subscribe_private("ownTrades", &token).await?;
        let tx = {
            let mut mt = self.my_trades.lock().unwrap();
            if mt.is_none() {
                let (tx, _) = watch::channel(vec![]);
                *mt = Some(tx);
            }
            mt.clone().unwrap()
        };
        let mut rx = tx.subscribe();
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_positions(
        &self,
        _symbols: Option<&[&str]>,
        _params: Params,
    ) -> Result<Vec<Position>> {
        // kraken 无持仓 WS 频道(与 ccxt 一致)
        Err(Error::not_supported("watch_positions(kraken)"))
    }
}

async fn wait_first<T: Clone>(mut rx: watch::Receiver<T>) -> Result<T> {
    rx.changed().await.map_err(ws_err)?;
    Ok(rx.borrow().clone())
}

fn ws_err(e: watch::error::RecvError) -> Error {
    Error::new(ErrorKind::NetworkError, format!("ws channel closed: {e}"))
}
