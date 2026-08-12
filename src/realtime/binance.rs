//! binance WebSocket 适配器(M4,ADR-0009):核心 8 频道。
//!
//! 对齐 ccxt.pro binance:
//! - 公开流 `wss://stream.binance.com:9443/ws`,`SUBSCRIBE` 帧;
//! - ticker `<sym>@miniTicker` / trades `<sym>@trade` / klines `<sym>@kline_<tf>`;
//! - 订单簿 `<sym>@depth@100ms`(diff)+ REST `/depth?limit=1000` 快照对账
//!   (u 序列,U/u 丢弃规则,ADR-0011 增量引擎);
//! - 私密流:POST `/api/v3/userDataStream` 取 listenKey → `/ws/<listenKey>`,
//!   `outboundAccountPosition` → 余额、`executionReport` → 订单/成交;
//! - 现货无持仓流,watch_positions 保持 NotSupported(与 ccxt 现货一致)。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::client::Client;
use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Params, Realtime};
use crate::realtime::orderbook::OrderBookStore;
use crate::realtime::ws::{SubscriptionSet, WsSession};
use crate::types::{Balances, OHLCV, Order, OrderBook, Position, Ticker, Trade};

const WS_BASE: &str = "wss://stream.binance.com:9443/ws";
const REST_BASE: &str = "https://api.binance.com/api/v3";

/// symbol + timeframe → OHLCV 频道。
type OhlcvChannel = Arc<Mutex<HashMap<(String, String), watch::Sender<Vec<OHLCV>>>>>;
type TickerChannel = Arc<Mutex<HashMap<String, watch::Sender<Ticker>>>>;
type BookChannel = Arc<Mutex<HashMap<String, watch::Sender<OrderBook>>>>;
type BookStoreMap = Arc<Mutex<HashMap<String, OrderBookStore>>>;
type TradeChannel = Arc<Mutex<HashMap<String, watch::Sender<Vec<Trade>>>>>;

/// binance WS 适配器。
pub struct BinanceWs {
    config: Config,
    client: Client,
    /// 公开流连接已就绪信号(懒启动)。
    pub_connected: Mutex<Option<watch::Receiver<bool>>>,
    tickers: TickerChannel,
    books: BookChannel,
    book_stores: BookStoreMap,
    trades: TradeChannel,
    ohlcvs: OhlcvChannel,
    // 私密流
    user_connected: Mutex<Option<watch::Receiver<bool>>>,
    balances: Mutex<Option<watch::Sender<Balances>>>,
    orders: Mutex<Option<watch::Sender<Vec<Order>>>>,
    my_trades: Mutex<Option<watch::Sender<Vec<Trade>>>>,
    subs: Mutex<SubscriptionSet>,
    sub_tx: Mutex<Option<tokio::sync::watch::Sender<Vec<String>>>>,
}

impl BinanceWs {
    /// 构造 WS 适配器(复用 REST 客户端做快照/认证)。
    pub fn new(config: Config) -> Result<Self> {
        let client = Client::new(
            config.timeout_ms,
            config.max_retries,
            config.proxy.as_deref(),
            config.rate_limit_ms.max(50),
            config.enable_rate_limit,
        )?;
        Ok(Self {
            config,
            client,
            pub_connected: Mutex::new(None),
            tickers: Arc::new(Mutex::new(HashMap::new())),
            books: Arc::new(Mutex::new(HashMap::new())),
            book_stores: Arc::new(Mutex::new(HashMap::new())),
            trades: Arc::new(Mutex::new(HashMap::new())),
            ohlcvs: Arc::new(Mutex::new(HashMap::new())),
            user_connected: Mutex::new(None),
            balances: Mutex::new(None),
            orders: Mutex::new(None),
            my_trades: Mutex::new(None),
            subs: Mutex::new(SubscriptionSet::new()),
            sub_tx: Mutex::new(None),
        })
    }

