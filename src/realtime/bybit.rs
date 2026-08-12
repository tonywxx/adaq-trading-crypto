//! bybit WebSocket 适配器(Phase C,ADR-0009):核心 8 频道。
//!
//! 对齐 bybit v5 WS 协议:
//! - 公开流 `wss://stream.bybit.com/v5/public/spot`,私密流 `.../v5/private`;
//! - 订阅帧 `{"op":"subscribe","args":["ticker.BTCUSDT","orderbook.50.BTCUSDT",...]}`;
//! - orderbook.50:首条 `type=snapshot`(记录 seq),后续 `type=delta` 按 seq 对账合并;
//! - 私密 auth:`{"op":"auth","args":[key, ts, recvWindow, hex(hmac(secret, ts+key+recvWindow))]}`;
//! - 私密频道:wallet(余额)/ order(订单)/ position(仓位)/ execution(成交)。

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use hmac::{Hmac, KeyInit, Mac};
use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::sync::watch;

use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Params, Realtime};
use crate::realtime::orderbook::OrderBookStore;
use crate::realtime::ws::{SubscriptionSet, WsSession};
use crate::types::{Balances, OHLCV, Order, OrderBook, Position, Ticker, Trade};

const WS_PUBLIC: &str = "wss://stream.bybit.com/v5/public/spot";
const WS_PRIVATE: &str = "wss://stream.bybit.com/v5/private";

type TickerChannel = Arc<Mutex<HashMap<String, watch::Sender<Ticker>>>>;
type BookChannel = Arc<Mutex<HashMap<String, watch::Sender<OrderBook>>>>;
type BookStoreMap = Arc<Mutex<HashMap<String, OrderBookStore>>>;
type TradeChannel = Arc<Mutex<HashMap<String, watch::Sender<Vec<Trade>>>>>;
type OhlcvChannel = Arc<Mutex<HashMap<(String, String), watch::Sender<Vec<OHLCV>>>>>;

/// bybit WS 适配器。
pub struct BybitWs {
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

impl BybitWs {
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
        symbol.replace('/', "")
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

    async fn subscribe(&self, arg: &str) -> Result<()> {
        let first = self.subs.lock().unwrap().register(arg);
        if !first {
            return Ok(());
        }
        let tx = self
            .sub_tx
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::new(ErrorKind::NetworkError, "ws not started"))?;
        let frame = json!({"op": "subscribe", "args": [arg]}).to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    async fn subscribe_private(&self, channel: &str) -> Result<()> {
        let key = format!("priv:{channel}");
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
        let frame = json!({"op": "subscribe", "args": [channel]}).to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }

    async fn ensure_private(&self) -> Result<()> {
        if self.priv_connected.lock().unwrap().is_some() {
            return Ok(());
        }
        let api_key = self
            .config
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "bybit api_key required"))?;
        let secret = self
            .config
            .secret
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "bybit secret required"))?;
        let ts = now_ms().to_string();
        let recv_window = "20000";
        let auth_str = format!("{ts}{api_key}{recv_window}");
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .map_err(|e| Error::new(ErrorKind::Authentication, format!("hmac key: {e}")))?;
        mac.update(auth_str.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        let auth_frame = json!({
            "op": "auth",
            "args": [api_key, ts, recv_window, signature]
        })
        .to_string();
        let balances = self.balances.lock().unwrap().clone();
        let orders = self.orders.lock().unwrap().clone();
        let positions = self.positions.lock().unwrap().clone();
        let my_trades = self.my_trades.lock().unwrap().clone();
        let headers = HeaderMap::new();
        let (sub_tx, sub_rx) = tokio::sync::watch::channel(vec![auth_frame]);
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
    let topic = msg["topic"].as_str().unwrap_or_default().to_string();
    let data = msg["data"].clone();
    if topic.starts_with("tickers.") {
        if let Some(tx) = tickers
            .lock()
            .unwrap()
            .get(topic.trim_start_matches("tickers."))
            .cloned()
        {
            let _ = tx.send(parse_ticker(&data));
        }
    } else if topic.starts_with("orderbook.") {
        let sym = topic.rsplit('.').next().unwrap_or_default().to_string();
        let mut stores = book_stores.lock().unwrap();
        let books = books.lock().unwrap();
        if let (Some(store), Some(tx)) = (stores.get_mut(&sym), books.get(&sym).cloned()) {
            let seq = data["seq"].as_u64().unwrap_or(0);
            let bids = collect_levels(data.get("b"));
            let asks = collect_levels(data.get("a"));
            match msg["type"].as_str() {
                Some("snapshot") => {
                    store.reset(&bids, &asks);
                    store.last_update_id = Some(seq);
                    let _ = tx.send(store.snapshot(
                        &sym,
                        data["cts"].as_i64(),
                        Some(seq as i64),
                        data.clone(),
                    ));
                }
                Some("delta") if store.apply_binance_delta(seq, &bids, &asks) => {
                    let _ = tx.send(store.snapshot(
                        &sym,
                        data["cts"].as_i64(),
                        Some(seq as i64),
                        data.clone(),
                    ));
                }
                _ => {}
            }
        }
    } else if topic.starts_with("publicTrade.") {
        let sym = topic.trim_start_matches("publicTrade.").to_string();
        if let Some(tx) = trades.lock().unwrap().get(&sym).cloned() {
            let arr = data.as_array().cloned().unwrap_or_default();
            let parsed: Vec<Trade> = arr.iter().map(parse_trade).collect();
            let _ = tx.send(parsed);
        }
    } else if topic.starts_with("kline.") {
        // kline.{interval}.{symbol}
        let parts: Vec<&str> = topic.split('.').collect();
        if parts.len() >= 3 {
            let tf = parts[1].to_string();
            let sym = parts[2..].join(".");
            if let Some(tx) = ohlcvs.lock().unwrap().get(&(sym, tf)).cloned() {
                let arr = data.as_array().cloned().unwrap_or_default();
                let parsed: Vec<OHLCV> = arr.iter().map(parse_candle).collect();
                let _ = tx.send(parsed);
            }
        }
    }
}

