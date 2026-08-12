//! Lighter (`lighter`) 转译适配器 — 由 `scripts/gen_adapters.py`
//! 从 ccxt 4.5.73 `describe()` 自动生成(best-effort 批量补齐)。
//! 保留 MIT 声明见仓库 `NOTICE`;精确性由精选手写集保证。
//! 类别: Dex。

#![allow(clippy::too_many_arguments)]

use crate::generic::{ApiSpec, Endpoint, MarketKind};

/// ccxt `describe()` 静态快照(描述驱动引擎的输入)。
pub static SPEC: ApiSpec = ApiSpec {
    id: "lighter",
    name: "Lighter",
    version: "v1",
    rate_limit_ms: 1000,
    has: &["addMargin", "cancelAllOrders", "cancelAllOrdersAfter", "cancelOrder", "createLimitOrder", "createMarketOrder", "createMarketOrderWs", "createOrder", "editOrder", "fetchAccounts", "fetchBalance", "fetchClosedOrders", "fetchCurrencies", "fetchCurrenciesWs", "fetchDeposits", "fetchFundingRates", "fetchL2OrderBook", "fetchMarkets", "fetchMyTrades", "fetchOHLCV", "fetchOpenOrders", "fetchOrderBook", "fetchPosition", "fetchPositions", "fetchStatus", "fetchTicker", "fetchTickers", "fetchTime", "fetchTransfers", "fetchWithdrawals", "privateAPI", "publicAPI", "reduceMargin", "sandbox", "setLeverage", "setMargin", "setMarginMode", "spot", "swap", "transfer", "withdraw"],
    endpoints: &[
        Endpoint { base: "https://mainnet.{hostname}", verb: "GET", key: "account", path: "account", auth: false },
        Endpoint { base: "https://mainnet.{hostname}", verb: "GET", key: "candles", path: "candles", auth: false },
        Endpoint { base: "https://mainnet.{hostname}", verb: "GET", key: "trades", path: "trades", auth: true },
    ],
    taker: 0.0,
    maker: 0.0,
    timeframes: &["1m", "5m", "15m", "30m", "1h", "4h", "12h", "1d", "1w"],
    kind: MarketKind::Dex,
};

crate::impl_generated_adapter!(Lighter, &SPEC);
