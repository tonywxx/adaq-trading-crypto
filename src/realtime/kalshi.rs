//! kalshi WebSocket 适配器(M4,ADR-0009)。
//!
//! ccxt 未实现 kalshi WS(`pro: false`)——本模块按 kalshi 官方契约实现
//! (https://docs.kalshi.com/getting_started/quick_start_websockets):
//! - 端点 `wss://external-api-ws.kalshi.com/trade-api/ws/v2`,握手需
//!   `KALSHI-ACCESS-KEY / -SIGNATURE / -TIMESTAMP` 头,签名 payload
//!   `{timestamp}GET/trade-api/ws/v2`(RSA-PSS,与 REST 相同);
//! - 订阅 `{"id": 1, "cmd": "subscribe", "params": {"channels": [...], "market_tickers": [...]}}`;
//! - 频道:`ticker`(公开)/ `trade`(公开)/ `orderbook_delta`(快照 + 增量);
//! - keep-alive:服务端 10s Ping(body `heartbeat`),客户端回 Pong。
//!
//! watch_balance/orders/my_trades/ohlcv 保持 NotSupported(`fill`/`order`/
//! `market_positions` 频道为后续增量)。

use std::collections::HashMap;
use std::sync::Mutex;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::error::{Error, ErrorKind, Result};
use crate::exchange::{Config, Exchange, Params, Realtime};
use crate::httpcore::{collect_levels, dec, iso8601, now_ms};
use crate::realtime::orderbook::OrderBookStore;
use crate::realtime::ws::{SubscriptionSet, WsSession, ws_err};
use crate::types::{Balances, OHLCV, Order, OrderBook, Position, Ticker, Trade};

const WS_BASE: &str = "wss://external-api-ws.kalshi.com/trade-api/ws/v2";
const SIGN_PATH: &str = "/trade-api/ws/v2";

/// kalshi WS 适配器。
pub struct KalshiWs {
    /// REST 适配器实例:WS 消息复用其 parse_* 解析(解析合一,ADR-0013 方向)。
    rest: std::sync::Arc<crate::adapters::Kalshi>,
    sub_tx: Mutex<Option<tokio::sync::watch::Sender<Vec<String>>>>,
    subs: Mutex<SubscriptionSet>,
    tickers: std::sync::Arc<Mutex<HashMap<String, watch::Sender<Ticker>>>>,
    trades: std::sync::Arc<Mutex<HashMap<String, watch::Sender<Vec<Trade>>>>>,
    books: std::sync::Arc<Mutex<HashMap<String, watch::Sender<OrderBook>>>>,
    book_stores: std::sync::Arc<Mutex<HashMap<String, OrderBookStore>>>,
    orders: Mutex<Option<watch::Sender<Vec<Order>>>>,
    my_trades: Mutex<Option<watch::Sender<Vec<Trade>>>>,
    positions: Mutex<Option<watch::Sender<Vec<Position>>>>,
}

impl KalshiWs {
    pub fn new(config: Config) -> Result<Self> {
        let rest = std::sync::Arc::new(crate::adapters::Kalshi::new(config)?);
        Ok(Self {
            rest,
            sub_tx: Mutex::new(None),
            subs: Mutex::new(SubscriptionSet::new()),
            tickers: std::sync::Arc::new(Mutex::new(HashMap::new())),
            trades: std::sync::Arc::new(Mutex::new(HashMap::new())),
            books: std::sync::Arc::new(Mutex::new(HashMap::new())),
            book_stores: std::sync::Arc::new(Mutex::new(HashMap::new())),
            orders: Mutex::new(None),
            my_trades: Mutex::new(None),
            positions: Mutex::new(None),
        })
    }

    /// 解析 outcome symbol → market ticker(懒加载 markets 缓存)。
    async fn market_ticker(&self, symbol: &str) -> Result<String> {
        self.rest.load_markets().await?;
        Ok(self.rest.resolve_outcome(symbol)?.market_ticker)
    }