    /// 懒启动公开流连接(单例),返回连接就绪信号。
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
        let rx = WsSession::spawn(WS_BASE.to_string(), headers, sub_rx, move |msg| {
            dispatch_public(msg, &tickers, &books, &book_stores, &trades, &ohlcvs)
        });
        *self.pub_connected.lock().unwrap() = Some(rx.clone());
        *self.sub_tx.lock().unwrap() = Some(sub_tx);
        rx
    }

    /// 订阅(仅首次真正发送 SUBSCRIBE 帧)。
    async fn subscribe_public(&self, stream: &str) -> Result<()> {
        let first = self.subs.lock().unwrap().register(stream);
        if !first {
            return Ok(());
        }
        let tx = self
            .sub_tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::new(ErrorKind::NetworkError, "ws not started"))?;
        let frame = json!({"method": "SUBSCRIBE", "params": [stream], "id": 1}).to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    /// REST 快照初始化订单簿(symbol_id 需小写)。
    async fn ensure_book(
        &self,
        symbol: &str,
        limit: Option<i64>,
    ) -> Result<watch::Receiver<OrderBook>> {
        // 幂等:已存在直接返回
        if let Some(tx) = self.books.lock().unwrap().get(symbol).cloned() {
            return Ok(tx.subscribe());
        }
        // REST 快照
        let mut p = Params::new();
        p.insert("symbol".into(), json!(symbol.replace('/', "")));
        p.insert("limit".into(), json!(limit.unwrap_or(1000).min(1000)));
        let url = format!("{REST_BASE}/depth{}", query_string(&p));
        let headers = HeaderMap::new();
        let resp = self.client.request("GET", &url, &headers, None).await?;
        let last_update_id = resp["lastUpdateId"].as_u64().unwrap_or(0) as i64;
        let bids: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> = resp["bids"]
            .as_array()
            .map(|a| a.iter().filter_map(level_pair).collect())
            .unwrap_or_default();
        let asks: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> = resp["asks"]
            .as_array()
            .map(|a| a.iter().filter_map(level_pair).collect())
            .unwrap_or_default();
        let mut store = OrderBookStore::new(limit.unwrap_or(1000) as usize);
        store.reset(&bids, &asks);
        store.last_update_id = Some(last_update_id as u64);
        let (tx, _) =
            watch::channel(store.snapshot(symbol, None, Some(last_update_id), resp.clone()));
        self.book_stores
            .lock()
            .unwrap()
            .insert(symbol.to_string(), store);
        self.books
            .lock()
            .unwrap()
            .insert(symbol.to_string(), tx.clone());
        Ok(tx.subscribe())
    }
}

/// 公开流消息分发(binance 事件格式)。
fn dispatch_public(
    msg: Value,
    tickers: &TickerChannel,
    books: &BookChannel,
    book_stores: &BookStoreMap,
    trades: &TradeChannel,
    ohlcvs: &OhlcvChannel,
) {
    let event = msg["e"].as_str().unwrap_or_default();
    match event {
        "24hrMiniTicker" | "24hrTicker" => {
            let sym = msg["s"].as_str().unwrap_or_default().to_string();
            if let Some(tx) = tickers.lock().unwrap().get(&sym).cloned() {
                let _ = tx.send(parse_mini_ticker(&msg, &sym));
            }
        }
        "depthUpdate" => {
            let sym = msg["s"].as_str().unwrap_or_default().to_string();
            let u = msg["u"].as_u64().unwrap_or(0);
            let mut stores = book_stores.lock().unwrap();
            let books = books.lock().unwrap();
            if let (Some(store), Some(tx)) = (stores.get_mut(&sym), books.get(&sym).cloned()) {
                let bids = collect_levels(msg.get("b"));
                let asks = collect_levels(msg.get("a"));
                if store.apply_binance_delta(u, &bids, &asks) {
                    let _ = tx.send(store.snapshot(
                        &sym,
                        msg["E"].as_i64(),
                        Some(u as i64),
                        msg.clone(),
                    ));
                }
            }
        }
        "trade" => {
            let sym = msg["s"].as_str().unwrap_or_default().to_string();
            if let Some(tx) = trades.lock().unwrap().get(&sym).cloned() {
                let _ = tx.send(vec![parse_trade(&msg, &sym)]);
            }
        }
        "kline" => {
            let k = msg.get("k");
            let sym = msg["s"].as_str().unwrap_or_default().to_string();
            let tf = k
                .and_then(|k| k["i"].as_str())
                .unwrap_or_default()
                .to_string();
            if let Some(tx) = ohlcvs
                .lock()
                .unwrap()
                .get(&(sym.clone(), tf.clone()))
                .cloned()
            {
                let _ = tx.send(vec![parse_kline(k.unwrap_or(&Value::Null), &sym)]);
            }
        }
        _ => {}
    }
}

