//! okx WebSocket 适配器(Phase C,ADR-0009):核心 8 频道。
//!
//! 对齐 OKX v5 WS 协议:
//! - 公开流 `wss://ws.okx.com:8443/ws/v5/public`,私密流 `.../ws/v5/private`;
//! - 订阅帧 `{"op":"subscribe","args":[{"channel":"...","instId":"BTC-USDT"}]}`;
//! - tickers / trades / candle{tf} / books(增量,seqId 对账)为公开频道;
//!   account / orders / positions / fills 为私密频道(先 login);
//! - 私密登录:`sign = base64(HMAC-SHA256(secret, ts + "GET" + "/users/self/verify"))`;
//! - books:`action=snapshot` 初始化(记录 seqId),`action=update` 校验
//!   seqId 连续后合并增量(复用 OrderBookStore::apply_binance_delta 的序列对账)。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::sync::watch;

use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Params, Realtime};
use crate::realtime::orderbook::OrderBookStore;
use crate::realtime::ws::{SubscriptionSet, WsSession};
use crate::types::{Balances, OHLCV, Order, OrderBook, Position, Ticker, Trade};

const WS_PUBLIC: &str = "wss://ws.okx.com:8443/ws/v5/public";
const WS_PRIVATE: &str = "wss://ws.okx.com:8443/ws/v5/private";

type TickerChannel = Arc<Mutex<HashMap<String, watch::Sender<Ticker>>>>;
type BookChannel = Arc<Mutex<HashMap<String, watch::Sender<OrderBook>>>>;
type BookStoreMap = Arc<Mutex<HashMap<String, OrderBookStore>>>;
type TradeChannel = Arc<Mutex<HashMap<String, watch::Sender<Vec<Trade>>>>>;
type OhlcvChannel = Arc<Mutex<HashMap<(String, String), watch::Sender<Vec<OHLCV>>>>>;

/// okx WS 适配器。
pub struct OkxWs {
    config: Config,
    pub_connected: Mutex<Option<watch::Receiver<bool>>>,
    priv_connected: Mutex<Option<watch::Receiver<bool>>>,
    tickers: TickerChannel,
    books: BookChannel,
    book_stores: BookStoreMap,
    trades: TradeChannel,
    ohlcvs: OhlcvChannel,
    balances: Mutex<Option<watch::Sender<Balances>>>,
    orders: Mutex<Option<watch::Sender<Vec<Order>>>>,
    positions: Mutex<Option<watch::Sender<Vec<Position>>>>,
    my_trades: Mutex<Option<watch::Sender<Vec<Trade>>>>,
    subs: Mutex<SubscriptionSet>,
    sub_tx: Mutex<Option<watch::Sender<Vec<String>>>>,
    priv_sub_tx: Mutex<Option<watch::Sender<Vec<String>>>>,
}

