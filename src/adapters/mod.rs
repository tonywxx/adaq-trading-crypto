//! 交易所适配器(ADR-0005):每个交易所一个模块,按 feature 编译。

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