fn parse_mini_ticker(msg: &Value, sym: &str) -> Ticker {
    let now = msg["E"].as_i64();
    Ticker {
        symbol: sym.to_string(),
        timestamp: now,
        datetime: now.and_then(iso8601),
        open: dec(msg.get("o")),
        high: dec(msg.get("h")),
        low: dec(msg.get("l")),
        close: dec(msg.get("c")),
        last: dec(msg.get("c")),
        base_volume: dec(msg.get("v")),
        quote_volume: dec(msg.get("q")),
        info: msg.clone(),
        ..Ticker::default()
    }
}

fn parse_trade(msg: &Value, sym: &str) -> Trade {
    let ts = msg["T"].as_i64();
    Trade {
        id: msg["t"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: Some(sym.to_string()),
        side: msg["m"]
            .as_bool()
            .map(|is_buyer_maker| if is_buyer_maker { "sell" } else { "buy" })
            .map(str::to_string),
        price: dec(msg.get("p")),
        amount: dec(msg.get("q")),
        info: msg.clone(),
        ..Trade::default()
    }
}

fn parse_kline(k: &Value, _sym: &str) -> OHLCV {
    OHLCV {
        timestamp: k["t"].as_i64(),
        open: dec(k.get("o")),
        high: dec(k.get("h")),
        low: dec(k.get("l")),
        close: dec(k.get("c")),
        volume: dec(k.get("v")),
    }
}

fn dec(v: Option<&Value>) -> Option<rust_decimal::Decimal> {
    v.and_then(super::ws::value_decimal)
}

fn level_pair(raw: &Value) -> Option<(rust_decimal::Decimal, rust_decimal::Decimal)> {
    let arr = raw.as_array()?;
    Some((dec(arr.first())?, dec(arr.get(1))?))
}

fn collect_levels(v: Option<&Value>) -> Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(level_pair).collect())
        .unwrap_or_default()
}

fn query_string(params: &Params) -> String {
    if params.is_empty() {
        return String::new();
    }
    let pairs: Vec<String> = params
        .iter()
        .map(|(k, v)| {
            let val = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            };
            format!("{}={}", pct_encode(k), pct_encode(&val))
        })
        .collect();
    format!("?{}", pairs.join("&"))
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

fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

/// 私密流(listenKey)消息分发。
fn dispatch_user(
    msg: Value,
    balances: &Option<watch::Sender<Balances>>,
    orders: &Option<watch::Sender<Vec<Order>>>,
    my_trades: &Option<watch::Sender<Vec<Trade>>>,
) {
    match msg["e"].as_str().unwrap_or_default() {
        "outboundAccountPosition" => {
            if let Some(tx) = balances.as_ref() {
                let mut out = Balances {
                    info: msg.clone(),
                    ..Balances::default()
                };
                for b in msg["B"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(code) = b["a"].as_str() {
                        let free = dec(b.get("f"));
                        let locked = dec(b.get("l"));
                        out.accounts.insert(
                            code.to_string(),
                            crate::types::Balance {
                                free,
                                used: locked,
                                total: match (free, locked) {
                                    (Some(f), Some(l)) => Some(f + l),
                                    other => other.0,
                                },
                                ..crate::types::Balance::default()
                            },
                        );
                    }
                }
                let _ = tx.send(out);
            }
        }
        "executionReport" => {
            // 订单状态更新
            if let Some(tx) = orders.as_ref() {
                let order = parse_exec_order(&msg);
                let _ = tx.send(vec![order]);
            }
            // 成交部分(仅当有成交价)
            if msg["l"].is_string() || msg["l"].is_number() {
                if let Some(tx) = my_trades.as_ref() {
                    let t = parse_exec_trade(&msg);
                    let _ = tx.send(vec![t]);
                }
            }
        }
        _ => {}
    }
}

