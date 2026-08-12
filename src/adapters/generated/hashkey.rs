//! HashKey Global (`hashkey`) 转译适配器 — 由 `scripts/gen_adapters.py`
//! 从 ccxt 4.5.73 `describe()` 自动生成(best-effort 批量补齐)。
//! 保留 MIT 声明见仓库 `NOTICE`;精确性由精选手写集保证。
//! 类别: Cex。

#![allow(clippy::too_many_arguments)]

use crate::generic::{ApiSpec, Endpoint, MarketKind};

/// ccxt `describe()` 静态快照(描述驱动引擎的输入)。
pub static SPEC: ApiSpec = ApiSpec {
    id: "hashkey",
    name: "HashKey Global",
    version: "v1",
    rate_limit_ms: 100,
    has: &["addMargin", "cancelAllOrders", "cancelOrder", "cancelOrders", "createLimitOrder", "createMarketBuyOrderWithCost", "createMarketOrder", "createMarketOrderWs", "createOrder", "createOrders", "createReduceOnlyOrder", "createStopLimitOrder", "createStopMarketOrder", "createStopOrder", "createTriggerOrder", "editOrder", "fetchAccounts", "fetchBalance", "fetchCanceledAndClosedOrders", "fetchCanceledOrders", "fetchClosedOrder", "fetchCurrencies", "fetchCurrenciesWs", "fetchDepositAddress", "fetchDeposits", "fetchFundingRate", "fetchFundingRateHistory", "fetchFundingRates", "fetchL2OrderBook", "fetchLastPrices", "fetchLedger", "fetchLeverage", "fetchLeverageTiers", "fetchMarketLeverageTiers", "fetchMarkets", "fetchMyTrades", "fetchOHLCV", "fetchOpenOrders", "fetchOrder", "fetchOrderBook", "fetchPositions", "fetchPositionsForSymbol", "fetchStatus", "fetchTicker", "fetchTickers", "fetchTime", "fetchTrades", "fetchTradingFee", "fetchTradingFees", "fetchWithdrawals", "privateAPI", "publicAPI", "reduceMargin", "setLeverage", "setMarginMode", "spot", "swap", "transfer", "withdraw"],
    endpoints: &[
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/exchangeInfo", path: "api/v1/exchangeInfo", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/depth", path: "quote/v1/depth", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/trades", path: "quote/v1/trades", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/klines", path: "quote/v1/klines", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/ticker/24hr", path: "quote/v1/ticker/24hr", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/ticker/price", path: "quote/v1/ticker/price", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/ticker/bookTicker", path: "quote/v1/ticker/bookTicker", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "quote/v1/depth/merged", path: "quote/v1/depth/merged", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/time", path: "api/v1/time", auth: false },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/spot/order", path: "api/v1/spot/order", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/spot/openOrders", path: "api/v1/spot/openOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/futures/order", path: "api/v1/futures/order", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/futures/openOrders", path: "api/v1/futures/openOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/futures/balance", path: "api/v1/futures/balance", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/vipInfo", path: "api/v1/account/vipInfo", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account", path: "api/v1/account", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/trades", path: "api/v1/account/trades", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/type", path: "api/v1/account/type", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/chainType", path: "api/v1/account/chainType", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/checkApiKey", path: "api/v1/account/checkApiKey", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/balanceFlow", path: "api/v1/account/balanceFlow", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/spot/subAccount/openOrders", path: "api/v1/spot/subAccount/openOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/subAccount/trades", path: "api/v1/subAccount/trades", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/futures/subAccount/openOrders", path: "api/v1/futures/subAccount/openOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/deposit/address", path: "api/v1/account/deposit/address", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/depositOrders", path: "api/v1/account/depositOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "GET", key: "api/v1/account/withdrawOrders", path: "api/v1/account/withdrawOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "POST", key: "api/v1/spot/order", path: "api/v1/spot/order", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "POST", key: "api/v1.1/spot/order", path: "api/v1.1/spot/order", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "POST", key: "api/v1/futures/order", path: "api/v1/futures/order", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "POST", key: "api/v1/account/assetTransfer", path: "api/v1/account/assetTransfer", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "POST", key: "api/v1/account/authAddress", path: "api/v1/account/authAddress", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "POST", key: "api/v1/account/withdraw", path: "api/v1/account/withdraw", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "DELETE", key: "api/v1/spot/order", path: "api/v1/spot/order", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "DELETE", key: "api/v1/spot/openOrders", path: "api/v1/spot/openOrders", auth: true },
        Endpoint { base: "https://api-glb.hashkey.com", verb: "DELETE", key: "api/v1/futures/order", path: "api/v1/futures/order", auth: true },
    ],
    taker: 0.0,
    maker: 0.0,
    timeframes: &["1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "1w", "1M"],
    kind: MarketKind::Cex,
};

crate::impl_generated_adapter!(Hashkey, &SPEC);
