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

// ===== 由 generated promote 为 curated(ADR-0017):手写完整交易 API =====

#[cfg(feature = "alpaca")]
pub mod alpaca;

#[cfg(feature = "alpaca")]
pub use alpaca::{Alpaca, ID as ALPACA_ID};

#[cfg(feature = "aster")]
pub mod aster;

#[cfg(feature = "aster")]
pub use aster::{Aster, ID as ASTER_ID};

#[cfg(feature = "binanceus")]
pub mod binanceus;

#[cfg(feature = "binanceus")]
pub use binanceus::{BinanceUs, ID as BINANCEUS_ID};

#[cfg(feature = "gemini")]
pub mod gemini;

#[cfg(feature = "gemini")]
pub use gemini::{Gemini, ID as GEMINI_ID};

#[cfg(feature = "hashkey")]
pub mod hashkey;

#[cfg(feature = "hashkey")]
pub use hashkey::{Hashkey, ID as HASHKEY_ID};

#[cfg(feature = "lighter")]
pub mod lighter;

#[cfg(feature = "lighter")]
pub use lighter::{ID as LIGHTER_ID, Lighter};

#[cfg(feature = "myokx")]
pub mod myokx;

#[cfg(feature = "myokx")]
pub use myokx::{ID as MYOKX_ID, MyOkx};

#[cfg(feature = "okxus")]
pub mod okxus;

#[cfg(feature = "okxus")]
pub use okxus::{ID as OKXUS_ID, OkxUs};

/// 转译生成的交易所适配器(由 `scripts/gen_adapters.py` 从 ccxt `describe()` 生成)。
/// 子模块各自按交易所 feature 门控;本模块本身常驻。
pub mod generated;
