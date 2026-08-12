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
use crate::httpcore::{collect_levels, dec, iso8601, query_string};
use crate::realtime::orderbook::OrderBookStore;
use crate::realtime::ws::{ChannelMap, Conn, SubscriptionSet, WsSession, wait_first, ws_err};
use crate::types::{Balances, OHLCV, Order, OrderBook, Position, Ticker, Trade};

const WS_BASE: &str = "wss://stream.binance.com:9443/ws";
const REST_BASE: &str = "https://api.binance.com/api/v3";

/// symbol + timeframe → OHLCV 频道。
type OhlcvChannel = ChannelMap<(String, String), Vec<OHLCV>>;
type TickerChannel = ChannelMap<String, Ticker>;
type BookChannel = ChannelMap<String, OrderBook>;
type BookStoreMap = Arc<Mutex<HashMap<String, OrderBookStore>>>;
type TradeChannel = ChannelMap<String, Vec<Trade>>;

/// binance WS 适配器。
pub struct BinanceWs {
    config: Config,
    client: Client,
    /// 公开流连接就绪信号(懒启动,ADR-0014 Conn 收口)。
    pub_connected: Conn,
    tickers: TickerChannel,
    books: BookChannel,
    book_stores: BookStoreMap,
    trades: TradeChannel,
    ohlcvs: OhlcvChannel,
    // 私密流
    user_connected: Conn,
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
            pub_connected: Conn::new(),
            tickers: Arc::new(Mutex::new(HashMap::new())),
            books: Arc::new(Mutex::new(HashMap::new())),
            book_stores: Arc::new(Mutex::new(HashMap::new())),
            trades: Arc::new(Mutex::new(HashMap::new())),
            ohlcvs: Arc::new(Mutex::new(HashMap::new())),
            user_connected: Conn::new(),
            balances: Mutex::new(None),
            orders: Mutex::new(None),
            my_trades: Mutex::new(None),
            subs: Mutex::new(SubscriptionSet::new()),
            sub_tx: Mutex::new(None),
        })
    }

    /// 懒启动公开流连接(单例),返回连接就绪信号。
    fn ensure_public(&self) -> watch::Receiver<bool> {
        self.pub_connected.ensure(|| {
            let (sub_tx, sub_rx) = tokio::sync::watch::channel(Vec::new());
            *self.sub_tx.lock().unwrap() = Some(sub_tx);
            let tickers = self.tickers.clone();
            let books = self.books.clone();
            let book_stores = self.book_stores.clone();
            let trades = self.trades.clone();
            let ohlcvs = self.ohlcvs.clone();
            WsSession::spawn(
                WS_BASE.to_string(),
                HeaderMap::new(),
                sub_rx,
                move |msg| dispatch_public(msg, &tickers, &books, &book_stores, &trades, &ohlcvs),
                None,
            )
        })
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
        let bids: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> =
            collect_levels(Some(&resp["bids"]));
        let asks: Vec<(rust_decimal::Decimal, rust_decimal::Decimal)> =
            collect_levels(Some(&resp["asks"]));
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

impl BinanceWs {
    /// 懒启动私密流(listenKey),返回 (balance, orders, my_trades) 三个 channel。
    async fn ensure_user(
        &self,
    ) -> Result<(
        Option<watch::Receiver<Balances>>,
        watch::Receiver<Vec<Order>>,
        watch::Receiver<Vec<Trade>>,
    )> {
        if self.user_connected.is_connected() {
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
        self.user_connected.ensure(|| {
            WsSession::spawn(
                url,
                headers,
                sub_rx,
                move |msg| dispatch_user(msg, &balances, &orders_tx, &mt_tx),
                None,
            )
        });
        let balance = self.balances.lock().unwrap().clone().map(|t| t.subscribe());
        let orders = self.orders.lock().unwrap().clone().unwrap().subscribe();
        let my_trades = self.my_trades.lock().unwrap().clone().unwrap().subscribe();
        Ok((balance, orders, my_trades))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels() -> (
        TickerChannel,
        BookChannel,
        BookStoreMap,
        TradeChannel,
        OhlcvChannel,
    ) {
        (
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        )
    }

    /// 离线重放(ADR-0014):合成 binance WS 消息喂 dispatch,验证频道输出。
    #[tokio::test]
    async fn replay_mini_ticker_routes_to_channel() {
        let (tickers, books, stores, trades, ohlcvs) = channels();
        let (tx, rx) = watch::channel(Ticker::default());
        tickers.lock().unwrap().insert("BTCUSDT".into(), tx);
        let msg = json!({
            "e": "24hrMiniTicker", "s": "BTCUSDT", "E": 123,
            "o": "95", "h": "101", "l": "94", "c": "100", "v": "10", "q": "1000"
        });
        dispatch_public(msg, &tickers, &books, &stores, &trades, &ohlcvs);
        let t = rx.borrow().clone();
        assert_eq!(t.symbol, "BTCUSDT");
        assert_eq!(t.last, Some("100".parse().unwrap()));
        assert_eq!(t.base_volume, Some("10".parse().unwrap()));
    }

    #[tokio::test]
    async fn replay_depth_update_applies_delta() {
        let (tickers, books, stores, trades, ohlcvs) = channels();
        let mut store = OrderBookStore::new(0);
        store.reset(&[("100".parse().unwrap(), "1".parse().unwrap())], &[]);
        store.last_update_id = Some(1);
        stores.lock().unwrap().insert("BTCUSDT".into(), store);
        let (tx, rx) = watch::channel(OrderBook::default());
        books.lock().unwrap().insert("BTCUSDT".into(), tx);
        let msg = json!({
            "e": "depthUpdate", "s": "BTCUSDT", "u": 2, "E": 123,
            "b": [["100", "2"]], "a": [["101", "3"]]
        });
        dispatch_public(msg, &tickers, &books, &stores, &trades, &ohlcvs);
        let book = rx.borrow().clone();
        assert_eq!(book.bids[0].amount, Some("2".parse().unwrap()));
        assert_eq!(book.asks.len(), 1);
        assert_eq!(book.asks[0].price, Some("101".parse().unwrap()));
    }

    #[tokio::test]
    async fn replay_trade_routes_to_channel() {
        let (tickers, books, stores, trades, ohlcvs) = channels();
        let (tx, rx) = watch::channel(vec![]);
        trades.lock().unwrap().insert("BTCUSDT".into(), tx);
        let msg = json!({"e": "trade", "s": "BTCUSDT", "T": 1, "t": "9", "m": true, "p": "100", "q": "2"});
        dispatch_public(msg, &tickers, &books, &stores, &trades, &ohlcvs);
        let ts = rx.borrow().clone();
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].side.as_deref(), Some("sell")); // m=true → maker sell
        assert_eq!(ts[0].price, Some("100".parse().unwrap()));
    }

    #[tokio::test]
    async fn replay_kline_routes_to_channel() {
        let (tickers, books, stores, trades, ohlcvs) = channels();
        let (tx, rx) = watch::channel(vec![]);
        ohlcvs
            .lock()
            .unwrap()
            .insert(("BTCUSDT".into(), "1m".into()), tx);
        let msg = json!({
            "e": "kline", "s": "BTCUSDT",
            "k": {"t": 1, "i": "1m", "o": "100", "h": "101", "l": "99", "c": "100.5", "v": "3"}
        });
        dispatch_public(msg, &tickers, &books, &stores, &trades, &ohlcvs);
        let cs = rx.borrow().clone();
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].close, Some("100.5".parse().unwrap()));
        assert_eq!(cs[0].volume, Some("3".parse().unwrap()));
    }

    #[tokio::test]
    async fn replay_user_execution_report_updates_order() {
        let (tx_o, rx_o) = watch::channel(vec![]);
        let orders = Some(tx_o);
        let msg = json!({
            "e": "executionReport", "E": 1, "s": "BTCUSDT", "i": "123",
            "X": "FILLED", "o": "LIMIT", "S": "BUY", "p": "100", "L": "99", "q": "1", "z": "1"
        });
        dispatch_user(msg, &None, &orders, &None);
        let os = rx_o.borrow().clone();
        assert_eq!(os.len(), 1);
        assert_eq!(os[0].status.as_deref(), Some("closed"));
        assert_eq!(os[0].id.as_deref(), Some("123"));
    }

    #[tokio::test]
    async fn replay_user_balance_position() {
        let (tx_b, rx_b) = watch::channel(Balances::default());
        let balances = Some(tx_b);
        let msg =
            json!({"e": "outboundAccountPosition", "B": [{"a": "BTC", "f": "1.5", "l": "0.5"}]});
        dispatch_user(msg, &balances, &None, &None);
        let b = rx_b.borrow().clone();
        let acct = b.accounts.get("BTC").unwrap();
        assert_eq!(acct.free, Some("1.5".parse().unwrap()));
        assert_eq!(acct.used, Some("0.5".parse().unwrap()));
        assert_eq!(acct.total, Some("2.0".parse().unwrap()));
    }
}