fn parse_exec_order(msg: &Value) -> Order {
    let ts = msg["E"].as_i64();
    Order {
        id: msg["i"].as_str().map(str::to_string),
        client_order_id: msg["c"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        status: msg["X"]
            .as_str()
            .map(|s| match s {
                "NEW" => "open",
                "PARTIALLY_FILLED" => "open",
                "FILLED" => "closed",
                "CANCELED" | "EXPIRED" => "canceled",
                "REJECTED" => "rejected",
                other => other,
            })
            .map(str::to_string),
        symbol: msg["s"].as_str().map(|s| s.replace('/', "")),
        order_type: msg["o"].as_str().map(|s| s.to_lowercase()),
        side: msg["S"].as_str().map(|s| s.to_lowercase()),
        price: dec(msg.get("p")),
        average: dec(msg.get("L")),
        amount: dec(msg.get("q")),
        filled: dec(msg.get("z")),
        remaining: dec(msg.get("q")).and_then(|q| dec(msg.get("z")).map(|z| q - z)),
        info: msg.clone(),
        ..Order::default()
    }
}

fn parse_exec_trade(msg: &Value) -> Trade {
    let ts = msg["T"].as_i64();
    Trade {
        id: msg["t"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: msg["s"].as_str().map(str::to_string),
        side: msg["S"].as_str().map(|s| s.to_lowercase()),
        price: dec(msg.get("L")),
        amount: dec(msg.get("l")),
        cost: dec(msg.get("l")).and_then(|l| dec(msg.get("L")).map(|p| l * p)),
        info: msg.clone(),
        ..Trade::default()
    }
}

impl Realtime for BinanceWs {
    async fn watch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        self.ensure_public();
        let sym = symbol.to_uppercase().replace('/', "");
        let stream = format!("{}@miniTicker", sym.to_lowercase());
        self.subscribe_public(&stream).await?;
        let rx = {
            let mut map = self.tickers.lock().unwrap();
            if !map.contains_key(&sym) {
                let (tx, _) = watch::channel(Ticker::default());
                map.insert(sym.clone(), tx.clone());
            }
            map.get(&sym).cloned().unwrap().subscribe()
        };
        // 触发订阅:连接任务负责真正发送;这里等待首个更新
        wait_first(rx).await
    }

    async fn watch_order_book(
        &self,
        symbol: &str,
        limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        self.ensure_public();
        let sym = symbol.to_uppercase().replace('/', "");
        let stream = format!("{}@depth@100ms", sym.to_lowercase());
        self.subscribe_public(&stream).await?;
        let rx = self.ensure_book(&sym, limit).await?;
        // REST 快照已在 ensure_book 中发出,直接返回当前状态(后续调用为增量最新值)
        Ok(rx.borrow().clone())
    }

    async fn watch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        self.ensure_public();
        let sym = symbol.to_uppercase().replace('/', "");
        let stream = format!("{}@trade", sym.to_lowercase());
        self.subscribe_public(&stream).await?;
        let rx = {
            let mut map = self.trades.lock().unwrap();
            if !map.contains_key(&sym) {
                let (tx, _) = watch::channel(vec![]);
                map.insert(sym.clone(), tx.clone());
            }
            map.get(&sym).cloned().unwrap().subscribe()
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
        let sym = symbol.to_uppercase().replace('/', "");
        let tf = timeframe.to_string();
        let stream = format!("{}@kline_{}", sym.to_lowercase(), timeframe);
        self.subscribe_public(&stream).await?;
        let key = (sym, tf);
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
        let (balance, _, _) = self.ensure_user().await?;
        let mut rx =
            balance.ok_or_else(|| Error::new(ErrorKind::NetworkError, "no balance channel"))?;
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
        let (_, orders, _) = self.ensure_user().await?;
        let mut rx = orders;
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
        let (_, _, my_trades) = self.ensure_user().await?;
        let mut rx = my_trades;
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_positions(
        &self,
        _symbols: Option<&[&str]>,
        _params: Params,
    ) -> Result<Vec<Position>> {
        // 现货无持仓流(与 ccxt 一致);futures 面后续增量
        Err(Error::not_supported("watch_positions(spot)"))
    }
}

/// 等待 watch channel 首个更新。
async fn wait_first<T: Clone>(mut rx: watch::Receiver<T>) -> Result<T> {
    rx.changed().await.map_err(ws_err)?;
    Ok(rx.borrow().clone())
}

fn ws_err(e: watch::error::RecvError) -> Error {
    Error::new(ErrorKind::NetworkError, format!("ws channel closed: {e}"))
}

impl BinanceWs {
    /// 懒启动私密流(listenKey),返回 (balance, orders, my_trades) 三个 channel。
    async fn ensure_user(
        &self,
    ) -> Result<(
        Option<watch::Receiver<Balances>>,
        watch::Receiver<Vec<Order>>,
        watch::Receiver<Vec<Trade>>,
    )> {
        if self.user_connected.lock().unwrap().is_some() {
            let balance = self.balances.lock().unwrap().clone().map(|t| t.subscribe());
            let orders = self.orders.lock().unwrap().clone().unwrap().subscribe();
            let my_trades = self.my_trades.lock().unwrap().clone().unwrap().subscribe();
            return Ok((balance, orders, my_trades));
        }
        // 获取 listenKey
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "binance api_key required"))?;
        let mut headers = HeaderMap::new();
        headers.insert("X-MBX-APIKEY", HeaderValue::from_str(api_key).unwrap());
        let resp = self
            .client
            .request(
                "POST",
                &format!("{REST_BASE}/userDataStream"),
                &headers,
                None,
            )
            .await?;
        let listen_key = resp["listenKey"]
            .as_str()
            .ok_or_else(|| Error::new(ErrorKind::BadResponse, "no listenKey"))?
            .to_string();
        let url = format!("{WS_BASE}/{listen_key}");
        // 初始 channel
        let mut channels = self.balances.lock().unwrap();
        if channels.is_none() {
            let (tx, _) = watch::channel(Balances::default());
            *channels = Some(tx);
        }
        let balances = self.balances.lock().unwrap().clone();
        let mut orders = self.orders.lock().unwrap();
        if orders.is_none() {
            let (tx, _) = watch::channel(vec![]);
            *orders = Some(tx);
        }
        let orders_tx = self.orders.lock().unwrap().clone();
        let mut mt = self.my_trades.lock().unwrap();
        if mt.is_none() {
            let (tx, _) = watch::channel(vec![]);
            *mt = Some(tx);
        }
        let mt_tx = self.my_trades.lock().unwrap().clone();
        let (_, sub_rx) = tokio::sync::watch::channel(Vec::new());
        let rx = WsSession::spawn(url, headers, sub_rx, move |msg| {
            dispatch_user(msg, &balances, &orders_tx, &mt_tx)
        });
        *self.user_connected.lock().unwrap() = Some(rx);
        let balance = self.balances.lock().unwrap().clone().map(|t| t.subscribe());
        let orders = self.orders.lock().unwrap().clone().unwrap().subscribe();
        let my_trades = self.my_trades.lock().unwrap().clone().unwrap().subscribe();
        Ok((balance, orders, my_trades))
    }
}