fn dispatch_private(
    msg: Value,
    balances: &Option<watch::Sender<Balances>>,
    orders: &Option<watch::Sender<Vec<Order>>>,
    positions: &Option<watch::Sender<Vec<Position>>>,
    my_trades: &Option<watch::Sender<Vec<Trade>>>,
) {
    let topic = msg["topic"].as_str().unwrap_or_default();
    let data = msg["data"].clone();
    match topic {
        "wallet" => {
            if let Some(tx) = balances.as_ref() {
                let mut out = Balances {
                    info: msg.clone(),
                    ..Balances::default()
                };
                if let Some(coins) = data
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|d| d.get("coin"))
                    .and_then(Value::as_array)
                {
                    for c in coins {
                        if let Some(code) = c["coin"].as_str() {
                            out.accounts.insert(
                                code.to_string(),
                                crate::types::Balance {
                                    free: dec(c.get("availableToWithdraw")),
                                    total: dec(c.get("walletBalance")),
                                    ..crate::types::Balance::default()
                                },
                            );
                        }
                    }
                }
                let _ = tx.send(out);
            }
        }
        "order" => {
            if let Some(tx) = orders.as_ref() {
                let arr = data.as_array().cloned().unwrap_or_default();
                let parsed: Vec<Order> = arr.iter().map(parse_order).collect();
                let _ = tx.send(parsed);
            }
        }
        "position" => {
            if let Some(tx) = positions.as_ref() {
                let arr = data.as_array().cloned().unwrap_or_default();
                let parsed: Vec<Position> = arr
                    .iter()
                    .filter(|p| p["size"].as_str().map(|s| s != "0").unwrap_or(false))
                    .map(parse_position)
                    .collect();
                let _ = tx.send(parsed);
            }
        }
        "execution" => {
            if let Some(tx) = my_trades.as_ref() {
                let arr = data.as_array().cloned().unwrap_or_default();
                let parsed: Vec<Trade> = arr.iter().map(parse_fill).collect();
                let _ = tx.send(parsed);
            }
        }
        _ => {}
    }
}

fn parse_ticker(raw: &Value) -> Ticker {
    let sym = raw["symbol"].as_str().unwrap_or_default().to_string();
    let ts = raw["timestamp"].as_i64().or_else(|| raw["ts"].as_i64());
    Ticker {
        symbol: sym,
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        high: dec(raw.get("high24h")),
        low: dec(raw.get("low24h")),
        bid: dec(raw.get("bid1Price")),
        ask: dec(raw.get("ask1Price")),
        last: dec(raw.get("lastPrice")),
        close: dec(raw.get("lastPrice")),
        base_volume: dec(raw.get("volume24h")),
        quote_volume: dec(raw.get("turnover24h")),
        info: raw.clone(),
        ..Ticker::default()
    }
}