    /// 构造认证头(KALSHI-ACCESS-*;签名 payload `{ts}GET{SIGN_PATH}`)。
    fn auth_headers(&self) -> Result<HeaderMap> {
        let api_key = self
            .rest
            .config()
            .api_key
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Authentication, "kalshi apiKey required"))?;
        let private_key =
            self.rest.config().private_key.as_deref().ok_or_else(|| {
                Error::new(ErrorKind::Authentication, "kalshi privateKey required")
            })?;
        let timestamp = now_ms().to_string();
        let payload = format!("{timestamp}GET{SIGN_PATH}");
        let signature = crate::adapters::kalshi::sign_rsa_pss(&payload, private_key)?;
        let mut headers = HeaderMap::new();
        headers.insert("KALSHI-ACCESS-KEY", HeaderValue::from_str(api_key).unwrap());
        headers.insert(
            "KALSHI-ACCESS-TIMESTAMP",
            HeaderValue::from_str(&timestamp).unwrap(),
        );
        headers.insert(
            "KALSHI-ACCESS-SIGNATURE",
            HeaderValue::from_str(&signature).unwrap(),
        );
        Ok(headers)
    }

    /// 启动连接(单例,带认证头)。
    async fn ensure_conn(&self) -> Result<tokio::sync::watch::Sender<Vec<String>>> {
        if let Some(tx) = self.sub_tx.lock().unwrap().clone() {
            return Ok(tx);
        }
        let headers = self.auth_headers()?;
        let tickers = self.tickers.clone();
        let trades = self.trades.clone();
        let books = self.books.clone();
        let book_stores = self.book_stores.clone();
        let mut orders_map = self.orders.lock().unwrap();
        if orders_map.is_none() {
            let (tx, _) = watch::channel(vec![]);
            *orders_map = Some(tx);
        }
        let orders = self.orders.lock().unwrap().clone();
        let mut mt_map = self.my_trades.lock().unwrap();
        if mt_map.is_none() {
            let (tx, _) = watch::channel(vec![]);
            *mt_map = Some(tx);
        }
        let my_trades = self.my_trades.lock().unwrap().clone();
        let mut pos_map = self.positions.lock().unwrap();
        if pos_map.is_none() {
            let (tx, _) = watch::channel(vec![]);
            *pos_map = Some(tx);
        }
        let positions = self.positions.lock().unwrap().clone();
        let (sub_tx, sub_rx) = tokio::sync::watch::channel(Vec::new());
        let rest = self.rest.clone();
        let _ = WsSession::spawn(
            WS_BASE.to_string(),
            headers,
            sub_rx,
            move |msg| {
                dispatch(
                    msg,
                    &tickers,
                    &trades,
                    &books,
                    &book_stores,
                    &orders,
                    &my_trades,
                    &positions,
                    &rest,
                )
            },
            None,
        );
        *self.sub_tx.lock().unwrap() = Some(sub_tx.clone());
        Ok(sub_tx)
    }

    /// 订阅市场 ticker 上的频道(首次发送 subscribe 命令)。
    async fn subscribe_market(&self, market_ticker: &str, channel: &str) -> Result<()> {
        let key = format!("{channel}:{market_ticker}");
        let first = self.subs.lock().unwrap().register(&key);
        if !first {
            return Ok(());
        }
        let tx = self.ensure_conn().await?;
        let frame = json!({
            "id": 1,
            "cmd": "subscribe",
            "params": {
                "channels": [channel],
                "market_tickers": [market_ticker],
            }
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        Ok(())
    }
}

/// kalshi WS 消息分发(type 字段: ticker / orderbook_snapshot / orderbook_delta / trade)。
#[allow(clippy::too_many_arguments)]
fn dispatch(
    msg: Value,
    tickers: &Mutex<HashMap<String, watch::Sender<Ticker>>>,
    trades: &Mutex<HashMap<String, watch::Sender<Vec<Trade>>>>,
    books: &Mutex<HashMap<String, watch::Sender<OrderBook>>>,
    book_stores: &Mutex<HashMap<String, OrderBookStore>>,
    orders: &Option<watch::Sender<Vec<Order>>>,
    my_trades: &Option<watch::Sender<Vec<Trade>>>,
    positions: &Option<watch::Sender<Vec<Position>>>,
    rest: &crate::adapters::Kalshi,
) {
    let msg_type = msg["type"].as_str().unwrap_or_default();
    let m = msg.get("msg").unwrap_or(&Value::Null);
    let market_ticker = m["market_ticker"].as_str().unwrap_or_default().to_string();
    match msg_type {
        "ticker" => {
            if let Some(tx) = tickers.lock().unwrap().get(&market_ticker).cloned() {
                let now = now_ms();
                let bid = dec(m.get("yes_bid_dollars"));
                let ask = dec(m.get("yes_ask_dollars"));
                let last = dec(m.get("last_price_dollars"));
                let t = Ticker {
                    symbol: market_ticker.clone(),
                    timestamp: Some(now),
                    datetime: iso8601(now),
                    bid,
                    ask,
                    last,
                    close: last,
                    average: match (bid, ask) {
                        (Some(b), Some(a)) => Some((b + a) / rust_decimal::Decimal::from(2)),
                        _ => None,
                    },
                    info: m.clone(),
                    ..Ticker::default()
                };
                let _ = tx.send(t);
            }
        }
        "trade" => {
            if let Some(tx) = trades.lock().unwrap().get(&market_ticker).cloned() {
                let _ = tx.send(vec![parse_trade(m, &market_ticker)]);
            }
        }
        "orderbook_snapshot" => {
            if let (Some(store), Some(tx)) = (
                book_stores.lock().unwrap().get_mut(&market_ticker),
                books.lock().unwrap().get(&market_ticker).cloned(),
            ) {
                let bids = collect_levels(m.get("bids"));
                let asks = collect_levels(m.get("asks"));
                store.reset(&bids, &asks);
                let _ = tx.send(store.snapshot(&market_ticker, None, None, m.clone()));
            }
        }
        "orderbook_delta" => {
            let mut stores = book_stores.lock().unwrap();
            let books = books.lock().unwrap();
            if let (Some(store), Some(tx)) = (
                stores.get_mut(&market_ticker),
                books.get(&market_ticker).cloned(),
            ) {
                // kalshi 只返回 bids(YES 视角);delta 的 ask 侧由 no 反转
                let mut bids = collect_levels(m.get("bids"));
                let mut asks = collect_levels(m.get("asks"));
                if !bids.is_empty() || !asks.is_empty() {
                    // 先清空再写回(增量格式:同价覆盖)
                    for (p, s) in bids.drain(..) {
                        store.apply_polymarket(
                            &[crate::realtime::orderbook::PriceChange { price: p, size: s }],
                            "BUY",
                        );
                    }
                    for (p, s) in asks.drain(..) {
                        store.apply_polymarket(
                            &[crate::realtime::orderbook::PriceChange { price: p, size: s }],
                            "SELL",
                        );
                    }
                    let _ = tx.send(store.snapshot(&market_ticker, None, None, m.clone()));
                }
            }
        }
        "fill" => {
            if let Some(tx) = my_trades.as_ref() {
                let price = dec(m.get("yes_price")).or_else(|| dec(m.get("price")));
                let amount = dec(m.get("count"));
                let t = Trade {
                    id: m["fill_id"].as_str().map(str::to_string),
                    timestamp: m["created_time"].as_str().and_then(parse_iso_ms),
                    datetime: m["created_time"].as_str().map(str::to_string),
                    symbol: m["ticker"].as_str().map(str::to_string),
                    side: m["side"].as_str().map(|s| s.to_lowercase()),
                    price,
                    amount,
                    cost: match (price, amount) {
                        (Some(p), Some(a)) => Some(p * a),
                        _ => None,
                    },
                    info: m.clone(),
                    ..Trade::default()
                };
                let _ = tx.send(vec![t]);
            }
        }
        "order" => {
            if let Some(tx) = orders.as_ref() {
                let o = rest.parse_order(m);
                let _ = tx.send(vec![o]);
            }
        }
        "market_positions" => {
            if let Some(tx) = positions.as_ref() {
                let mut out = Vec::new();
                if let Some(arr) = m.get("market_positions").and_then(Value::as_array) {
                    for p in arr {
                        out.push(Position {
                            symbol: p["ticker"].as_str().map(str::to_string),
                            id: p["market_id"].as_str().map(str::to_string),
                            contracts: dec(p.get("position")),
                            info: p.clone(),
                            ..Position::default()
                        });
                    }
                }
                let _ = tx.send(out);
            }
        }
        _ => {}
    }
}

fn parse_trade(m: &Value, market_ticker: &str) -> Trade {
    let price = dec(m.get("yes_price_dollars")).or_else(|| dec(m.get("price")));
    let amount = dec(m.get("count"));
    Trade {
        id: m["trade_id"].as_str().map(str::to_string),
        timestamp: m["created_time"].as_str().and_then(parse_iso_ms),
        datetime: m["created_time"].as_str().map(str::to_string),
        symbol: Some(market_ticker.to_string()),
        side: m["taker_side"].as_str().map(|s| s.to_lowercase()),
        price,
        amount,
        cost: match (price, amount) {
            (Some(p), Some(a)) => Some(p * a),
            _ => None,
        },
        info: m.clone(),
        ..Trade::default()
    }
}

fn parse_iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

impl Realtime for KalshiWs {
    async fn watch_ticker(&self, symbol: &str, _params: Params) -> Result<Ticker> {
        let ticker = self.market_ticker(symbol).await?;
        self.subscribe_market(&ticker, "ticker").await?;
        let rx = {
            let mut map = self.tickers.lock().unwrap();
            if !map.contains_key(&ticker) {
                let (tx, _) = watch::channel(Ticker::default());
                map.insert(ticker.clone(), tx.clone());
            }
            map.get(&ticker).cloned().unwrap().subscribe()
        };
        let mut rx = rx;
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_trades(
        &self,
        symbol: &str,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Trade>> {
        let ticker = self.market_ticker(symbol).await?;
        self.subscribe_market(&ticker, "trade").await?;
        let rx = {
            let mut map = self.trades.lock().unwrap();
            if !map.contains_key(&ticker) {
                let (tx, _) = watch::channel(vec![]);
                map.insert(ticker.clone(), tx.clone());
            }
            map.get(&ticker).cloned().unwrap().subscribe()
        };
        let mut rx = rx;
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_order_book(
        &self,
        symbol: &str,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<OrderBook> {
        let ticker = self.market_ticker(symbol).await?;
        self.subscribe_market(&ticker, "orderbook_delta").await?;
        let rx = {
            let mut stores = self.book_stores.lock().unwrap();
            let mut books = self.books.lock().unwrap();
            if !stores.contains_key(&ticker) {
                let store = OrderBookStore::new(0);
                let (tx, _) = watch::channel(store.snapshot(&ticker, None, None, Value::Null));
                stores.insert(ticker.clone(), store);
                books.insert(ticker.clone(), tx.clone());
            }
            books.get(&ticker).cloned().unwrap().subscribe()
        };
        let mut rx = rx;
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_ohlcv(
        &self,
        _symbol: &str,
        _timeframe: &str,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<OHLCV>> {
        Err(Error::not_supported("watch_ohlcv(kalshi)"))
    }

    async fn watch_balance(&self, _params: Params) -> Result<Balances> {
        Err(Error::not_supported("watch_balance(kalshi)"))
    }

    async fn watch_orders(
        &self,
        _symbol: Option<&str>,
        _since: Option<i64>,
        _limit: Option<i64>,
        _params: Params,
    ) -> Result<Vec<Order>> {
        let tx = self.ensure_conn().await?;
        let frame = json!({
            "id": 2,
            "cmd": "subscribe",
            "params": {"channels": ["order"]},
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        let rx = self.orders.lock().unwrap().clone().unwrap().subscribe();
        let mut rx = rx;
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
        let tx = self.ensure_conn().await?;
        let frame = json!({
            "id": 3,
            "cmd": "subscribe",
            "params": {"channels": ["fill"]},
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        let rx = self.my_trades.lock().unwrap().clone().unwrap().subscribe();
        let mut rx = rx;
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }

    async fn watch_positions(
        &self,
        _symbols: Option<&[&str]>,
        _params: Params,
    ) -> Result<Vec<Position>> {
        let tx = self.ensure_conn().await?;
        let frame = json!({
            "id": 4,
            "cmd": "subscribe",
            "params": {"channels": ["market_positions"]},
        })
        .to_string();
        tx.send_modify(|list| list.push(frame));
        let rx = self.positions.lock().unwrap().clone().unwrap().subscribe();
        let mut rx = rx;
        rx.changed().await.map_err(ws_err)?;
        Ok(rx.borrow().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 离线重放(候选 3 解析合一):WS order 消息经共享 REST parse_order。
    #[tokio::test]
    async fn replay_ws_order_shared_parse() {
        let rest = std::sync::Arc::new(crate::adapters::Kalshi::new(Config::new()).unwrap());
        let tickers = std::sync::Arc::new(Mutex::new(HashMap::new()));
        let trades = std::sync::Arc::new(Mutex::new(HashMap::new()));
        let books = std::sync::Arc::new(Mutex::new(HashMap::new()));
        let book_stores = std::sync::Arc::new(Mutex::new(HashMap::new()));
        let (tx_o, rx_o) = watch::channel(vec![]);
        let orders = Some(tx_o);
        let msg = json!({
            "type": "order",
            "msg": {
                "status": "resting", "order_id": "o1",
                "created_time": "2026-08-12T03:00:00Z", "ticker": "X:YES",
                "side": "yes", "price": "0.5", "count": "10",
                "fill_count": "0", "remaining_count": "10"
            }
        });
        dispatch(
            msg,
            &tickers,
            &trades,
            &books,
            &book_stores,
            &orders,
            &None,
            &None,
            &rest,
        );
        let os = rx_o.borrow().clone();
        assert_eq!(os.len(), 1);
        assert_eq!(os[0].status.as_deref(), Some("open"));
        assert_eq!(os[0].order_type.as_deref(), Some("limit"));
        assert_eq!(os[0].id.as_deref(), Some("o1"));
    }
}
