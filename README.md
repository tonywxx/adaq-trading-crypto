# adaq-trading-crypto

[![Crates.io](https://img.shields.io/crates/v/adaq-trading-crypto.svg)](https://crates.io/crates/adaq-trading-crypto)
[![docs.rs](https://docs.rs/adaq-trading-crypto/badge.svg)](https://docs.rs/adaq-trading-crypto)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue.svg)](https://blog.rust-lang.org)

A unified trading interface for **crypto and prediction markets**, written in Rust for the
[AdaQ](https://github.com/tonywxx/adaq) quantitative-trading platform.

The API surface and the unified data structures are aligned with
[ccxt](https://github.com/ccxt/ccxt): the same `snake_case` method names
(`fetch_ohlcv`, `create_order`, …), the same field names, and typed structs that keep the raw
exchange response under an `info` field as an escape hatch.

> 📘 简体中文文档见 [README.zh-CN.md](./README.zh-CN.md).

## Features

- **ccxt-aligned unified REST API** via the `Exchange` trait — `snake_case` method names and
  field names, so code ports almost line-for-line from ccxt.
- **109 adapters**: 14 hand-written (10 curated crypto exchanges + Hyperliquid + 3 prediction
  markets) and 95 generated from ccxt `describe()` by `scripts/gen_adapters.py` and a
  describe-driven generic engine (`generic.rs`).
- **Typed, serde-tolerant data structures** — `Market`, `Ticker`, `OrderBook`, `OHLCV`, `Order`,
  `Trade`, `Position`, `Balance`, … — with `info` carrying the raw response for full fidelity.
- **Exact decimal arithmetic** — `rust_decimal` by default, plus a `Precise` module that mirrors
  ccxt's `Precise` semantics and a `decimal_to_precision` port of `decimalToPrecision`.
- **Optional synchronous wrapper** (`sync` feature) — use the API without managing an async
  runtime.
- **Optional real-time WebSocket layer** (`realtime` feature) — 8 `watch_*` channels.
- **Prediction-market support** — Kalshi, Polymarket (EIP-712 order signing), Manifold (native),
  plus Limitless / Myriad / Opinion through the generated set.
- **Apache-2.0** licensed; ccxt-derived parsing logic retains its MIT notice (see `NOTICE`).

## Installation

```toml
[dependencies]
adaq-trading-crypto = { version = "1", features = ["binance", "okx"] }
```

Enable only the exchanges you need (each is a Cargo feature), plus optional `realtime` /
`sync`. The default features are `binance`, `okx`, `kalshi`, `polymarket`. Use the `full`
feature to compile everything.

## Quick Start

### Async

```rust
use adaq_trading_crypto::{Config, Exchange};
use adaq_trading_crypto::adapters::Binance;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut cfg = Config::new();
    cfg.api_key = Some("YOUR_KEY".into());
    cfg.secret  = Some("YOUR_SECRET".into());

    let ex = Binance::new(cfg)?;
    let ticker = ex.fetch_ticker("BTC/USDT", Default::default()).await?;
    println!("last = {}", ticker.last.unwrap());
    Ok(())
}
```

### Sync (no runtime to manage)

```rust
use adaq_trading_crypto::sync::BlockingExchange;
use adaq_trading_crypto::{Config, Exchange};
use adaq_trading_crypto::adapters::Binance;

let ex = BlockingExchange::new(Binance::new(Config::new())?)?;
let ticker = ex.fetch_ticker("BTC/USDT", Default::default())?;
println!("last = {}", ticker.last.unwrap());
```

### Prediction market (Polymarket)

```rust
use adaq_trading_crypto::adapters::Polymarket;
use adaq_trading_crypto::{Config, Exchange};

let ex = Polymarket::new(Config::new())?;
let markets = ex.fetch_markets(Default::default()).await?;
```

## Supported Exchanges

### Hand-written adapters (14)

| Feature       | Exchange        | Scope                                                                |
| ------------- | --------------- | -------------------------------------------------------------------- |
| `binance`     | Binance         | REST public + private, WS 8 channels                                 |
| `okx`         | OKX             | REST public + private, WS 8 channels                                 |
| `bybit`       | Bybit           | REST public + private, WS 8 channels                                 |
| `kraken`      | Kraken          | REST public + private, WS 8 channels (no `positions` channel)        |
| `coinbase`    | Coinbase        | REST public + private (v3 Advanced Trade)                            |
| `bitget`      | Bitget          | REST public + private                                                |
| `gate`        | Gate.io         | REST public + private                                                |
| `mexc`        | MEXC            | REST public + private                                                |
| `htx`         | HTX (Huobi)     | REST public + private                                                |
| `kucoin`      | KuCoin          | REST public + private                                                |
| `hyperliquid` | Hyperliquid     | DEX: public market data (markets / tickers / ohlcv / order_book)     |
| `kalshi`      | Kalshi          | REST full (incl. order placement), WS private channels               |
| `polymarket`  | Polymarket      | REST + EIP-712 order signing, WS 5 channels                          |
| `manifold`    | Manifold        | Native adapter (no ccxt reference): markets / ticker / trades        |

### Generated adapters (95)

The remaining exchanges live under the `generated` feature group; each is gated by its own Cargo
feature (e.g. `alpaca`, `apex`, `deribit`, `krakenfutures`, …). They are produced by
`scripts/gen_adapters.py` from ccxt `describe()` and share a single describe-driven engine
(`generic.rs`). They cover the best-effort common REST surface
(markets / tickers / ohlcv / order_book / trades / balance / orders / create_order /
cancel_order / …) via best-effort field parsing. Methods that need exchange-specific signing stay
`NotSupported` and defer to the curated set. This includes the three long-tail prediction markets
from the `ccxt.prediction` namespace — **Limitless / Myriad / Opinion** — through the same
pipeline, while Kalshi / Polymarket / Manifold remain hand-written.

## Feature Flags

| Feature           | Effect                                                                 |
| ----------------- | ---------------------------------------------------------------------- |
| per-exchange      | Compile in that adapter (e.g. `binance`, `kraken`, `polymarket`, …)    |
| `realtime`        | WebSocket `watch_*` methods (the 8 channels below)                     |
| `sync`            | Blocking wrapper over the async API (`sync::BlockingExchange`)         |
| `prediction`      | `kalshi` + `polymarket` + `manifold` (hand-written prediction markets) |
| `full`            | All exchanges (curated + generated) + `realtime`                       |
| _default_         | `binance`, `okx`, `kalshi`, `polymarket`                              |

## Architecture

Adapters follow the **HttpCore + four-seam** model (ADR-0013):

- **`HttpCore`** — an exchange-agnostic deep module that owns the HTTP request skeleton, market
  caching, client-side pagination/filtering, safe field extraction, and `Precise` arithmetic.
- **Four seams** — each adapter only fills: `describe` (endpoint paths/parameters), `sign`
  (signing algorithm), `handle_errors` (error-code mapping), and field mapping (`parse`
  overrides). Generated adapters fill only the `describe` seam and reuse `HttpCore` unchanged.

Prediction-market specifics (outcome context, synthetic tickers, RSA-PSS / EIP-712 / ECDSA
signing) live in the adapters, never in the core.

The unified data structures in `types` use `snake_case` field names aligned with ccxt and keep the
raw response under `info` for full fidelity.

## Real-time (WebSocket)

Enable `realtime` to use the 8 `watch_*` channels on the `Realtime` trait:
`watch_ticker`, `watch_order_book`, `watch_trades`, `watch_ohlcv`, `watch_balance`,
`watch_orders`, `watch_my_trades`, `watch_positions` (per ADR-0009). The `watch_*` layer reuses
the REST `parse_*` parsers (ADR-0015) and shares an incremental order-book engine. The `sync`
feature covers REST only; the WebSocket layer requires async.

## Numerical Accuracy

Prices and quantities use exact decimal arithmetic (`rust_decimal` by default). A public `precise`
module mirrors ccxt's `Precise` semantics, and `decimal_to_precision(value, mode, precision,
rounding, padding)` replicates ccxt's `decimalToPrecision` (decimal places / significant digits /
tick size; round / truncate / up / down).

## Error Handling

A unified `Error` wraps an `ErrorKind` that mirrors ccxt's exception tree, carrying context,
retryability, and `From` conversions for ergonomic `?` propagation.

## Testing & Quality

- **Contract tests** — the unified method surface is extracted from the `Exchange` / `Realtime`
  traits at build time (see `build.rs`) and asserted for completeness; this is a release gate.
- **Differential tests** — parsers are compared field-by-field against real ccxt output recorded
  from identical raw responses (offline, deterministic fixtures in CI).
- `clippy -D warnings`, `rustfmt`, MSRV **1.85** (edition **2024**), `cargo nextest`.

## Development

Regenerate the 95 transpiled adapters from a local ccxt checkout:

```bash
python3 scripts/gen_adapters.py --ccxt /path/to/ccxt
```

The generator emits per-exchange modules under `src/adapters/generated/` from ccxt `describe()`;
the runtime lives in `src/generic.rs`.

## License

Apache-2.0. Adapter parsing logic ported from ccxt retains its MIT notice — see `NOTICE`.

---

📘 简体中文文档见 [README.zh-CN.md](./README.zh-CN.md)。
