//! 实时/WebSocket 面(ADR-0009):核心 8 频道 watch_*,`realtime` feature 门控。
//!
//! - [`orderbook`]:core 共享的增量订单簿引擎(ADR-0011);
//! - [`ws`]:连接基础设施(订阅命令通道 + 消息分发);
//! - [`binance`] / [`polymarket`] / [`kalshi`]:各交易所 Realtime 实现。

pub mod orderbook;
pub mod ws;

#[cfg(feature = "realtime")]
pub mod binance;
#[cfg(feature = "realtime")]
pub mod kalshi;
#[cfg(feature = "realtime")]
pub mod polymarket;

#[cfg(feature = "realtime")]
pub use binance::BinanceWs;
#[cfg(feature = "realtime")]
pub use kalshi::KalshiWs;
#[cfg(feature = "realtime")]
pub use polymarket::PolymarketWs;
