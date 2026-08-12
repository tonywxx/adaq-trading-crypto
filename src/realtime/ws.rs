//! WebSocket 连接基础设施:连接建立(可选自定义头)、订阅命令通道、接收循环。
//!
//! 仅编译于 `realtime` feature(ADR-0009)。每个交易所的 watch_* 适配器
//! 在后台 spawn 一个 [`WsSession`]:连接任务用 `tokio::select!` 同时处理
//! 收到的消息与调用方的订阅命令(mpsc),消息解析后分发给 `watch` channel;
//! watch_* 方法等待 [`tokio::sync::watch`] 的下一条更新。

#[cfg(feature = "realtime")]
use std::sync::Arc;

#[cfg(feature = "realtime")]
use futures_util::{SinkExt, StreamExt};
use reqwest::header::HeaderMap;
use tokio::net::TcpStream;
#[cfg(feature = "realtime")]
use tokio::sync::watch;
#[cfg(feature = "realtime")]
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::{Error, ErrorKind, Result};

/// WS 连接类型。
type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// 建立 WS 连接(带自定义请求头,如 kalshi 认证头)。
pub async fn connect(url: &str, headers: &HeaderMap) -> Result<WsStream> {
    let mut req = url.into_client_request().map_err(|e| {
        Error::new(
            ErrorKind::NetworkError,
            format!("invalid ws url {url}: {e}"),
        )
    })?;
    for (k, v) in headers.iter() {
        req.headers_mut().insert(k.clone(), v.clone());
    }
    let (ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| Error::new(ErrorKind::NetworkError, format!("ws connect {url}: {e}")))?;
    Ok(ws)
}

/// 启动一个长连接会话。
///
/// - `sub_rx`:订阅命令通道,调用方通过它发送订阅 JSON 帧(如 binance `SUBSCRIBE`);
/// - `dispatch`:收到的 JSON 消息分发回调;
/// - 返回连接就绪信号(watch)。
#[cfg(feature = "realtime")]
impl<'a> WsSession<'a> {
    pub fn spawn<F>(
        url: String,
        headers: HeaderMap,
        sub_rx: watch::Receiver<Vec<String>>,
        dispatch: F,
    ) -> watch::Receiver<bool>
    where
        F: Fn(serde_json::Value) + Send + Sync + 'static,
    {
        let (tx, rx) = watch::channel(false);
        let dispatch = Arc::new(dispatch);
        tokio::spawn(async move {
            // 本地已发送订阅帧集合(重连后无需重发)
            let mut sent: std::collections::HashSet<String> = std::collections::HashSet::new();
            loop {
                let ws = match connect(&url, &headers).await {
                    Ok(w) => w,
                    Err(e) => {
                        eprintln!("[realtime] connect {url} failed: {e}; retry in 3s");
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }
                };
                let _ = tx.send(true);
                let mut session = WsSession {
                    ws,
                    sub_rx: sub_rx.clone(),
                    dispatch: dispatch.clone(),
                    sent: &mut sent,
                };
                if let Err(e) = session.run().await {
                    eprintln!("[realtime] session {url} ended: {e}; reconnecting");
                }
                let _ = tx.send(false);
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
        rx
    }
} // impl WsSession

#[cfg(feature = "realtime")]
pub(crate) struct WsSession<'a> {
    ws: WsStream,
    sub_rx: watch::Receiver<Vec<String>>,
    dispatch: Arc<dyn Fn(serde_json::Value) + Send + Sync>,
    sent: &'a mut std::collections::HashSet<String>,
}

#[cfg(feature = "realtime")]
impl<'a> WsSession<'a> {
    async fn run(&mut self) -> Result<()> {
        loop {
            tokio::select! {
                msg = self.ws.next() => {
                    let msg = msg.ok_or_else(|| Error::new(ErrorKind::NetworkError, "ws stream ended"))?
                        .map_err(|e| Error::new(ErrorKind::NetworkError, format!("ws recv: {e}")))?;
                    match msg {
                        Message::Text(text) => {
                            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                                (self.dispatch)(value);
                            }
                        }
                        Message::Ping(payload) => {
                            self.ws.send(Message::Pong(payload)).await.ok();
                        }
                        Message::Close(_) => {
                            return Err(Error::new(ErrorKind::NetworkError, "ws closed"));
                        }
                        _ => {}
                    }
                }
                _ = self.sub_rx.changed() => {
                    let frames = self.sub_rx.borrow().clone();
                    for frame in frames {
                        if self.sent.insert(frame.clone()) {
                            self.ws.send(Message::Text(frame.into())).await
                                .map_err(|e| Error::new(ErrorKind::NetworkError, format!("ws send sub: {e}")))?;
                        }
                    }
                }
            }
        }
    }
}

/// 订阅管理:已建立的订阅(避免重复订阅)。
#[derive(Debug, Default)]
pub struct SubscriptionSet {
    streams: HashMap<String, usize>,
}

impl SubscriptionSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册订阅,返回是否首次(需要真正发送订阅命令)。
    pub fn register(&mut self, key: &str) -> bool {
        let count = self.streams.entry(key.to_string()).or_insert(0);
        if *count == 0 {
            *count = 1;
            true
        } else {
            *count += 1;
            false
        }
    }
}

use std::collections::HashMap;

/// 解析 Decimal(兼容字符串/数字)。
pub fn value_decimal(v: &serde_json::Value) -> Option<rust_decimal::Decimal> {
    match v {
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Number(n) => n.to_string().parse().ok(),
        _ => None,
    }
}
