//! 交易所适配器(ADR-0005):每个交易所一个模块,按 feature 编译。

/// 预测市场 outcome 索引(kalshi/polymarket 共用,ADR-0013 适配器侧共享)。
#[cfg(any(feature = "kalshi", feature = "polymarket"))]
mod outcome;

#[cfg(feature = "binance")]
pub mod binance;

#[cfg(feature = "binance")]
pub use binance::{Binance, ID as BINANCE_ID};

#[cfg(feature = "coinbase")]
pub mod coinbase;

#[cfg(feature = "coinbase")]
pub use coinbase::{Coinbase, ID as COINBASE_ID};

#[cfg(feature = "kalshi")]
pub mod kalshi;

#[cfg(feature = "kalshi")]
pub use kalshi::{ID as KALSHI_ID, Kalshi};

#[cfg(feature = "polymarket")]
pub mod polymarket;

#[cfg(feature = "polymarket")]
pub use polymarket::{ID as POLYMARKET_ID, Polymarket};

#[cfg(feature = "okx")]
pub mod okx;

#[cfg(feature = "okx")]
pub use okx::{ID as OKX_ID, Okx};

#[cfg(feature = "bybit")]
pub mod bybit;

#[cfg(feature = "bybit")]
pub use bybit::{Bybit, ID as BYBIT_ID};

#[cfg(feature = "bitget")]
pub mod bitget;

#[cfg(feature = "bitget")]
pub use bitget::{Bitget, ID as BITGET_ID};

#[cfg(feature = "kraken")]
pub mod kraken;

#[cfg(feature = "kraken")]
pub use kraken::{ID as KRAKEN_ID, Kraken};

#[cfg(feature = "gate")]
pub mod gate;

#[cfg(feature = "gate")]
pub use gate::{Gate, ID as GATE_ID};

#[cfg(feature = "mexc")]
pub mod mexc;

#[cfg(feature = "mexc")]
pub use mexc::{ID as MEXC_ID, Mexc};

#[cfg(feature = "htx")]
pub mod htx;

#[cfg(feature = "htx")]
pub use htx::{Htx, ID as HTX_ID};

#[cfg(feature = "kucoin")]
pub mod kucoin;

#[cfg(feature = "kucoin")]
pub use kucoin::{ID as KUCOIN_ID, Kucoin};

#[cfg(feature = "manifold")]
pub mod manifold;

#[cfg(feature = "manifold")]
pub use manifold::{ID as MANIFOLD_ID, Manifold};

#[cfg(feature = "hyperliquid")]
pub mod hyperliquid;

#[cfg(feature = "hyperliquid")]
pub use hyperliquid::{Hyperliquid, ID as HYPERLIQUID_ID};

/// 转译生成的交易所适配器(由 `scripts/gen_adapters.py` 从 ccxt `describe()` 生成)。
/// 子模块各自按交易所 feature 门控;本模块本身常驻。
pub mod generated;