impl OkxWs {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
            pub_connected: Mutex::new(None),
            priv_connected: Mutex::new(None),
            tickers: Arc::new(Mutex::new(HashMap::new())),
            books: Arc::new(Mutex::new(HashMap::new())),
            book_stores: Arc::new(Mutex::new(HashMap::new())),
            trades: Arc::new(Mutex::new(HashMap::new())),
            ohlcvs: Arc::new(Mutex::new(HashMap::new())),
            balances: Mutex::new(None),
            orders: Mutex::new(None),
            positions: Mutex::new(None),
            my_trades: Mutex::new(None),
            subs: Mutex::new(SubscriptionSet::new()),
            sub_tx: Mutex::new(None),
            priv_sub_tx: Mutex::new(None),
        })
    }

    fn symbol_id(&self, symbol: &str) -> String {
        symbol.replace('/', "-")
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

    async fn subscribe(&self, channel: &str, inst_id: &str) -> Result<()> {
        let key = format!("{channel}:{inst_id}");
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
            "op": "subscribe",
            "args": [{"channel": channel, "instId": inst_id}]
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    async fn subscribe_private(&self, channel: &str) -> Result<()> {
        let first = self
            .subs
            .lock()
            .unwrap()
            .register(&format!("priv:{channel}"));
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
            "op": "subscribe",
            "args": [{"channel": channel, "instType": "SPOT"}]
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    async fn ensure_private(&self) -> Result<()> {
        if self.priv_connected.lock().unwrap().is_some() {
            return Ok(());
        }
        // 构造登录帧与私密连接
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "okx api_key required"))?;
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "okx secret required"))?;
        let passphrase = self
            .config
            .password
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "okx password required"))?;
        let ts = now_ms().to_string();
        let auth = format!("{ts}GET/users/self/verify");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| Error::new(ErrorKind::Authentication, format!("hmac key: {e}")))?;
        mac.update(auth.as_bytes());
        let sign = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let login_frame = json!({
            "op": "login",
            "args": [{"apiKey": api_key, "passphrase": passphrase, "timestamp": ts, "sign": sign}]
        })
        .to_string();
        let balances = self.balances.lock().unwrap().clone();
        let orders = self.orders.lock().unwrap().clone();
        let positions = self.positions.lock().unwrap().clone();
        let my_trades = self.my_trades.lock().unwrap().clone();
        let headers = HeaderMap::new();
        let (sub_tx, sub_rx) = tokio::sync::watch::channel(vec![login_frame]);
        let rx = WsSession::spawn(WS_PRIVATE.to_string(), headers, sub_rx, move |msg| {
            dispatch_private(msg, &balances, &orders, &positions, &my_trades)
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
    let channel = msg["arg"]["channel"].as_str().unwrap_or_default();
    let inst = msg["arg"]["instId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let data = msg["data"].as_array().cloned().unwrap_or_default();
    if data.is_empty() {
        return;
    }
    match channel {
        "tickers" => {
            let raw = &data[0];
            let symbol = inst.replace('-', "/");
            if let Some(tx) = tickers.lock().unwrap().get(&inst).cloned() {
                let _ = tx.send(parse_ticker(raw, &symbol));
            }
        }
        "books" => {
            let mut stores = book_stores.lock().unwrap();
            let books = books.lock().unwrap();
            if let (Some(store), Some(tx)) = (stores.get_mut(&inst), books.get(&inst).cloned()) {
                let seq = data[0]["seqId"].as_u64().unwrap_or(0);
                let bids = collect_levels(data[0].get("bids"));
                let asks = collect_levels(data[0].get("asks"));
                match msg["action"].as_str() {
                    Some("snapshot") => {
                        store.reset(&bids, &asks);
                        store.last_update_id = Some(seq);
                        let _ = tx.send(store.snapshot(
                            &inst.replace('-', "/"),
                            data[0]["ts"].as_str().and_then(|s| s.parse::<i64>().ok()),
                            Some(seq as i64),
                            data[0].clone(),
                        ));
                    }
                    Some("update") if store.apply_binance_delta(seq, &bids, &asks) => {
                        let _ = tx.send(store.snapshot(
                            &inst.replace('-', "/"),
                            data[0]["ts"].as_str().and_then(|s| s.parse::<i64>().ok()),
                            Some(seq as i64),
                            data[0].clone(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        "trades" => {
            if let Some(tx) = trades.lock().unwrap().get(&inst).cloned() {
                let symbol = inst.replace('-', "/");
                let parsed: Vec<Trade> = data.iter().map(|t| parse_trade(t, &symbol)).collect();
                let _ = tx.send(parsed);
            }
        }
        channel if channel.starts_with("candle") => {
            // candle1m → (inst, "1m")
            let tf = channel.trim_start_matches("candle").to_string();
            if let Some(tx) = ohlcvs
                .lock()
                .unwrap()
                .get(&(inst.clone(), tf.clone()))
                .cloned()
            {
                let _ = tx.send(vec![parse_candle(&data[0])]);
            }
        }
        _ => {}
    }
}

fn dispatch_private(
    msg: Value,
    balances: &Option<watch::Sender<Balances>>,
    orders: &Option<watch::Sender<Vec<Order>>>,
    positions: &Option<watch::Sender<Vec<Position>>>,
    my_trades: &Option<watch::Sender<Vec<Trade>>>,
) {
    let channel = msg["arg"]["channel"].as_str().unwrap_or_default();
    let data = msg["data"].as_array().cloned().unwrap_or_default();
    if data.is_empty() {
        return;
    }
    match channel {
        "account" => {
            if let Some(tx) = balances.as_ref() {
                let mut out = Balances {
                    info: msg.clone(),
                    ..Balances::default()
                };
                if let Some(details) = data[0].get("details").and_then(Value::as_array) {
                    for d in details {
                        if let Some(code) = d["ccy"].as_str() {
                            let cash = dec(d.get("cashBal"));
                            let avail = dec(d.get("availBal"));
                            out.accounts.insert(
                                code.to_string(),
                                crate::types::Balance {
                                    free: avail.or(cash),
                                    total: cash,
                                    ..crate::types::Balance::default()
                                },
                            );
                        }
                    }
                }
                let _ = tx.send(out);
            }
        }
        "orders" => {
            if let Some(tx) = orders.as_ref() {
                let parsed: Vec<Order> = data.iter().map(parse_order).collect();
                let _ = tx.send(parsed);
            }
        }
        "positions" => {
            if let Some(tx) = positions.as_ref() {
                let parsed: Vec<Position> = data
                    .iter()
                    .filter(|p| p["pos"].as_str().map(|s| s != "0").unwrap_or(false))
                    .map(parse_position)
                    .collect();
                let _ = tx.send(parsed);
            }
        }
        "fills" => {
            if let Some(tx) = my_trades.as_ref() {
                let parsed: Vec<Trade> = data.iter().map(parse_fill).collect();
                let _ = tx.send(parsed);
            }
        }
        _ => {}
    }
}

fn parse_ticker(raw: &Value, symbol: &str) -> Ticker {
    let ts = raw["ts"].as_str().and_then(|s| s.parse::<i64>().ok());
    Ticker {
        symbol: symbol.to_string(),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        open: dec(raw.get("open24h")),
        high: dec(raw.get("high24h")),
        low: dec(raw.get("low24h")),
        bid: dec(raw.get("bidPx")),
        ask: dec(raw.get("askPx")),
        last: dec(raw.get("last")),
        close: dec(raw.get("last")),
        base_volume: dec(raw.get("vol24h")),
        quote_volume: dec(raw.get("volCcy24h")),
        info: raw.clone(),
        ..Ticker::default()
    }
}

fn parse_trade(raw: &Value, symbol: &str) -> Trade {
    let ts = raw["ts"].as_str().and_then(|s| s.parse::<i64>().ok());
    Trade {
        id: raw["tradeId"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: Some(symbol.to_string()),
        side: raw["side"].as_str().map(|s| s.to_lowercase()),
        price: dec(raw.get("px")),
        amount: dec(raw.get("sz")),
        info: raw.clone(),
        ..Trade::default()
    }
}

fn parse_candle(raw: &Value) -> OHLCV {
    // [ts, o, h, l, c, vol, ...]
    let arr = raw.as_array();
    let ts = arr
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok());
    OHLCV {
        timestamp: ts,
        open: arr
            .and_then(|a| a.get(1))
            .and_then(super::ws::value_decimal),
        high: arr
            .and_then(|a| a.get(2))
            .and_then(super::ws::value_decimal),
        low: arr
            .and_then(|a| a.get(3))
            .and_then(super::ws::value_decimal),
        close: arr
            .and_then(|a| a.get(4))
            .and_then(super::ws::value_decimal),
        volume: arr
            .and_then(|a| a.get(5))
            .and_then(super::ws::value_decimal),
    }
}

fn parse_order(raw: &Value) -> Order {
    let status = raw["state"]
        .as_str()
        .map(|s| match s {
            "live" => "open",
            "partially_filled" => "open",
            "filled" => "closed",
            "canceled" => "canceled",
            other => other,
        })
        .map(str::to_string);
    let ts = raw["uTime"].as_str().and_then(|s| s.parse::<i64>().ok());
    Order {
        id: raw["ordId"].as_str().map(str::to_string),
        client_order_id: raw["clOrdId"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        status,
        symbol: raw["instId"].as_str().map(|s| s.replace('-', "/")),
        order_type: raw["ordType"].as_str().map(|s| s.to_lowercase()),
        side: raw["side"].as_str().map(|s| s.to_lowercase()),
        price: dec(raw.get("px")),
        average: dec(raw.get("avgPx")),
        amount: dec(raw.get("sz")),
        filled: dec(raw.get("accFillSz")),
        info: raw.clone(),
        ..Order::default()
    }
}

fn parse_position(raw: &Value) -> Position {
    Position {
        symbol: raw["instId"].as_str().map(|s| s.replace('-', "/")),
        id: raw["posId"].as_str().map(str::to_string),
        contracts: dec(raw.get("pos")),
        entry_price: dec(raw.get("avgPx")),
        unrealized_pnl: dec(raw.get("upl")),
        notional: dec(raw.get("notionalUsd")),
        info: raw.clone(),
        ..Position::default()
    }
}

fn parse_fill(raw: &Value) -> Trade {
    let ts = raw["ts"].as_str().and_then(|s| s.parse::<i64>().ok());
    Trade {
        id: raw["tradeId"].as_str().map(str::to_string),
        order: raw["ordId"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: raw["instId"].as_str().map(|s| s.replace('-', "/")),
        side: raw["side"].as_str().map(|s| s.to_lowercase()),
        price: dec(raw.get("px")),
        amount: dec(raw.get("sz")),
        cost: dec(raw.get("fillPnl")),
        info: raw.clone(),
        ..Trade::default()
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

fn iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Realtime for OkxWs {
    async fn watch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        self.ensure_public();
        let inst = self.symbol_id(symbol);
        self.subscribe("tickers", &inst).await?;
        let rx = {
            let mut map = self.tickers.lock().unwrap();
            if !map.contains_key(&inst) {
                let (tx, _) = watch::channel(Ticker::default());
                map.insert(inst.clone(), tx.clone());
            }
            map.get(&inst).cloned().unwrap().subscribe()
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
        let inst = self.symbol_id(symbol);
        self.subscribe("books", &inst).await?;
        if !self.book_stores.lock().unwrap().contains_key(&inst) {
            self.book_stores
                .lock()
                .unwrap()
                .insert(inst.clone(), OrderBookStore::new(0));
            let (tx, _) = watch::channel(OrderBook::default());
            self.books.lock().unwrap().insert(inst.clone(), tx);
        }
        let rx = self
            .books
            .lock()
            .unwrap()
            .get(&inst)
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
        let inst = self.symbol_id(symbol);
        self.subscribe("trades", &inst).await?;
        let rx = {
            let mut map = self.trades.lock().unwrap();
            if !map.contains_key(&inst) {
                let (tx, _) = watch::channel(vec![]);
                map.insert(inst.clone(), tx.clone());
            }
            map.get(&inst).cloned().unwrap().subscribe()
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
        let inst = self.symbol_id(symbol);
        let channel = format!("candle{timeframe}");
        self.subscribe(&channel, &inst).await?;
        let key = (inst, timeframe.to_string());
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
        self.subscribe_private("account").await?;
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
        self.subscribe_private("orders").await?;
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
        self.subscribe_private("fills").await?;
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
        self.ensure_private().await?;
        self.subscribe_private("positions").await?;
        let tx = {
            let mut p = self.positions.lock().unwrap();
            if p.is_none() {
                let (tx, _) = watch::channel(vec![]);
                *p = Some(tx);
            }
            p.clone().unwrap()
        };
        let mut rx = tx.subscribe();
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }
}

async fn wait_first<T: Clone>(mut rx: watch::Receiver<T>) -> Result<T> {
    rx.changed().await.map_err(ws_err)?;
    Ok(rx.borrow().clone())
}

fn ws_err(e: watch::error::RecvError) -> Error {
    Error::new(ErrorKind::NetworkError, format!("ws channel closed: {e}"))
}