fn parse_trade(raw: &Value) -> Trade {
    let ts = raw["T"].as_i64();
    Trade {
        id: raw["i"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: raw["s"].as_str().map(str::to_string),
        side: raw["S"].as_str().map(|s| s.to_lowercase()),
        price: dec(raw.get("p")),
        amount: dec(raw.get("v")),
        info: raw.clone(),
        ..Trade::default()
    }
}

fn parse_candle(raw: &Value) -> OHLCV {
    OHLCV {
        timestamp: raw["start"].as_i64(),
        open: dec(raw.get("open")),
        high: dec(raw.get("high")),
        low: dec(raw.get("low")),
        close: dec(raw.get("close")),
        volume: dec(raw.get("volume")),
    }
}

fn parse_order(raw: &Value) -> Order {
    let status = raw["orderStatus"].as_str().map(|s| match s {
        "New" | "PartiallyFilled" => "open",
        "Filled" => "closed",
        "Cancelled" => "canceled",
        "Rejected" => "rejected",
        other => other,
    });
    let ts = raw["timestamp"].as_i64();
    Order {
        id: raw["orderId"].as_str().map(str::to_string),
        client_order_id: raw["orderLinkId"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        status: status.map(str::to_string),
        symbol: raw["symbol"].as_str().map(str::to_string),
        order_type: raw["orderType"].as_str().map(|s| s.to_lowercase()),
        side: raw["side"].as_str().map(|s| s.to_lowercase()),
        price: dec(raw.get("price")),
        average: dec(raw.get("avgPrice")),
        amount: dec(raw.get("qty")),
        filled: dec(raw.get("cumExecQty")),
        info: raw.clone(),
        ..Order::default()
    }
}

fn parse_position(raw: &Value) -> Position {
    Position {
        symbol: raw["symbol"].as_str().map(str::to_string),
        id: raw["positionIdx"].as_str().map(str::to_string),
        contracts: dec(raw.get("size")),
        entry_price: dec(raw.get("avgPrice")),
        unrealized_pnl: dec(raw.get("unrealisedPnl")),
        notional: dec(raw.get("positionValue")),
        info: raw.clone(),
        ..Position::default()
    }
}

fn parse_fill(raw: &Value) -> Trade {
    let ts = raw["execTime"].as_i64();
    Trade {
        id: raw["execId"].as_str().map(str::to_string),
        order: raw["orderId"].as_str().map(str::to_string),
        timestamp: ts,
        datetime: ts.and_then(iso8601),
        symbol: raw["symbol"].as_str().map(str::to_string),
        side: raw["side"].as_str().map(|s| s.to_lowercase()),
        price: dec(raw.get("execPrice")),
        amount: dec(raw.get("execQty")),
        cost: dec(raw.get("execFee")),
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

impl Realtime for BybitWs {
    async fn watch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        self.ensure_public();
        let sym = self.symbol_id(symbol);
        // bybit 现货 ticker 频道为复数形式 tickers.{symbol}
        self.subscribe(&format!("tickers.{sym}")).await?;
        let rx = {
            let mut map = self.tickers.lock().unwrap();
            if !map.contains_key(&sym) {
                let (tx, _) = watch::channel(Ticker::default());
                map.insert(sym.clone(), tx.clone());
            }
            map.get(&sym).cloned().unwrap().subscribe()
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
        let sym = self.symbol_id(symbol);
        self.subscribe(&format!("orderbook.50.{sym}")).await?;
        if !self.book_stores.lock().unwrap().contains_key(&sym) {
            self.book_stores
                .lock()
                .unwrap()
                .insert(sym.clone(), OrderBookStore::new(50));
            let (tx, _) = watch::channel(OrderBook::default());
            self.books.lock().unwrap().insert(sym.clone(), tx);
        }
        let rx = self
            .books
            .lock()
            .unwrap()
            .get(&sym)
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
        let sym = self.symbol_id(symbol);
        // bybit 现货成交频道为 publicTrade.{symbol}
        self.subscribe(&format!("publicTrade.{sym}")).await?;
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
        let sym = self.symbol_id(symbol);
        let tf = timeframe.to_string();
        self.subscribe(&format!("kline.{tf}.{sym}")).await?;
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
        self.ensure_private().await?;
        self.subscribe_private("wallet").await?;
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
        self.subscribe_private("order").await?;
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
        self.subscribe_private("execution").await?;
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
        self.subscribe_private("position").await?;
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
