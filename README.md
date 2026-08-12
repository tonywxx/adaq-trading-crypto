# adaq-trading-crypto

Unified trading interface (Rust) for crypto and prediction markets, for the AdaQ quantitative-trading platform. API surface and unified data structures are aligned with [ccxt](https://github.com/ccxt/ccxt): same snake_case method names (`fetch_ohlcv`, `create_order`, ...), same field names, typed structs with `info` carrying the raw exchange response.

## Adapters

Curated set (ADR-0005): 7 crypto exchanges + 2 prediction markets + 1 native prediction adapter, each with differential testing against real ccxt output.

| Feature | Exchange | Scope |
|---|---|---|
| `binance` | Binance | REST public + private, WS 8 channels |
| `okx` | OKX | REST public + private, WS 8 channels |
| `bybit` | Bybit | REST public + private, WS 8 channels |
| `kraken` | Kraken | REST public + private, WS 8 channels (no positions channel) |
| `coinbase` | Coinbase | REST public + private (v3 Advanced Trade) |
| `bitget` | Bitget | REST public + private |
| `gate` | Gate.io | REST public + private |
| `mexc` | MEXC | REST public + private |
| `htx` | Huobi | REST public + private |
| `kucoin` | KuCoin | REST public + private |
| `kalshi` | Kalshi | REST full (incl. order placement), WS private channels |
| `polymarket` | Polymarket | REST + EIP-712 order signing, WS 5 channels |
| `manifold` | Manifold | Native adapter (no ccxt reference): markets / ticker / trades |

## Features

Per-exchange features (as above) plus:

- `realtime` — WebSocket `watch_*` methods (core 8 channels: ticker / order_book / trades / ohlcv / balance / orders / my_trades / positions, per ADR-0009).
- `sync` — blocking wrapper over the async API (no manual runtime needed).
- `prediction` — `kalshi` + `polymarket` + `manifold`.
- `full` — all exchanges + `realtime`.

Default features: `binance`, `okx`, `kalshi`, `polymarket`.

## Usage

```rust
use adaq_trading_crypto::{Config, Exchange};
use adaq_trading_crypto::adapters::Binance;

let ex = Binance::new(Config::new())?;
let ticker = ex.fetch_ticker("BTC/USDT", Default::default()).await?;
println!("{}", ticker.last.unwrap());
```

Synchronous scripts (no async runtime to manage):

```rust
use adaq_trading_crypto::{Config, sync::BlockingExchange};
use adaq_trading_crypto::adapters::Binance;

let ex = BlockingExchange::new(Binance::new(Config::new())?)?;
let ticker = ex.fetch_ticker("BTC/USDT", Default::default())?;
```

## Quality gates

- **Contract tests** — the unified method surface (150+ REST + 8 WS methods) is extracted from the trait at build time and asserted for completeness (release gate).
- **Differential tests** — our parsers are compared field-by-field against real ccxt output recorded from the same raw responses (fixtures, offline, deterministic in CI).
- `clippy -D warnings`, `rustfmt`, MSRV 1.85 (edition 2024), `cargo nextest`.

## License

Apache-2.0. Adapter parsing logic ported from ccxt retains its MIT notice — see `NOTICE`.
